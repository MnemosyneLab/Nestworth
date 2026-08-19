//! History Origin initialization.
//!
//! Origin item tables hold the immutable financial baseline captured at
//! cutover. The same transaction writes first Holding Quantity, Account-state,
//! Holding-state, and quote-preference observations with timestamps equal to
//! `origin_at` and `activity_id` NULL. Those rows are opening observations for
//! historical valuation, not Activities or trades. Current Holding Quantity
//! remains on `holdings.quantity` as the v0.1.2 projection and is never copied
//! into the Activity ledger.

use sqlx::{Row, Sqlite, Transaction};

use super::{
    history_repositories::{
        get_origin_by_household, insert_account_state_observation, insert_account_state_ownership,
        insert_fx_preference_observation, insert_holding_quantity,
        insert_holding_state_observation, insert_instrument_preference_observation, insert_origin,
        insert_origin_account_state, insert_origin_account_value, insert_origin_cash_value,
        insert_origin_holding, insert_origin_ownership, snapshot_state_exists,
        upsert_snapshot_state, AccountStateObservationRecord, AccountStateOwnershipRecord,
        FxPreferenceObservationRecord, HistoryOriginRecord, HoldingQuantityRecord,
        HoldingStateObservationRecord, InstrumentPreferenceObservationRecord,
        OriginAccountStateRecord, OriginAccountValueRecord, OriginCashValueRecord,
        OriginHoldingRecord, OriginOwnershipRecord, SnapshotStateRecord,
    },
    reference::{begin_write_tx, finish_write_tx, map_read_error},
};
use crate::{
    domain::{
        origin_timezone_from_iana_name, resolve_host_origin_timezone, AccountStateObservationId,
        HistoryOriginId, HistoryTimezone, HoldingQuantityValueId, HoldingStateObservationId,
        HouseholdId, QuotePreferenceObservationId, Timestamp,
    },
    error::AppError,
    infrastructure::database::SqlitePool,
    state::AppState,
};

pub const ORIGIN_SOURCE_MIGRATED_V012: &str = "migrated_v012";
pub const ORIGIN_SOURCE_FRESH_ONBOARDING: &str = "fresh_onboarding";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OriginTimezoneChoice {
    pub timezone: HistoryTimezone,
    pub confirmed: bool,
}

impl OriginTimezoneChoice {
    #[must_use]
    pub fn from_host() -> Self {
        let (timezone, confirmed) = resolve_host_origin_timezone();
        Self {
            timezone,
            confirmed,
        }
    }

