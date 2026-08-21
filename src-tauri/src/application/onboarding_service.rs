use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Sqlite, Transaction};

use crate::{
    domain::{CurrencyCode, Household, Member, Timestamp},
    error::AppError,
    infrastructure::database::SqlitePool,
    state::AppState,
};

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CompleteOnboardingInput {
    pub household_name: String,
    pub base_currency: String,
    pub members: Vec<OnboardingMemberInput>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingMemberInput {
    pub name: String,
}

struct PreparedOnboarding {
    household: Household,
    members: Vec<Member>,
    now: Timestamp,
}

pub async fn complete_onboarding(
    state: &AppState,
    input: CompleteOnboardingInput,
) -> Result<(), AppError> {
    let prepared = prepare_onboarding(input)?;
    let database = state.writable_db()?;
    persist_onboarding(database, prepared).await
}

fn prepare_onboarding(input: CompleteOnboardingInput) -> Result<PreparedOnboarding, AppError> {
    if input.members.is_empty() {
        return Err(AppError::validation(
            "members",
            "At least one member is required.",
        ));
    }

    let now = Timestamp::now();
    let base_currency = CurrencyCode::parse_supported(&input.base_currency)
        .map_err(|error| remap_validation_field(error, "currency", "baseCurrency"))?;
    let household = Household::new(&input.household_name, base_currency, now.clone())
        .map_err(|error| remap_validation_field(error, "name", "householdName"))?;

    let mut members = Vec::with_capacity(input.members.len());
    for (index, member) in input.members.iter().enumerate() {
        let sort_order = i64::try_from(index).map_err(|_| AppError::Internal)?;
        let member = Member::new(
            household.id(),
            &member.name,
            None,
            None,
            sort_order,
            now.clone(),
        )
        .map_err(|error| remap_validation_field(error, "name", &format!("members.{index}.name")))?;
        members.push(member);
    }

    Ok(PreparedOnboarding {
        household,
        members,
        now,
    })
}

async fn persist_onboarding(
    database: &SqlitePool,
    prepared: PreparedOnboarding,
) -> Result<(), AppError> {
    let mut tx = database.begin().await.map_err(map_write_error)?;
    match persist_onboarding_in_transaction(&mut tx, prepared).await {
        Ok(()) => tx.commit().await.map_err(map_write_error),
        Err(error) => {
            if let Err(_rollback_error) = tx.rollback().await {
                tracing::error!(
                    event = "onboarding.rollback_failed",
                    "failed to roll back onboarding transaction"
                );
            }
            Err(error)
        }
    }
}

async fn persist_onboarding_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    prepared: PreparedOnboarding,
) -> Result<(), AppError> {
    let existing_households: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM households")
        .fetch_one(&mut **tx)
        .await
        .map_err(map_write_error)?;
    if existing_households > 0 {
        return Err(AppError::AlreadyOnboarded);
    }

    let timestamp = prepared.now.to_rfc3339();
    sqlx::query(
        "INSERT INTO households (id, name, base_currency, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(prepared.household.id().to_string())
    .bind(prepared.household.name())
    .bind(prepared.household.base_currency().as_str())
    .bind(&timestamp)
    .bind(&timestamp)
    .execute(&mut **tx)
    .await
    .map_err(map_write_error)?;

    for member in &prepared.members {
        sqlx::query(
            "INSERT INTO members (id, household_id, name, sort_order, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(member.id().to_string())
        .bind(member.household_id().to_string())
        .bind(member.name())
        .bind(member.sort_order())
        .bind(&timestamp)
        .bind(&timestamp)
        .execute(&mut **tx)
        .await
        .map_err(map_write_error)?;
    }

    let updated =
        sqlx::query("UPDATE app_settings SET last_household_id = ?, updated_at = ? WHERE id = 1")
            .bind(prepared.household.id().to_string())
            .bind(&timestamp)
            .execute(&mut **tx)
            .await
            .map_err(map_write_error)?;

    if updated.rows_affected() != 1 {
        tracing::error!(
            event = "onboarding.settings_update_failed",
            rows_affected = updated.rows_affected(),
            "onboarding did not update the singleton application settings row"
        );
        return Err(AppError::Internal);
    }

    crate::application::history_origin::insert_fresh_origin_in_tx(
        tx,
        prepared.household.id(),
        &prepared.now,
        crate::application::history_origin::HISTORY_ORIGIN_SCHEMA_VERSION,
    )
    .await?;

    crate::application::freshness_policy_service::ensure_default_policies_in_tx(
        tx,
        &prepared.household.id().to_string(),
    )
    .await
    .map_err(map_write_error)?;

    tracing::info!(
        event = "onboarding.complete",
        household_id = %prepared.household.id(),
        member_count = prepared.members.len(),
        "onboarding completed"
    );
    Ok(())
}

fn remap_validation_field(error: AppError, from: &str, to: &str) -> AppError {
    match error {
        AppError::Validation { field, message } if field == from => AppError::Validation {
            field: to.to_owned(),
            message,
        },
        other => other,
    }
}

fn map_write_error(error: sqlx::Error) -> AppError {
    if is_household_unique_conflict(&error) {
        return AppError::AlreadyOnboarded;
    }
    tracing::error!(event = "onboarding.write_failed", "onboarding write failed");
    AppError::from(error)
}

fn is_household_unique_conflict(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(database_error) => {
            let message = database_error.message();
            database_error.is_unique_violation() && message.contains("households")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

    use sqlx::Row;

    use super::{
        complete_onboarding, is_household_unique_conflict, CompleteOnboardingInput,
        OnboardingMemberInput,
    };
    use crate::{
        commands::bootstrap::bootstrap_impl,
        error::{AppError, ErrorCode},
        infrastructure::{database::connect_writable, database_bootstrap::DatabaseBootstrapStatus},
        state::AppState,
    };

    fn test_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "nestworth-phase4-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn valid_input() -> CompleteOnboardingInput {
        CompleteOnboardingInput {
            household_name: "Wang Family".to_owned(),
            base_currency: "CNY".to_owned(),
            members: vec![
                OnboardingMemberInput {
                    name: "Walt".to_owned(),
                },
                OnboardingMemberInput {
                    name: "Spouse".to_owned(),
                },
            ],
        }
    }

    async fn snapshot(
        state: &AppState,
    ) -> (
        Vec<(String, String, String)>,
        Vec<(String, i64)>,
        Option<String>,
    ) {
        let database = state.writable_db().expect("writable database");
        let households = sqlx::query("SELECT id, name, base_currency FROM households ORDER BY id")
            .fetch_all(database)
            .await
            .expect("households should load")
            .into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("id"),
                    row.get::<String, _>("name"),
                    row.get::<String, _>("base_currency"),
                )
            })
            .collect();
        let members = sqlx::query("SELECT name, sort_order FROM members ORDER BY sort_order, id")
            .fetch_all(database)
            .await
            .expect("members should load")
            .into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("name"),
                    row.get::<i64, _>("sort_order"),
                )
            })
            .collect();
        let last_household_id = sqlx::query_scalar::<_, Option<String>>(
            "SELECT last_household_id FROM app_settings WHERE id = 1",
        )
        .fetch_optional(database)
        .await
        .expect("settings should load")
        .flatten();
        (households, members, last_household_id)
    }

    #[test]
    fn empty_database_onboards_household_and_members() {
        tauri::async_runtime::block_on(async {
            let path = test_path("success");
            let _ = fs::remove_file(&path);
            let state = AppState::initialize(path.clone()).await;

            complete_onboarding(&state, valid_input())
                .await
                .expect("onboarding should succeed");

            let (households, members, last_household_id) = snapshot(&state).await;
            assert_eq!(households.len(), 1);
            assert_eq!(households[0].1, "Wang Family");
            assert_eq!(households[0].2, "CNY");
            assert_eq!(
                members,
                vec![("Walt".to_owned(), 0), ("Spouse".to_owned(), 1)]
            );
            assert_eq!(last_household_id.as_deref(), Some(households[0].0.as_str()));
            let policies: Vec<(String, i64)> = sqlx::query_as(
                "SELECT kind, review_interval_days
                 FROM freshness_policies
                 WHERE household_id = ? AND target_account_id IS NULL
                   AND target_instrument_id IS NULL AND target_currency_a IS NULL
                 ORDER BY kind",
            )
            .bind(&households[0].0)
            .fetch_all(state.writable_db().expect("writable database"))
            .await
            .expect("default policies should load");
            assert_eq!(policies.len(), 4);
            assert_eq!(policies[0], ("account_cash".to_owned(), 30));
            assert_eq!(policies[1], ("account_value".to_owned(), 30));
            assert_eq!(policies[2], ("fx_quote".to_owned(), 7));
            assert_eq!(policies[3], ("instrument_quote".to_owned(), 7));

            let bootstrap = bootstrap_impl(&state)
                .await
                .expect("bootstrap should succeed");
            match bootstrap {
                crate::commands::bootstrap::BootstrapDto::Ready {
                    onboarding_required,
                    household,
                    members: bootstrap_members,
                    ..
                } => {
                    assert!(!onboarding_required);
                    assert_eq!(household.expect("household").name, "Wang Family");
                    assert_eq!(
                        bootstrap_members
                            .iter()
                            .map(|member| member.name.as_str())
                            .collect::<Vec<_>>(),
                        vec!["Walt", "Spouse"]
                    );
                }
                other => panic!("expected ready bootstrap, got {other:?}"),
            }

            fs::remove_file(path).expect("test database should be removable");
        });
    }

    #[test]
    fn repeated_onboarding_is_already_onboarded_without_writes() {
        tauri::async_runtime::block_on(async {
            let path = test_path("repeat");
            let _ = fs::remove_file(&path);
            let state = AppState::initialize(path.clone()).await;
            complete_onboarding(&state, valid_input())
                .await
                .expect("first onboarding should succeed");

            let before = snapshot(&state).await;
            let before_hash = crate::test_support::stable_sqlite_hash(&path).await;
            let error = complete_onboarding(&state, valid_input())
                .await
                .expect_err("second onboarding should fail");
            assert!(matches!(error, AppError::AlreadyOnboarded));
            assert_eq!(error.into_command_error().code, ErrorCode::AlreadyOnboarded);
            assert_eq!(snapshot(&state).await, before);
            assert_eq!(
                crate::test_support::stable_sqlite_hash(&path).await,
                before_hash
            );

            fs::remove_file(path).expect("test database should be removable");
        });
    }

    #[test]
    fn invalid_input_does_not_write_business_rows() {
        tauri::async_runtime::block_on(async {
            let path = test_path("invalid");
            let _ = fs::remove_file(&path);
            let state = AppState::initialize(path.clone()).await;
            let before = snapshot(&state).await;

            let mut empty_members = valid_input();
            empty_members.members.clear();
            assert!(matches!(
                complete_onboarding(&state, empty_members).await,
                Err(AppError::Validation { field, .. }) if field == "members"
            ));

            let mut bad_name = valid_input();
            bad_name.household_name = "   ".to_owned();
            assert!(matches!(
                complete_onboarding(&state, bad_name).await,
                Err(AppError::Validation { field, .. }) if field == "householdName"
            ));

            let mut bad_currency = valid_input();
            bad_currency.base_currency = "cny".to_owned();
            assert!(matches!(
                complete_onboarding(&state, bad_currency).await,
                Err(AppError::Validation { field, .. }) if field == "baseCurrency"
            ));

            let mut bad_member = valid_input();
            bad_member.members[1].name = String::new();
            assert!(matches!(
                complete_onboarding(&state, bad_member).await,
                Err(AppError::Validation { field, .. }) if field == "members.1.name"
            ));

            assert_eq!(snapshot(&state).await, before);
            fs::remove_file(path).expect("test database should be removable");
        });
    }

    #[test]
    fn missing_settings_row_rolls_back_partial_onboarding() {
        tauri::async_runtime::block_on(async {
            let path = test_path("rollback");
            let _ = fs::remove_file(&path);
            let state = AppState::initialize(path.clone()).await;
            let database = state.writable_db().expect("writable database");
            sqlx::query("DELETE FROM app_settings")
                .execute(database)
                .await
                .expect("settings row should be removable for rollback fixture");

            let error = complete_onboarding(&state, valid_input())
                .await
                .expect_err("onboarding should fail without settings");
            assert!(matches!(error, AppError::Internal));

            let (households, members, last_household_id) = snapshot(&state).await;
            assert!(households.is_empty());
            assert!(members.is_empty());
            assert!(last_household_id.is_none());

            fs::remove_file(path).expect("test database should be removable");
        });
    }

    #[test]
    fn blocked_future_database_rejects_onboarding_without_writes() {
        tauri::async_runtime::block_on(async {
            let path = test_path("future");
            let _ = fs::remove_file(&path);
            let pool = connect_writable(&path, true)
                .await
                .expect("fixture database should open");
            sqlx::query(
                "CREATE TABLE _sqlx_migrations (version BIGINT PRIMARY KEY NOT NULL, description TEXT NOT NULL, installed_on TIMESTAMP NOT NULL, success BOOLEAN NOT NULL, checksum BLOB NOT NULL, execution_time BIGINT NOT NULL)",
            )
            .execute(&pool)
            .await
            .expect("migration metadata table should be created");
            sqlx::query(
                "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (999, 'future', CURRENT_TIMESTAMP, 1, zeroblob(32), 1)",
            )
            .execute(&pool)
            .await
            .expect("future migration row should be inserted");
            pool.close().await;

            let state = AppState::initialize(path.clone()).await;
            assert!(!state.is_writable());
            assert!(matches!(
                state.bootstrap_status(),
                DatabaseBootstrapStatus::UnsupportedNewerDatabase {
                    found: 999,
                    supported: 5
                }
            ));
            let before_hash = crate::test_support::stable_sqlite_hash(&path).await;

            let error = complete_onboarding(&state, valid_input())
                .await
                .expect_err("blocked database must not accept onboarding");
            assert!(matches!(
                error,
                AppError::UnsupportedNewerDatabase {
                    found: 999,
                    supported: 5
                }
            ));
            assert_eq!(
                crate::test_support::stable_sqlite_hash(&path).await,
                before_hash
            );

            fs::remove_file(path).expect("test database should be removable");
        });
    }

    #[test]
    fn unique_household_constraint_maps_to_already_onboarded() {
        tauri::async_runtime::block_on(async {
            let path = test_path("unique");
            let _ = fs::remove_file(&path);
            let state = AppState::initialize(path.clone()).await;
            complete_onboarding(&state, valid_input())
                .await
                .expect("first household should insert");

            let database = state.writable_db().expect("writable database");
            let error = sqlx::query(
                "INSERT INTO households (id, name, base_currency, created_at, updated_at) VALUES ('second', 'Other', 'USD', '2026-08-17T00:00:00.000Z', '2026-08-17T00:00:00.000Z')",
            )
            .execute(database)
            .await
            .expect_err("second household must violate singleton constraint");
            assert!(is_household_unique_conflict(&error));
            assert!(matches!(
                super::map_write_error(error),
                AppError::AlreadyOnboarded
            ));

            fs::remove_file(path).expect("test database should be removable");
        });
    }
}
