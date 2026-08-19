use std::{
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::{
    application::{
        analytics_query_service::{
            declare_lot_cost_basis, get_analytics_status, get_gain_summary,
            get_net_worth_attribution, get_performance_summary, list_cost_basis_declarations,
            list_holding_lots, list_unknown_basis_lots, revoke_lot_cost_basis, AnalyticsPeriodDto,
            AnalyticsScopeDto, DeclareLotCostBasisInput, GetAnalyticsStatusInput,
            GetGainSummaryInput, GetNetWorthAttributionInput, GetPerformanceSummaryInput,
            ListCostBasisDeclarationsInput, ListHoldingLotsInput, ListUnknownBasisLotsInput,
            LotRefDto, LotRefSourceKind, RevokeLotCostBasisInput,
        },
        history_query_service::{
            confirm_history_timezone, correct_activity, create_activity, get_account_timeline,
            get_activity, get_history_origin, list_activities, preview_activity, reverse_activity,
            ConfirmHistoryTimezoneInput, CorrectActivityInput, CreateActivityInput,
            GetAccountTimelineInput, ListActivitiesInput, ReverseActivityInput,
        },
        history_snapshot_service::{
            get_history_status, get_net_worth_trend, rebuild_history_snapshots,
            GetNetWorthTrendInput, RebuildHistorySnapshotsInput,
        },
        onboarding_service::{complete_onboarding, CompleteOnboardingInput, OnboardingMemberInput},
    },
    error::AppError,
    infrastructure::database::connect_writable,
    state::AppState,
};

pub const UNKNOWN_UUID: &str = "00000000-0000-7000-8000-000000000001";

pub fn test_path(phase: &str, name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "nestworth-{phase}-{name}-{}-{nonce}",
        std::process::id()
    ))
}

pub fn file_hash(path: &Path) -> u64 {
    let bytes = fs::read(path).expect("database fixture should exist");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

pub async fn stable_sqlite_hash(path: &Path) -> u64 {
    if let Ok(pool) = connect_writable(path, false).await {
        let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&pool)
            .await;
        pool.close().await;
    }
    file_hash(path)
}