    #[must_use]
    pub fn from_iana_name(name: Option<&str>) -> Self {
        let (timezone, confirmed) = origin_timezone_from_iana_name(name);
        Self {
            timezone,
            confirmed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OriginPresence {
    Missing,
    Complete,
    Partial,
}

pub async fn initialize_history_origin_if_needed(
    pool: &SqlitePool,
    schema_version: i64,
) -> Result<(), AppError> {
    let mut tx = begin_write_tx(pool).await?;
    let result = initialize_history_origin_in_tx(
        &mut tx,
        schema_version,
        OriginTimezoneChoice::from_host(),
        Timestamp::now(),
    )
    .await;
    finish_write_tx(tx, result).await
}

pub async fn initialize_history_origin_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    schema_version: i64,
    timezone: OriginTimezoneChoice,
    origin_at: Timestamp,
) -> Result<(), AppError> {
    let Some(household_id) = household_id(tx).await? else {
        return Ok(());
    };
    match origin_presence(tx, &household_id).await? {
        OriginPresence::Complete => Ok(()),
        OriginPresence::Partial => {
            tracing::error!(
                event = "history.origin_partial",
                "history origin is incomplete"
            );
            Err(AppError::HistoryInitializationFailed)
        }
        OriginPresence::Missing => {
            create_origin_in_tx(
                tx,
                &household_id,
                ORIGIN_SOURCE_MIGRATED_V012,
                timezone,
                &origin_at,
                schema_version,
                true,
            )
            .await
        }
    }
}

pub async fn insert_fresh_origin_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: HouseholdId,
    now: &Timestamp,
    schema_version: i64,
) -> Result<(), AppError> {
    create_origin_in_tx(
        tx,
        &household_id.to_string(),
        ORIGIN_SOURCE_FRESH_ONBOARDING,
        OriginTimezoneChoice::from_host(),
        now,
        schema_version,
        false,
    )
    .await
}

pub async fn ensure_activity_writes_allowed(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<HistoryOriginRecord, AppError> {
    let household_id = household_id(tx)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    match origin_presence(tx, &household_id).await? {
        OriginPresence::Complete => {}
        OriginPresence::Missing | OriginPresence::Partial => {
            return Err(AppError::HistoryInitializationFailed);
        }
    }
    let origin = get_origin_by_household(tx, &household_id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    if !origin.timezone_confirmed {
        return Err(AppError::HistoryTimezoneConfirmationRequired);
    }
    Ok(origin)
}

pub async fn confirm_history_timezone(state: &AppState, timezone: &str) -> Result<(), AppError> {
    let timezone = HistoryTimezone::parse(timezone)?;
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = confirm_history_timezone_in_tx(&mut tx, timezone).await;
    finish_write_tx(tx, result).await
}

async fn confirm_history_timezone_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    timezone: HistoryTimezone,
) -> Result<(), AppError> {
    let household_id = household_id(tx)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    match origin_presence(tx, &household_id).await? {
        OriginPresence::Complete => {}
        OriginPresence::Missing | OriginPresence::Partial => {
            return Err(AppError::HistoryInitializationFailed);
        }
    }
    let origin = get_origin_by_household(tx, &household_id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    if origin.timezone_confirmed {
        return Err(AppError::conflict(
            "The history timezone can no longer be changed.",
        ));
    }
    let activity_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| map_read_error("history.activity_count_failed", error))?;
    let snapshot_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM daily_valuation_snapshots")
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| map_read_error("history.snapshot_count_failed", error))?;
    if activity_count > 0 || snapshot_count > 0 {
        return Err(AppError::conflict(
            "The history timezone can no longer be changed.",
        ));
    }
    let origin_at = Timestamp::parse(&origin.origin_at)?;
    let origin_local_date = timezone.local_date(&origin_at).to_ymd();
    super::history_repositories::update_origin_timezone(
        tx,
        &origin.id,
        timezone.as_str(),
        &origin_local_date,
    )
    .await?;
    tracing::info!(
        event = "history.timezone_confirmed",
        "history timezone was confirmed"
    );
    Ok(())
}

async fn create_origin_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    source: &str,
    timezone: OriginTimezoneChoice,
    origin_at: &Timestamp,
    schema_version: i64,
    capture_current_state: bool,
) -> Result<(), AppError> {
    let origin = HistoryOriginRecord {
        id: HistoryOriginId::new().to_string(),
        household_id: household_id.to_owned(),
        timezone: timezone.timezone.as_str().to_owned(),
        timezone_confirmed: timezone.confirmed,
        origin_at: origin_at.to_rfc3339(),
        origin_local_date: timezone.timezone.local_date(origin_at).to_ymd(),
        source: source.to_owned(),
        schema_version,
        created_at: origin_at.to_rfc3339(),
    };
    insert_origin(tx, &origin).await?;
    if capture_current_state {
        capture_current_state_in_tx(tx, &origin).await?;
    }
    upsert_snapshot_state(
        tx,
        &SnapshotStateRecord {
            household_id: household_id.to_owned(),
            dirty_from: Some(origin.origin_local_date.clone()),
            last_completed_on: None,
            rebuild_status: "idle".to_owned(),
            rebuild_cursor_on: None,
            updated_at: origin.created_at.clone(),
        },
    )
    .await?;
    tracing::info!(
        event = "history.origin_initialized",
        source,
        timezone_confirmed = timezone.confirmed,
        "history origin initialized"
    );
    Ok(())
}

async fn capture_current_state_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
) -> Result<(), AppError> {
    let origin_at = &origin.origin_at;
    let account_values = sqlx::query(
        "SELECT v.account_id, v.amount, v.currency, v.value_kind
         FROM account_values v
         JOIN accounts a ON a.id = v.account_id
         JOIN (
            SELECT id,
                   ROW_NUMBER() OVER (PARTITION BY account_id ORDER BY effective_at DESC, created_at DESC, id DESC) AS rn
            FROM account_values
         ) latest ON latest.id = v.id AND latest.rn = 1
         WHERE a.household_id = ? AND a.tracking_mode IN ('balance', 'manual_value')",
    )
    .bind(&origin.household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_capture_values_failed", error))?;
    for row in account_values {
        insert_origin_account_value(
            tx,
            &OriginAccountValueRecord {
                origin_id: origin.id.clone(),
                account_id: required_text(&row, "account_id")?,
                amount: required_text(&row, "amount")?,
                currency: required_text(&row, "currency")?,
                value_kind: required_text(&row, "value_kind")?,
            },
        )
        .await?;
    }

    let cash_values = sqlx::query(
        "SELECT v.account_id, v.amount, v.currency
         FROM account_cash_values v
         JOIN accounts a ON a.id = v.account_id
         JOIN (
            SELECT id,
                   ROW_NUMBER() OVER (PARTITION BY account_id, currency ORDER BY effective_at DESC, created_at DESC, id DESC) AS rn
            FROM account_cash_values
         ) latest ON latest.id = v.id AND latest.rn = 1
         WHERE a.household_id = ?",
    )
    .bind(&origin.household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_capture_cash_failed", error))?;
    for row in cash_values {
        insert_origin_cash_value(
            tx,
            &OriginCashValueRecord {
                origin_id: origin.id.clone(),
                account_id: required_text(&row, "account_id")?,
                amount: required_text(&row, "amount")?,
                currency: required_text(&row, "currency")?,
            },
        )
        .await?;
    }

    let holdings = sqlx::query(
        "SELECT h.id, h.account_id, h.instrument_id, h.quantity, h.archived_at
         FROM holdings h
         JOIN accounts a ON a.id = h.account_id
         WHERE a.household_id = ?
         ORDER BY h.id",
    )
    .bind(&origin.household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_capture_holdings_failed", error))?;
    for row in holdings {
        let holding_id: String = required_text(&row, "id")?;
        let archived_at: Option<String> = optional_text(&row, "archived_at")?;
        let quantity: String = required_text(&row, "quantity")?;
        let active = archived_at.is_none();
        insert_origin_holding(
            tx,
            &OriginHoldingRecord {
                origin_id: origin.id.clone(),
                holding_id: holding_id.clone(),
                account_id: required_text(&row, "account_id")?,
                instrument_id: required_text(&row, "instrument_id")?,
                quantity: quantity.clone(),
                active,
            },
        )
        .await?;
        insert_holding_quantity(
            tx,
            &HoldingQuantityRecord {
                id: HoldingQuantityValueId::new().to_string(),
                holding_id: holding_id.clone(),
                quantity,
                effective_at: origin_at.clone(),
                created_at: origin_at.clone(),
                activity_id: None,
            },
        )
        .await?;
        insert_holding_state_observation(
            tx,
            &HoldingStateObservationRecord {
                id: HoldingStateObservationId::new().to_string(),
                holding_id,
                active,
                archived_at,
                effective_at: origin_at.clone(),
                created_at: origin_at.clone(),
            },
        )
        .await?;
    }

    let mut observation_ids = std::collections::HashMap::new();
    let accounts = sqlx::query(
        "SELECT id, primary_category, secondary_category, tracking_mode,
                include_in_net_worth, include_in_investment, include_in_liquid_assets,
                archived_at, institution_id, group_id
         FROM accounts WHERE household_id = ? ORDER BY id",
    )
    .bind(&origin.household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_capture_accounts_failed", error))?;
    for row in accounts {
        let account_id: String = required_text(&row, "id")?;
        let state = OriginAccountStateRecord {
            origin_id: origin.id.clone(),
            account_id: account_id.clone(),
            primary_category: required_text(&row, "primary_category")?,
            secondary_category: required_text(&row, "secondary_category")?,
            tracking_mode: required_text(&row, "tracking_mode")?,
            include_in_net_worth: required_i64(&row, "include_in_net_worth")? != 0,
            include_in_investment: required_i64(&row, "include_in_investment")? != 0,
            include_in_liquid_assets: required_i64(&row, "include_in_liquid_assets")? != 0,
            archived_at: optional_text(&row, "archived_at")?,
            institution_id: optional_text(&row, "institution_id")?,
            group_id: optional_text(&row, "group_id")?,
        };
        insert_origin_account_state(tx, &state).await?;
        let observation_id = AccountStateObservationId::new().to_string();
        insert_account_state_observation(
            tx,
            &AccountStateObservationRecord {
                id: observation_id.clone(),
                account_id: account_id.clone(),
                primary_category: state.primary_category,
                secondary_category: state.secondary_category,
                tracking_mode: state.tracking_mode,
                include_in_net_worth: state.include_in_net_worth,
                include_in_investment: state.include_in_investment,
                include_in_liquid_assets: state.include_in_liquid_assets,
                archived_at: state.archived_at,
                institution_id: state.institution_id,
                group_id: state.group_id,
                effective_at: origin_at.clone(),
                created_at: origin_at.clone(),
            },
        )
        .await?;
        observation_ids.insert(account_id, observation_id);
    }

    let owners = sqlx::query(
        "SELECT o.account_id, o.member_id, o.share_bps
         FROM account_ownership o
         JOIN accounts a ON a.id = o.account_id
         WHERE a.household_id = ?
         ORDER BY o.account_id, o.member_id",
    )
    .bind(&origin.household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_capture_ownership_failed", error))?;
    for owner in owners {
        let account_id: String = required_text(&owner, "account_id")?;
        let member_id: String = required_text(&owner, "member_id")?;
        let share_bps = required_i64(&owner, "share_bps")?;
        insert_origin_ownership(
            tx,
            &OriginOwnershipRecord {
                origin_id: origin.id.clone(),
                account_id: account_id.clone(),
                member_id: member_id.clone(),
                share_bps,
            },
        )
        .await?;
        let Some(observation_id) = observation_ids.get(&account_id) else {
            tracing::error!(
                event = "history.origin_ownership_missing_state",
                "origin ownership has no matching account state"
            );
            return Err(AppError::HistoryInitializationFailed);
        };
        insert_account_state_ownership(
            tx,
            &AccountStateOwnershipRecord {
                observation_id: observation_id.clone(),
                member_id,
                share_bps,
            },
        )
        .await?;
    }

    let instruments = sqlx::query(
        "SELECT id, quote_preference FROM instruments WHERE household_id = ? ORDER BY id",
    )
    .bind(&origin.household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_capture_instruments_failed", error))?;
    for row in instruments {
        insert_instrument_preference_observation(
            tx,
            &InstrumentPreferenceObservationRecord {
                id: QuotePreferenceObservationId::new().to_string(),
                instrument_id: required_text(&row, "id")?,
                quote_preference: required_text(&row, "quote_preference")?,
                effective_at: origin_at.clone(),
                created_at: origin_at.clone(),
            },
        )
        .await?;
    }

    let fx_preferences = sqlx::query(
        "SELECT currency_a, currency_b, source_kind
         FROM fx_quote_preferences WHERE household_id = ? ORDER BY currency_a, currency_b",
    )
    .bind(&origin.household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_capture_fx_failed", error))?;
    for row in fx_preferences {
        insert_fx_preference_observation(
            tx,
            &FxPreferenceObservationRecord {
                id: QuotePreferenceObservationId::new().to_string(),
                household_id: origin.household_id.clone(),
                currency_a: required_text(&row, "currency_a")?,
                currency_b: required_text(&row, "currency_b")?,
                source_kind: required_text(&row, "source_kind")?,
                effective_at: origin_at.clone(),
                created_at: origin_at.clone(),
            },
        )
        .await?;
    }
    Ok(())
}

async fn origin_presence(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<OriginPresence, AppError> {
    let origin = get_origin_by_household(tx, household_id).await?;
    let snapshot_state = snapshot_state_exists(tx, household_id).await?;
    match (origin.is_some(), snapshot_state) {
        (false, false) => Ok(OriginPresence::Missing),
        (true, true) => Ok(OriginPresence::Complete),
        (true, false) | (false, true) => Ok(OriginPresence::Partial),
    }
}

async fn household_id(tx: &mut Transaction<'_, Sqlite>) -> Result<Option<String>, AppError> {
    sqlx::query_scalar("SELECT id FROM households ORDER BY created_at, id LIMIT 1")
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| map_read_error("history.household_load_failed", error))
}

fn required_text(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<String, AppError> {
    row.try_get(column)
        .map_err(|_| AppError::DatabaseUnavailable)
}

fn optional_text(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<String>, AppError> {
    row.try_get(column)
        .map_err(|_| AppError::DatabaseUnavailable)
}

fn required_i64(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<i64, AppError> {
    row.try_get(column)
        .map_err(|_| AppError::DatabaseUnavailable)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::{
        confirm_history_timezone, ensure_activity_writes_allowed, initialize_history_origin_in_tx,
        OriginTimezoneChoice, ORIGIN_SOURCE_FRESH_ONBOARDING,
    };
    use crate::{
        application::{
            history_repositories::{
                insert_activity, insert_origin, insert_origin_account_value, list_activities_desc,
                ActivityListCursor, HistoryOriginRecord, OriginAccountValueRecord,
            },
            onboarding_service::complete_onboarding,
            reference::{begin_write_tx, finish_write_tx},
        },
        domain::{
            Activity, ActivityId, ActivityKind, ActivityLeg, CalendarDate, CurrencyCode, Direction,
            HouseholdId, LegComponent, LegRole, Money, Timestamp,
        },
        error::{AppError, ErrorCode},
        infrastructure::{
            database::connect_writable,
            database_bootstrap::{initialize_database, DatabaseBootstrapStatus, MIGRATOR},
        },
        state::AppState,
        test_support::{cleanup, test_path, valid_onboarding_input},
    };

    async fn empty_state(name: &str) -> (AppState, std::path::PathBuf) {
        let path = test_path("phase2-origin", name);
        let _ = fs::remove_file(&path);
        let state = AppState::initialize(path.clone()).await;
        (state, path)
    }

    async fn load_v012_migrated(name: &str) -> (AppState, std::path::PathBuf) {
        let path = test_path("phase2-v012", name);
        let _ = fs::remove_file(&path);
        let pool = connect_writable(&path, true)
            .await
            .expect("v0.1.2 fixture should open");
        for version in [1_i64, 2] {
            let migration = MIGRATOR
                .iter()
                .find(|item| item.version == version)
                .expect("migration 001 and 002 should exist")
                .clone();
            let mut conn = pool.acquire().await.expect("connection");
            sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
                .await
                .expect("migration metadata table should be created");
            sqlx::migrate::Migrate::apply(&mut *conn, &migration)
                .await
                .expect("released schema should apply");
        }
        sqlx::raw_sql(include_str!("../../test-fixtures/v0.1.2.sql"))
            .execute(&pool)
            .await
            .expect("released fixture should load");
        pool.close().await;
        let state = AppState::initialize(path.clone()).await;
        (state, path)
    }

    #[test]
    fn empty_database_defers_origin_until_onboarding() {
        tauri::async_runtime::block_on(async {
            let (state, path) = empty_state("defer").await;
            let database = state.writable_db().expect("writable");
            let origin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history_origins")
                .fetch_one(database)
                .await
                .expect("origin count");
            assert_eq!(origin_count, 0);

            complete_onboarding(&state, valid_onboarding_input())
                .await
                .expect("onboarding should succeed");
            let origin: (String, i64) =
                sqlx::query_as("SELECT source, timezone_confirmed FROM history_origins")
                    .fetch_one(database)
                    .await
                    .expect("origin after onboarding");
            assert_eq!(origin.0, ORIGIN_SOURCE_FRESH_ONBOARDING);
            let expected = OriginTimezoneChoice::from_host();
            let timezone: String = sqlx::query_scalar("SELECT timezone FROM history_origins")
                .fetch_one(database)
                .await
                .expect("timezone");
            assert_eq!(timezone, expected.timezone.as_str());
            assert_eq!(origin.1, i64::from(expected.confirmed));
            cleanup(&path);
        });
    }

    #[test]
    fn resolved_host_timezone_is_stored_at_origin_creation() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v012_migrated("host-tz").await;
            let database = state.writable_db().expect("writable");
            let expected = OriginTimezoneChoice::from_host();
            let row: (String, i64) =
                sqlx::query_as("SELECT timezone, timezone_confirmed FROM history_origins")
                    .fetch_one(database)
                    .await
                    .expect("origin timezone");
            assert_eq!(row.0, expected.timezone.as_str());
            assert_eq!(row.1, i64::from(expected.confirmed));
            cleanup(&path);
        });
    }

    #[test]
    fn unconfirmed_utc_blocks_activity_writes_until_confirmed() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v012_migrated("unconfirmed").await;
            let database = state.writable_db().expect("writable");
            sqlx::query(
                "UPDATE history_origins
                 SET timezone = 'UTC', timezone_confirmed = 0, origin_at = '2026-01-02T04:30:00.000Z', origin_local_date = '2026-01-02'",
            )
            .execute(database)
            .await
            .expect("unconfirmed utc should persist");

            let mut tx = begin_write_tx(database).await.expect("tx");
            let error = ensure_activity_writes_allowed(&mut tx)
                .await
                .expect_err("unconfirmed timezone must block writes");
            let _ = tx.rollback().await;
            assert!(matches!(
                error,
                AppError::HistoryTimezoneConfirmationRequired
            ));
            assert_eq!(
                error.into_command_error().code,
                ErrorCode::HistoryTimezoneConfirmationRequired
            );

            confirm_history_timezone(&state, "America/New_York")
                .await
                .expect("confirmation should succeed");
            let row: (String, i64, String) = sqlx::query_as(
                "SELECT timezone, timezone_confirmed, origin_local_date FROM history_origins",
            )
            .fetch_one(database)
            .await
            .expect("confirmed origin");
            assert_eq!(row.0, "America/New_York");
            assert_eq!(row.1, 1);
            assert_eq!(row.2, "2026-01-01");

            confirm_history_timezone(&state, "UTC")
                .await
                .expect_err("timezone is immutable after confirmation");
            cleanup(&path);
        });
    }

    #[test]
    fn unconfirmed_host_fallback_writes_utc() {
        tauri::async_runtime::block_on(async {
            let (state, path) = empty_state("fallback-write").await;
            complete_onboarding(&state, valid_onboarding_input())
                .await
                .expect("onboarding");
            let database = state.writable_db().expect("writable");
            sqlx::query("DELETE FROM history_snapshot_state")
                .execute(database)
                .await
                .expect("clear snapshot state");
            sqlx::query("DELETE FROM history_origins")
                .execute(database)
                .await
                .expect("clear origin");

            let mut tx = begin_write_tx(database).await.expect("tx");
            initialize_history_origin_in_tx(
                &mut tx,
                3,
                OriginTimezoneChoice::from_iana_name(None),
                Timestamp::parse("2026-01-02T04:30:00.000Z").expect("origin_at"),
            )
            .await
            .expect("origin with unconfirmed utc");
            finish_write_tx(tx, Ok(())).await.expect("commit");

            let row: (String, i64, String) = sqlx::query_as(
                "SELECT timezone, timezone_confirmed, origin_local_date FROM history_origins",
            )
            .fetch_one(database)
            .await
            .expect("fallback origin");
            assert_eq!(row.0, "UTC");
            assert_eq!(row.1, 0);
            assert_eq!(row.2, "2026-01-02");
            cleanup(&path);
        });
    }

    #[test]
    fn timezone_confirmation_is_blocked_after_first_activity() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v012_migrated("confirm-after-activity").await;
            let database = state.writable_db().expect("writable");
            sqlx::query("UPDATE history_origins SET timezone_confirmed = 0, timezone = 'UTC'")
                .execute(database)
                .await
                .expect("unconfirm");

            let mut tx = begin_write_tx(database).await.expect("tx");
            insert_activity(
                &mut tx,
                &sample_deposit(
                    "01900000-0000-7000-8000-000000000001",
                    "2026-06-01T12:00:01.000Z",
                ),
            )
            .await
            .expect("activity insert");
            finish_write_tx(tx, Ok(())).await.expect("commit");

            let error = confirm_history_timezone(&state, "UTC")
                .await
                .expect_err("activity makes timezone immutable");
            assert!(matches!(error, AppError::Conflict { .. }));
            cleanup(&path);
        });
    }

    #[test]
    fn partial_origin_insert_rolls_back_and_blocks_activity_writes() {
        tauri::async_runtime::block_on(async {
            let (state, path) = empty_state("rollback").await;
            complete_onboarding(&state, valid_onboarding_input())
                .await
                .expect("onboarding");
            let database = state.writable_db().expect("writable");
            sqlx::query("DELETE FROM history_snapshot_state")
                .execute(database)
                .await
                .expect("clear snapshot state");
            sqlx::query("DELETE FROM history_origins")
                .execute(database)
                .await
                .expect("clear origin");

            let mut tx = begin_write_tx(database).await.expect("tx");
            let household_id: String = sqlx::query_scalar("SELECT id FROM households")
                .fetch_one(&mut *tx)
                .await
                .expect("household");
            let origin_id = crate::domain::HistoryOriginId::new().to_string();
            insert_origin(
                &mut tx,
                &HistoryOriginRecord {
                    id: origin_id.clone(),
                    household_id,
                    timezone: "UTC".to_owned(),
                    timezone_confirmed: true,
                    origin_at: "2026-01-01T00:00:00.000Z".to_owned(),
                    origin_local_date: "2026-01-01".to_owned(),
                    source: "migrated_v012".to_owned(),
                    schema_version: 3,
                    created_at: "2026-01-01T00:00:00.000Z".to_owned(),
                },
            )
            .await
            .expect("origin header");
            let error = insert_origin_account_value(
                &mut tx,
                &OriginAccountValueRecord {
                    origin_id,
                    account_id: "99999999-9999-4999-8999-999999999999".to_owned(),
                    amount: "1".to_owned(),
                    currency: "CNY".to_owned(),
                    value_kind: "balance".to_owned(),
                },
            )
            .await
            .expect_err("missing account must fail");
            assert!(matches!(error, AppError::DatabaseUnavailable));
            finish_write_tx::<()>(tx, Err(error))
                .await
                .expect_err("rollback");

            let origin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history_origins")
                .fetch_one(database)
                .await
                .expect("origin count");
            assert_eq!(origin_count, 0);

            let mut tx = begin_write_tx(database).await.expect("tx");
            let blocked = ensure_activity_writes_allowed(&mut tx)
                .await
                .expect_err("missing origin blocks activity");
            let _ = tx.rollback().await;
            assert!(matches!(blocked, AppError::HistoryInitializationFailed));
            assert_eq!(
                blocked.into_command_error().code,
                ErrorCode::HistoryInitializationFailed
            );
            cleanup(&path);
        });
    }

    #[test]
    fn partial_origin_blocks_bootstrap() {
        tauri::async_runtime::block_on(async {
            let (state, path) = empty_state("partial-bootstrap").await;
            complete_onboarding(&state, valid_onboarding_input())
                .await
                .expect("onboarding");
            let database = state.writable_db().expect("writable");
            sqlx::query("DELETE FROM history_snapshot_state")
                .execute(database)
                .await
                .expect("leave origin header only");
            drop(state);

            let reopened = initialize_database(path.clone()).await;
            assert_eq!(
                reopened.status,
                DatabaseBootstrapStatus::HistoryInitializationFailed
            );
            assert!(reopened.pool.is_none());
            cleanup(&path);
        });
    }

    #[test]
    fn activity_cursor_orders_equal_effective_at_by_created_at_then_id() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v012_migrated("cursor").await;
            let database = state.writable_db().expect("writable");
            let mut tx = begin_write_tx(database).await.expect("tx");
            insert_activity(
                &mut tx,
                &sample_deposit(
                    "01900000-0000-7000-8000-000000000001",
                    "2026-06-01T12:00:01.000Z",
                ),
            )
            .await
            .expect("first");
            insert_activity(
                &mut tx,
                &sample_deposit(
                    "01900000-0000-7000-8000-000000000002",
                    "2026-06-01T12:00:01.000Z",
                ),
            )
            .await
            .expect("second");
            let listed =
                list_activities_desc(&mut tx, "11111111-1111-4111-8111-111111111111", None, 10)
                    .await
                    .expect("list");
            assert_eq!(listed.len(), 2);
            assert_eq!(
                listed[0].id().to_string(),
                "01900000-0000-7000-8000-000000000002"
            );
            assert_eq!(
                listed[1].id().to_string(),
                "01900000-0000-7000-8000-000000000001"
            );
            let paged = list_activities_desc(
                &mut tx,
                "11111111-1111-4111-8111-111111111111",
                Some(&ActivityListCursor {
                    effective_at: listed[0].effective_at().to_rfc3339(),
                    created_at: listed[0].created_at().to_rfc3339(),
                    id: listed[0].id().to_string(),
                }),
                10,
            )
            .await
            .expect("page");
            assert_eq!(paged.len(), 1);
            assert_eq!(
                paged[0].id().to_string(),
                "01900000-0000-7000-8000-000000000001"
            );
            finish_write_tx(tx, Ok(())).await.expect("commit");
            cleanup(&path);
        });
    }

    fn sample_deposit(id: &str, created_at: &str) -> Activity {
        let activity_id = ActivityId::from_uuid(Uuid::parse_str(id).expect("id"));
        let account_id = crate::domain::AccountId::parse("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")
            .expect("account");
        let leg = ActivityLeg::from_persisted(
            crate::domain::ActivityLegId::new(),
            activity_id,
            account_id,
            LegRole::Destination,
            Direction::Increase,
            LegComponent::AccountValue {
                amount: Money::parse("1", CurrencyCode::CNY).expect("money"),
            },
            None,
            0,
        )
        .expect("leg");
        Activity::from_persisted(
            activity_id,
            HouseholdId::parse("11111111-1111-4111-8111-111111111111").expect("household"),
            ActivityKind::Deposit,
            Timestamp::parse("2026-06-01T12:00:00.000Z").expect("effective"),
            CalendarDate::parse("2026-06-01").expect("date"),
            Timestamp::parse(created_at).expect("created"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![leg],
        )
        .expect("activity")
    }

    #[test]
    fn missing_origin_blocks_activity_commands() {
        tauri::async_runtime::block_on(async {
            let (state, path) = empty_state("missing-origin").await;
            complete_onboarding(&state, valid_onboarding_input())
                .await
                .expect("onboarding");
            let database = state.writable_db().expect("writable");
            sqlx::query("DELETE FROM history_snapshot_state")
                .execute(database)
                .await
                .expect("clear state");
            sqlx::query("DELETE FROM history_origins")
                .execute(database)
                .await
                .expect("clear origin");
            let mut tx = begin_write_tx(database).await.expect("tx");
            let error = ensure_activity_writes_allowed(&mut tx)
                .await
                .expect_err("missing origin");
            let _ = tx.rollback().await;
            assert!(matches!(error, AppError::HistoryInitializationFailed));
            cleanup(&path);
        });
    }

    #[test]
    fn confirm_utc_is_accepted_before_activity() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v012_migrated("confirm-utc").await;
            let database = state.writable_db().expect("writable");
            sqlx::query("UPDATE history_origins SET timezone_confirmed = 0")
                .execute(database)
                .await
                .expect("unconfirm");
            confirm_history_timezone(&state, "UTC")
                .await
                .expect("utc is a valid confirmation");
            let confirmed: i64 =
                sqlx::query_scalar("SELECT timezone_confirmed FROM history_origins")
                    .fetch_one(database)
                    .await
                    .expect("confirmed");
            assert_eq!(confirmed, 1);
            cleanup(&path);
        });
    }
}