pub fn valid_onboarding_input() -> CompleteOnboardingInput {
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

pub async fn initialize_state(path: PathBuf) -> AppState {
    let _ = fs::remove_file(&path);
    AppState::initialize(path).await
}

pub async fn onboarded_state(name: &str) -> (AppState, PathBuf) {
    let path = test_path("phase5", name);
    let state = initialize_state(path.clone()).await;
    complete_onboarding(&state, valid_onboarding_input())
        .await
        .expect("onboarding should succeed");
    (state, path)
}

pub async fn blocked_future_state(name: &str) -> (AppState, PathBuf, u64) {
    let path = test_path("phase5-future", name);
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
    let before_hash = stable_sqlite_hash(&path).await;
    (state, path, before_hash)
}

pub fn cleanup(path: &Path) {
    fs::remove_file(path).expect("test database should be removable");
}

fn blocked_deposit() -> CreateActivityInput {
    CreateActivityInput::Deposit {
        local_date: "2026-01-01".to_owned(),
        local_time: "00:01".to_owned(),
        ambiguous_offset: None,
        note: Some("must not be persisted".to_owned()),
        account_id: UNKNOWN_UUID.to_owned(),
        component: "account_value".to_owned(),
        amount: "1".to_owned(),
        currency: "CNY".to_owned(),
    }
}

pub async fn assert_activity_history_commands_write_nothing(
    state: &AppState,
    path: &Path,
    before_hash: u64,
) {
    let deposit = blocked_deposit();
    let errors = [
        ("get_history_origin", get_history_origin(state).await.err()),
        (
            "confirm_history_timezone",
            confirm_history_timezone(
                state,
                ConfirmHistoryTimezoneInput {
                    timezone: "UTC".to_owned(),
                },
            )
            .await
            .err(),
        ),
        (
            "preview_activity",
            preview_activity(state, deposit.clone()).await.err(),
        ),
        (
            "list_activities",
            list_activities(
                state,
                ListActivitiesInput {
                    cursor: None,
                    limit: Some(10),
                    start_local_date: None,
                    end_local_date: None,
                    account_id: None,
                    instrument_id: None,
                    kind: None,
                    classification: None,
                },
            )
            .await
            .err(),
        ),
        (
            "get_activity",
            get_activity(state, UNKNOWN_UUID).await.err(),
        ),
        (
            "create_activity",
            create_activity(state, deposit.clone()).await.err(),
        ),
        (
            "reverse_activity",
            reverse_activity(
                state,
                ReverseActivityInput {
                    id: UNKNOWN_UUID.to_owned(),
                    local_date: None,
                    local_time: None,
                    ambiguous_offset: None,
                },
            )
            .await
            .err(),
        ),
        (
            "correct_activity",
            correct_activity(
                state,
                CorrectActivityInput {
                    original_id: UNKNOWN_UUID.to_owned(),
                    replacement: deposit,
                },
            )
            .await
            .err(),
        ),
        (
            "get_account_timeline",
            get_account_timeline(
                state,
                GetAccountTimelineInput {
                    account_id: UNKNOWN_UUID.to_owned(),
                    cursor: None,
                    limit: Some(10),
                },
            )
            .await
            .err(),
        ),
        ("get_history_status", get_history_status(state).await.err()),
        (
            "rebuild_history_snapshots",
            rebuild_history_snapshots(state, RebuildHistorySnapshotsInput { cancel: false })
                .await
                .err(),
        ),
        (
            "get_net_worth_trend",
            get_net_worth_trend(
                state,
                GetNetWorthTrendInput {
                    range: "all".to_owned(),
                },
            )
            .await
            .err(),
        ),
        (
            "get_analytics_status",
            get_analytics_status(
                state,
                GetAnalyticsStatusInput {
                    scope: AnalyticsScopeDto::Household,
                },
            )
            .await
            .err(),
        ),
        (
            "get_performance_summary",
            get_performance_summary(
                state,
                GetPerformanceSummaryInput {
                    scope: AnalyticsScopeDto::Household,
                    period: AnalyticsPeriodDto::OneMonth,
                },
            )
            .await
            .err(),
        ),
        (
            "get_gain_summary",
            get_gain_summary(
                state,
                GetGainSummaryInput {
                    scope: AnalyticsScopeDto::Household,
                },
            )
            .await
            .err(),
        ),
        (
            "get_net_worth_attribution",
            get_net_worth_attribution(
                state,
                GetNetWorthAttributionInput {
                    scope: AnalyticsScopeDto::Household,
                    period: AnalyticsPeriodDto::OneMonth,
                },
            )
            .await
            .err(),
        ),
        (
            "list_holding_lots",
            list_holding_lots(
                state,
                ListHoldingLotsInput {
                    scope: AnalyticsScopeDto::Household,
                    cursor: None,
                    limit: Some(10),
                },
            )
            .await
            .err(),
        ),
        (
            "list_unknown_basis_lots",
            list_unknown_basis_lots(
                state,
                ListUnknownBasisLotsInput {
                    scope: AnalyticsScopeDto::Household,
                    cursor: None,
                    limit: Some(10),
                },
            )
            .await
            .err(),
        ),
        (
            "list_cost_basis_declarations",
            list_cost_basis_declarations(
                state,
                ListCostBasisDeclarationsInput {
                    scope: AnalyticsScopeDto::Household,
                    cursor: None,
                    limit: Some(10),
                },
            )
            .await
            .err(),
        ),
        (
            "declare_lot_cost_basis",
            declare_lot_cost_basis(
                state,
                DeclareLotCostBasisInput {
                    lot_ref: LotRefDto {
                        source_kind: LotRefSourceKind::OriginHolding,
                        source_id: UNKNOWN_UUID.to_owned(),
                    },
                    instrument_id: UNKNOWN_UUID.to_owned(),
                    declared_cost: "1".to_owned(),
                    declared_currency: "USD".to_owned(),
                    acquired_on: None,
                    note: None,
                },
            )
            .await
            .err(),
        ),
        (
            "revoke_lot_cost_basis",
            revoke_lot_cost_basis(
                state,
                RevokeLotCostBasisInput {
                    lot_ref: LotRefDto {
                        source_kind: LotRefSourceKind::OriginHolding,
                        source_id: UNKNOWN_UUID.to_owned(),
                    },
                },
            )
            .await
            .err(),
        ),
    ];
    for (command, error) in errors {
        assert!(
            matches!(error, Some(AppError::UnsupportedNewerDatabase { .. })),
            "{command} must reject an unsupported future database without writing: {error:?}"
        );
    }
    assert_eq!(stable_sqlite_hash(path).await, before_hash);
}
