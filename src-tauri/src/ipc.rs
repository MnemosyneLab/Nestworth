use tauri::State;
use tauri_specta::{collect_commands, Builder};

use crate::{
    application::{
        account_service::{
            AccountRecordDto, CreateAccountInput, UpdateAccountInput, UpdateAccountValueInput,
        },
        analytics_query_service::{
            AnalyticsStatusDto, CostBasisDeclarationIpcDto, CostBasisDeclarationPageDto,
            DeclareLotCostBasisInput, GainSummaryIpcDto, GetAnalyticsStatusInput,
            GetGainSummaryInput, GetNetWorthAttributionInput, GetPerformanceSummaryInput,
            HoldingGainSummaryListDto, HoldingLotPageDto, ListCostBasisDeclarationsInput,
            ListHoldingGainSummariesInput, ListHoldingLotsInput, ListUnknownBasisLotsInput,
            NetWorthAttributionIpcDto, RevokeLotCostBasisInput,
        },
        backup_service::{
            BackupInspectionDto, BackupManifestDto, CreateBackupInput, InspectBackupInput,
        },
        benchmark_service::{
            AppendBenchmarkObservationInput, BenchmarkComparisonDto, BenchmarkDto,
            BenchmarkObservationDto, CreateBenchmarkInput, GetBenchmarkComparisonInput,
            ListBenchmarkObservationsInput, SetDefaultBenchmarkInput, UpdateBenchmarkInput,
        },
        cash_service::{AccountCashRecordDto, AppendAccountCashInput, ListAccountCashInput},
        csv_import_service::{
            CommitCsvImportInput, CsvImportCommitDto, GetImportBatchInput, ImportBatchDetailDto,
            ImportBatchPageDto, ListImportBatchesInput,
        },
        csv_preview_service::{CsvImportPreviewDto, PreviewCsvImportInput},
        export_service::{
            CanonicalExportDto, CsvExportDto, ExportCanonicalJsonInput, ExportCsvInput,
        },
        group_service::{CreateGroupInput, GroupRecordDto, UpdateGroupInput},
        history_query_service::{
            AccountTimelinePageDto, ActivityDetailDto, ActivityPageDto, ActivityPreviewDto,
            ConfirmHistoryTimezoneInput, CorrectActivityInput, CreateActivityInput,
            GetAccountTimelineInput, HistoryOriginDto, ListActivitiesInput, PostedCorrectionDto,
            ReverseActivityInput,
        },
        history_snapshot_service::{
            GetNetWorthTrendInput, HistoryStatusDto, NetWorthTrendDto,
            RebuildHistorySnapshotsInput, RebuildHistorySnapshotsResultDto,
        },
        holding_service::{
            CreateHoldingInput, HoldingRecordDto, ListHoldingsInput, UpdateHoldingInput,
        },
        institution_service::{
            CreateInstitutionInput, InstitutionRecordDto, UpdateInstitutionInput,
        },
        instrument_service::{CreateInstrumentInput, InstrumentRecordDto, UpdateInstrumentInput},
        maintenance_service::{
            FreshnessPolicyDto, MaintenancePageDto, MaintenanceSnoozeDto,
            SnoozeMaintenanceItemInput, UpdateFreshnessPolicyInput,
        },
        market_data::MarketDataCapabilitiesDto,
        market_data_history_service::{BackfillHistoryRangeInput, BackfillInstrumentHistoryInput},
        media_service::{GetMediaInput, MediaAssetDto, SetMediaInput},
        member_service::{CreateMemberInput, MemberRecordDto, UpdateMemberInput},
        onboarding_service::CompleteOnboardingInput,
        overview_service::OverviewDto,
        pending_service::{
            CreatePendingActivityInput, CreateRecurringActivityRuleInput,
            GenerateDuePendingActivitiesResultDto, ListPendingActivitiesInput, PendingActivityDto,
            PendingActivityPageDto, PendingActivityPostDto, PendingActivityPreviewDto,
            PendingActivityTimeInput, RecurringActivityRuleDto, UpdatePendingActivityInput,
            UpdateRecurringActivityRuleInput,
        },
        portfolio_service::PortfolioDto,
        quote_service::{
            AppendManualFxQuoteInput, AppendManualInstrumentQuoteInput, FxPairStatusDto,
            FxQuoteRecordDto, InstrumentQuoteRecordDto, ListFxQuotesInput,
            ListInstrumentQuotesInput, SetFxQuotePreferenceInput,
            SetInstrumentQuotePreferenceInput,
        },
        reference::{IdInput, ListFilterInput},
        refresh_service::{
            ProviderInstrumentDto, RefreshInstrumentInput, RefreshResultDto,
            SearchProviderInstrumentsInput,
        },
        restore_service::{
            InspectRecoveryBackupInput, RecoveryBackupListDto, RestoreBackupInput,
            RestoreBackupResultDto,
        },
        return_service::PerformanceSummaryDto,
        search_service::{GlobalSearchInput, GlobalSearchResultDto},
        settings_service::{AppSettingsDto, DeleteAllDataInput, UpdateSettingsInput},
    },
    commands::{
        accounts::{
            archive_account_impl, create_account_impl, get_account_impl, list_accounts_impl,
            restore_account_impl, update_account_impl, update_account_value_impl,
        },
        analytics::{
            declare_lot_cost_basis_impl, get_analytics_status_impl, get_gain_summary_impl,
            get_net_worth_attribution_impl, get_performance_summary_impl,
            list_cost_basis_declarations_impl, list_holding_gain_summaries_impl,
            list_holding_lots_impl, list_unknown_basis_lots_impl, revoke_lot_cost_basis_impl,
        },
        backup::{
            create_backup_impl, inspect_backup_impl, inspect_recovery_backup_impl,
            list_recovery_backups_impl, restore_backup_impl,
        },
        benchmarks::{
            append_benchmark_observation_impl, archive_benchmark_impl, create_benchmark_impl,
            get_benchmark_comparison_impl, list_benchmark_observations_impl, list_benchmarks_impl,
            restore_benchmark_impl, set_default_benchmark_impl, update_benchmark_impl,
        },
        bootstrap::{bootstrap_impl, BootstrapDto},
        cash::{append_account_cash_impl, list_account_cash_impl},
        export::{
            commit_csv_import_impl, export_canonical_json_impl, export_csv_impl,
            get_import_batch_impl, list_import_batches_impl, preview_csv_import_impl,
        },
        groups::{
            archive_group_impl, create_group_impl, list_groups_impl, restore_group_impl,
            update_group_impl,
        },
        history::{
            confirm_history_timezone_impl, correct_activity_impl, create_activity_impl,
            get_account_timeline_impl, get_activity_impl, get_history_origin_impl,
            get_history_status_impl, get_net_worth_trend_impl, list_activities_impl,
            preview_activity_impl, rebuild_history_snapshots_impl, reverse_activity_impl,
        },
        holdings::{
            archive_holding_impl, create_holding_impl, list_holdings_impl, restore_holding_impl,
            update_holding_impl,
        },
        institutions::{
            archive_institution_impl, create_institution_impl, list_institutions_impl,
            restore_institution_impl, update_institution_impl,
        },
        instruments::{
            archive_instrument_impl, create_instrument_impl, get_instrument_impl,
            list_instruments_impl, restore_instrument_impl, update_instrument_impl,
        },
        maintenance::{
            list_freshness_policies_impl, list_maintenance_items_impl,
            snooze_maintenance_item_impl, update_freshness_policy_impl,
        },
        media::{
            get_media_impl, set_account_logo_impl, set_group_logo_impl, set_institution_logo_impl,
            set_instrument_logo_impl, set_member_avatar_impl,
        },
        members::{
            archive_member_impl, create_member_impl, list_members_impl, restore_member_impl,
            update_member_impl,
        },
        onboarding::complete_onboarding_impl,
        overview::get_overview_impl,
        pending::{
            archive_recurring_activity_rule_impl, create_pending_activity_impl,
            create_recurring_activity_rule_impl, generate_due_pending_activities_impl,
            list_pending_activities_impl, list_recurring_activity_rules_impl,
            post_pending_activity_impl, preview_pending_activity_impl,
            restore_recurring_activity_rule_impl, skip_pending_activity_impl,
            update_pending_activity_impl, update_recurring_activity_rule_impl,
        },
        portfolio::get_portfolio_impl,
        quotes::{
            append_manual_fx_quote_impl, append_manual_instrument_quote_impl, list_fx_quotes_impl,
            list_instrument_quotes_impl, list_required_fx_impl, set_fx_quote_preference_impl,
            set_instrument_quote_preference_impl,
        },
        refresh::{
            backfill_all_history_impl, backfill_instrument_history_impl,
            backfill_required_fx_history_impl, get_market_data_capabilities_impl, refresh_all_impl,
            refresh_instrument_impl, refresh_required_fx_impl, search_provider_instruments_impl,
        },
        search::global_search_impl,
        settings::{delete_all_data_impl, get_settings_impl, update_settings_impl},
    },
    state::AppState,
};

macro_rules! with_shared_operation {
    ($state:expr, $future:expr) => {{
        let _operation = ($state)
            .acquire_shared_operation()
            .await
            .map_err(crate::error::CommandError::from)?;
        $future.await
    }};
}

#[tauri::command]
#[specta::specta]
pub async fn bootstrap(
    state: State<'_, AppState>,
) -> Result<BootstrapDto, crate::error::CommandError> {
    with_shared_operation!(&state, bootstrap_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn complete_onboarding(
    state: State<'_, AppState>,
    input: CompleteOnboardingInput,
) -> Result<(), crate::error::CommandError> {
    with_shared_operation!(&state, complete_onboarding_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_members(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<MemberRecordDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_members_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn create_member(
    state: State<'_, AppState>,
    input: CreateMemberInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, create_member_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn update_member(
    state: State<'_, AppState>,
    input: UpdateMemberInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, update_member_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn archive_member(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, archive_member_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn restore_member(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, restore_member_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_institutions(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<InstitutionRecordDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_institutions_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn create_institution(
    state: State<'_, AppState>,
    input: CreateInstitutionInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, create_institution_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn update_institution(
    state: State<'_, AppState>,
    input: UpdateInstitutionInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, update_institution_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn archive_institution(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, archive_institution_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn restore_institution(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, restore_institution_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_groups(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<GroupRecordDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_groups_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn create_group(
    state: State<'_, AppState>,
    input: CreateGroupInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, create_group_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn update_group(
    state: State<'_, AppState>,
    input: UpdateGroupInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, update_group_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn archive_group(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, archive_group_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn restore_group(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, restore_group_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_accounts(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<AccountRecordDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_accounts_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_account(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_account_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn create_account(
    state: State<'_, AppState>,
    input: CreateAccountInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, create_account_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn update_account(
    state: State<'_, AppState>,
    input: UpdateAccountInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, update_account_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn update_account_value(
    state: State<'_, AppState>,
    input: UpdateAccountValueInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, update_account_value_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn archive_account(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, archive_account_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn restore_account(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, restore_account_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_overview(
    state: State<'_, AppState>,
) -> Result<OverviewDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_overview_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn set_member_avatar(
    state: State<'_, AppState>,
    input: SetMediaInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, set_member_avatar_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn set_institution_logo(
    state: State<'_, AppState>,
    input: SetMediaInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, set_institution_logo_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn set_group_logo(
    state: State<'_, AppState>,
    input: SetMediaInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, set_group_logo_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn set_account_logo(
    state: State<'_, AppState>,
    input: SetMediaInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, set_account_logo_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn set_instrument_logo(
    state: State<'_, AppState>,
    input: SetMediaInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, set_instrument_logo_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_instruments(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<InstrumentRecordDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_instruments_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_instrument(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_instrument_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn create_instrument(
    state: State<'_, AppState>,
    input: CreateInstrumentInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, create_instrument_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn update_instrument(
    state: State<'_, AppState>,
    input: UpdateInstrumentInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, update_instrument_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn archive_instrument(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, archive_instrument_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn restore_instrument(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, restore_instrument_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_holdings(
    state: State<'_, AppState>,
    input: ListHoldingsInput,
) -> Result<Vec<HoldingRecordDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_holdings_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn create_holding(
    state: State<'_, AppState>,
    input: CreateHoldingInput,
) -> Result<HoldingRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, create_holding_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn update_holding(
    state: State<'_, AppState>,
    input: UpdateHoldingInput,
) -> Result<HoldingRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, update_holding_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn archive_holding(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<HoldingRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, archive_holding_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn restore_holding(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<HoldingRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, restore_holding_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_account_cash(
    state: State<'_, AppState>,
    input: ListAccountCashInput,
) -> Result<Vec<AccountCashRecordDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_account_cash_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn append_account_cash(
    state: State<'_, AppState>,
    input: AppendAccountCashInput,
) -> Result<AccountCashRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, append_account_cash_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_instrument_quotes(
    state: State<'_, AppState>,
    input: ListInstrumentQuotesInput,
) -> Result<Vec<InstrumentQuoteRecordDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_instrument_quotes_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn append_manual_instrument_quote(
    state: State<'_, AppState>,
    input: AppendManualInstrumentQuoteInput,
) -> Result<InstrumentQuoteRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, append_manual_instrument_quote_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn set_instrument_quote_preference(
    state: State<'_, AppState>,
    input: SetInstrumentQuotePreferenceInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, set_instrument_quote_preference_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_required_fx(
    state: State<'_, AppState>,
) -> Result<Vec<FxPairStatusDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_required_fx_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn list_fx_quotes(
    state: State<'_, AppState>,
    input: ListFxQuotesInput,
) -> Result<Vec<FxQuoteRecordDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_fx_quotes_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn append_manual_fx_quote(
    state: State<'_, AppState>,
    input: AppendManualFxQuoteInput,
) -> Result<FxQuoteRecordDto, crate::error::CommandError> {
    with_shared_operation!(&state, append_manual_fx_quote_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn set_fx_quote_preference(
    state: State<'_, AppState>,
    input: SetFxQuotePreferenceInput,
) -> Result<FxPairStatusDto, crate::error::CommandError> {
    with_shared_operation!(&state, set_fx_quote_preference_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_portfolio(
    state: State<'_, AppState>,
) -> Result<PortfolioDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_portfolio_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn search_provider_instruments(
    state: State<'_, AppState>,
    input: SearchProviderInstrumentsInput,
) -> Result<Vec<ProviderInstrumentDto>, crate::error::CommandError> {
    with_shared_operation!(&state, search_provider_instruments_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_market_data_capabilities(
    state: State<'_, AppState>,
) -> Result<MarketDataCapabilitiesDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_market_data_capabilities_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn refresh_instrument(
    state: State<'_, AppState>,
    input: RefreshInstrumentInput,
) -> Result<RefreshResultDto, crate::error::CommandError> {
    with_shared_operation!(&state, refresh_instrument_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn refresh_required_fx(
    state: State<'_, AppState>,
) -> Result<RefreshResultDto, crate::error::CommandError> {
    with_shared_operation!(&state, refresh_required_fx_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn refresh_all(
    state: State<'_, AppState>,
) -> Result<RefreshResultDto, crate::error::CommandError> {
    with_shared_operation!(&state, refresh_all_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn backfill_instrument_history(
    state: State<'_, AppState>,
    input: BackfillInstrumentHistoryInput,
) -> Result<RefreshResultDto, crate::error::CommandError> {
    with_shared_operation!(&state, backfill_instrument_history_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn backfill_required_fx_history(
    state: State<'_, AppState>,
    input: BackfillHistoryRangeInput,
) -> Result<RefreshResultDto, crate::error::CommandError> {
    with_shared_operation!(&state, backfill_required_fx_history_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn backfill_all_history(
    state: State<'_, AppState>,
    input: BackfillHistoryRangeInput,
) -> Result<RefreshResultDto, crate::error::CommandError> {
    with_shared_operation!(&state, backfill_all_history_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_media(
    state: State<'_, AppState>,
    input: GetMediaInput,
) -> Result<MediaAssetDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_media_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_settings(
    state: State<'_, AppState>,
) -> Result<AppSettingsDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_settings_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn update_settings(
    state: State<'_, AppState>,
    input: UpdateSettingsInput,
) -> Result<AppSettingsDto, crate::error::CommandError> {
    with_shared_operation!(&state, update_settings_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn delete_all_data(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: DeleteAllDataInput,
) -> Result<(), crate::error::CommandError> {
    delete_all_data_impl(&state, input).await?;
    app.restart()
}

#[tauri::command]
#[specta::specta]
pub async fn get_history_origin(
    state: State<'_, AppState>,
) -> Result<HistoryOriginDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_history_origin_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn confirm_history_timezone(
    state: State<'_, AppState>,
    input: ConfirmHistoryTimezoneInput,
) -> Result<HistoryOriginDto, crate::error::CommandError> {
    with_shared_operation!(&state, confirm_history_timezone_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn preview_activity(
    state: State<'_, AppState>,
    input: CreateActivityInput,
) -> Result<ActivityPreviewDto, crate::error::CommandError> {
    with_shared_operation!(&state, preview_activity_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_activities(
    state: State<'_, AppState>,
    input: ListActivitiesInput,
) -> Result<ActivityPageDto, crate::error::CommandError> {
    with_shared_operation!(&state, list_activities_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_activity(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<ActivityDetailDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_activity_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn create_activity(
    state: State<'_, AppState>,
    input: CreateActivityInput,
) -> Result<ActivityDetailDto, crate::error::CommandError> {
    with_shared_operation!(&state, create_activity_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn create_pending_activity(
    state: State<'_, AppState>,
    input: CreatePendingActivityInput,
) -> Result<PendingActivityDto, crate::error::CommandError> {
    with_shared_operation!(&state, create_pending_activity_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn update_pending_activity(
    state: State<'_, AppState>,
    input: UpdatePendingActivityInput,
) -> Result<PendingActivityDto, crate::error::CommandError> {
    with_shared_operation!(&state, update_pending_activity_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_pending_activities(
    state: State<'_, AppState>,
    input: ListPendingActivitiesInput,
) -> Result<PendingActivityPageDto, crate::error::CommandError> {
    with_shared_operation!(&state, list_pending_activities_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn preview_pending_activity(
    state: State<'_, AppState>,
    input: PendingActivityTimeInput,
) -> Result<PendingActivityPreviewDto, crate::error::CommandError> {
    with_shared_operation!(&state, preview_pending_activity_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn post_pending_activity(
    state: State<'_, AppState>,
    input: PendingActivityTimeInput,
) -> Result<PendingActivityPostDto, crate::error::CommandError> {
    with_shared_operation!(&state, post_pending_activity_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn skip_pending_activity(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<PendingActivityDto, crate::error::CommandError> {
    with_shared_operation!(&state, skip_pending_activity_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_recurring_activity_rules(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<RecurringActivityRuleDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_recurring_activity_rules_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn create_recurring_activity_rule(
    state: State<'_, AppState>,
    input: CreateRecurringActivityRuleInput,
) -> Result<RecurringActivityRuleDto, crate::error::CommandError> {
    with_shared_operation!(&state, create_recurring_activity_rule_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn update_recurring_activity_rule(
    state: State<'_, AppState>,
    input: UpdateRecurringActivityRuleInput,
) -> Result<RecurringActivityRuleDto, crate::error::CommandError> {
    with_shared_operation!(&state, update_recurring_activity_rule_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn archive_recurring_activity_rule(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<RecurringActivityRuleDto, crate::error::CommandError> {
    with_shared_operation!(&state, archive_recurring_activity_rule_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn restore_recurring_activity_rule(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<RecurringActivityRuleDto, crate::error::CommandError> {
    with_shared_operation!(&state, restore_recurring_activity_rule_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn generate_due_pending_activities(
    state: State<'_, AppState>,
) -> Result<GenerateDuePendingActivitiesResultDto, crate::error::CommandError> {
    with_shared_operation!(&state, generate_due_pending_activities_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn create_backup(
    state: State<'_, AppState>,
    input: CreateBackupInput,
) -> Result<BackupManifestDto, crate::error::CommandError> {
    with_shared_operation!(&state, create_backup_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn inspect_backup(
    state: State<'_, AppState>,
    input: InspectBackupInput,
) -> Result<BackupInspectionDto, crate::error::CommandError> {
    with_shared_operation!(&state, inspect_backup_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_recovery_backups(
    state: State<'_, AppState>,
) -> Result<RecoveryBackupListDto, crate::error::CommandError> {
    with_shared_operation!(&state, list_recovery_backups_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn inspect_recovery_backup(
    state: State<'_, AppState>,
    input: InspectRecoveryBackupInput,
) -> Result<BackupInspectionDto, crate::error::CommandError> {
    with_shared_operation!(&state, inspect_recovery_backup_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn restore_backup(
    state: State<'_, AppState>,
    input: RestoreBackupInput,
) -> Result<RestoreBackupResultDto, crate::error::CommandError> {
    restore_backup_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn export_canonical_json(
    state: State<'_, AppState>,
    input: ExportCanonicalJsonInput,
) -> Result<CanonicalExportDto, crate::error::CommandError> {
    with_shared_operation!(&state, export_canonical_json_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn export_csv(
    state: State<'_, AppState>,
    input: ExportCsvInput,
) -> Result<CsvExportDto, crate::error::CommandError> {
    with_shared_operation!(&state, export_csv_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn preview_csv_import(
    state: State<'_, AppState>,
    input: PreviewCsvImportInput,
) -> Result<CsvImportPreviewDto, crate::error::CommandError> {
    with_shared_operation!(&state, preview_csv_import_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn commit_csv_import(
    state: State<'_, AppState>,
    input: CommitCsvImportInput,
) -> Result<CsvImportCommitDto, crate::error::CommandError> {
    with_shared_operation!(&state, commit_csv_import_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_import_batches(
    state: State<'_, AppState>,
    input: ListImportBatchesInput,
) -> Result<ImportBatchPageDto, crate::error::CommandError> {
    with_shared_operation!(&state, list_import_batches_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_import_batch(
    state: State<'_, AppState>,
    input: GetImportBatchInput,
) -> Result<ImportBatchDetailDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_import_batch_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_maintenance_items(
    state: State<'_, AppState>,
) -> Result<MaintenancePageDto, crate::error::CommandError> {
    with_shared_operation!(&state, list_maintenance_items_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn list_freshness_policies(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<FreshnessPolicyDto>, crate::error::CommandError> {
    with_shared_operation!(
        &state,
        list_freshness_policies_impl(&state, input.include_archived)
    )
}

#[tauri::command]
#[specta::specta]
pub async fn update_freshness_policy(
    state: State<'_, AppState>,
    input: UpdateFreshnessPolicyInput,
) -> Result<FreshnessPolicyDto, crate::error::CommandError> {
    with_shared_operation!(&state, update_freshness_policy_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn snooze_maintenance_item(
    state: State<'_, AppState>,
    input: SnoozeMaintenanceItemInput,
) -> Result<MaintenanceSnoozeDto, crate::error::CommandError> {
    with_shared_operation!(&state, snooze_maintenance_item_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_benchmarks(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<BenchmarkDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_benchmarks_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn create_benchmark(
    state: State<'_, AppState>,
    input: CreateBenchmarkInput,
) -> Result<BenchmarkDto, crate::error::CommandError> {
    with_shared_operation!(&state, create_benchmark_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn update_benchmark(
    state: State<'_, AppState>,
    input: UpdateBenchmarkInput,
) -> Result<BenchmarkDto, crate::error::CommandError> {
    with_shared_operation!(&state, update_benchmark_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn archive_benchmark(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<BenchmarkDto, crate::error::CommandError> {
    with_shared_operation!(&state, archive_benchmark_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn restore_benchmark(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<BenchmarkDto, crate::error::CommandError> {
    with_shared_operation!(&state, restore_benchmark_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_benchmark_observations(
    state: State<'_, AppState>,
    input: ListBenchmarkObservationsInput,
) -> Result<Vec<BenchmarkObservationDto>, crate::error::CommandError> {
    with_shared_operation!(&state, list_benchmark_observations_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn append_benchmark_observation(
    state: State<'_, AppState>,
    input: AppendBenchmarkObservationInput,
) -> Result<BenchmarkObservationDto, crate::error::CommandError> {
    with_shared_operation!(&state, append_benchmark_observation_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn set_default_benchmark(
    state: State<'_, AppState>,
    input: SetDefaultBenchmarkInput,
) -> Result<BenchmarkDto, crate::error::CommandError> {
    with_shared_operation!(&state, set_default_benchmark_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_benchmark_comparison(
    state: State<'_, AppState>,
    input: GetBenchmarkComparisonInput,
) -> Result<BenchmarkComparisonDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_benchmark_comparison_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn global_search(
    state: State<'_, AppState>,
    input: GlobalSearchInput,
) -> Result<Vec<GlobalSearchResultDto>, crate::error::CommandError> {
    with_shared_operation!(&state, global_search_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn reverse_activity(
    state: State<'_, AppState>,
    input: ReverseActivityInput,
) -> Result<ActivityDetailDto, crate::error::CommandError> {
    with_shared_operation!(&state, reverse_activity_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn correct_activity(
    state: State<'_, AppState>,
    input: CorrectActivityInput,
) -> Result<PostedCorrectionDto, crate::error::CommandError> {
    with_shared_operation!(&state, correct_activity_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_account_timeline(
    state: State<'_, AppState>,
    input: GetAccountTimelineInput,
) -> Result<AccountTimelinePageDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_account_timeline_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_history_status(
    state: State<'_, AppState>,
) -> Result<HistoryStatusDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_history_status_impl(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn rebuild_history_snapshots(
    state: State<'_, AppState>,
    input: RebuildHistorySnapshotsInput,
) -> Result<RebuildHistorySnapshotsResultDto, crate::error::CommandError> {
    with_shared_operation!(&state, rebuild_history_snapshots_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_net_worth_trend(
    state: State<'_, AppState>,
    input: GetNetWorthTrendInput,
) -> Result<NetWorthTrendDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_net_worth_trend_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_analytics_status(
    state: State<'_, AppState>,
    input: GetAnalyticsStatusInput,
) -> Result<AnalyticsStatusDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_analytics_status_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_performance_summary(
    state: State<'_, AppState>,
    input: GetPerformanceSummaryInput,
) -> Result<PerformanceSummaryDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_performance_summary_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_gain_summary(
    state: State<'_, AppState>,
    input: GetGainSummaryInput,
) -> Result<GainSummaryIpcDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_gain_summary_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_holding_gain_summaries(
    state: State<'_, AppState>,
    input: ListHoldingGainSummariesInput,
) -> Result<HoldingGainSummaryListDto, crate::error::CommandError> {
    with_shared_operation!(&state, list_holding_gain_summaries_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn get_net_worth_attribution(
    state: State<'_, AppState>,
    input: GetNetWorthAttributionInput,
) -> Result<NetWorthAttributionIpcDto, crate::error::CommandError> {
    with_shared_operation!(&state, get_net_worth_attribution_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_holding_lots(
    state: State<'_, AppState>,
    input: ListHoldingLotsInput,
) -> Result<HoldingLotPageDto, crate::error::CommandError> {
    with_shared_operation!(&state, list_holding_lots_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_unknown_basis_lots(
    state: State<'_, AppState>,
    input: ListUnknownBasisLotsInput,
) -> Result<HoldingLotPageDto, crate::error::CommandError> {
    with_shared_operation!(&state, list_unknown_basis_lots_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn list_cost_basis_declarations(
    state: State<'_, AppState>,
    input: ListCostBasisDeclarationsInput,
) -> Result<CostBasisDeclarationPageDto, crate::error::CommandError> {
    with_shared_operation!(&state, list_cost_basis_declarations_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn declare_lot_cost_basis(
    state: State<'_, AppState>,
    input: DeclareLotCostBasisInput,
) -> Result<CostBasisDeclarationIpcDto, crate::error::CommandError> {
    with_shared_operation!(&state, declare_lot_cost_basis_impl(&state, input))
}

#[tauri::command]
#[specta::specta]
pub async fn revoke_lot_cost_basis(
    state: State<'_, AppState>,
    input: RevokeLotCostBasisInput,
) -> Result<CostBasisDeclarationIpcDto, crate::error::CommandError> {
    with_shared_operation!(&state, revoke_lot_cost_basis_impl(&state, input))
}

pub fn command_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        bootstrap,
        complete_onboarding,
        list_members,
        create_member,
        update_member,
        archive_member,
        restore_member,
        list_institutions,
        create_institution,
        update_institution,
        archive_institution,
        restore_institution,
        list_groups,
        create_group,
        update_group,
        archive_group,
        restore_group,
        list_accounts,
        get_account,
        create_account,
        update_account,
        update_account_value,
        archive_account,
        restore_account,
        get_overview,
        set_member_avatar,
        set_institution_logo,
        set_group_logo,
        set_account_logo,
        set_instrument_logo,
        list_instruments,
        get_instrument,
        create_instrument,
        update_instrument,
        archive_instrument,
        restore_instrument,
        list_holdings,
        create_holding,
        update_holding,
        archive_holding,
        restore_holding,
        list_account_cash,
        append_account_cash,
        list_instrument_quotes,
        append_manual_instrument_quote,
        set_instrument_quote_preference,
        list_required_fx,
        list_fx_quotes,
        append_manual_fx_quote,
        set_fx_quote_preference,
        get_portfolio,
        search_provider_instruments,
        get_market_data_capabilities,
        refresh_instrument,
        refresh_required_fx,
        refresh_all,
        backfill_instrument_history,
        backfill_required_fx_history,
        backfill_all_history,
        get_media,
        get_settings,
        update_settings,
        delete_all_data,
        get_history_origin,
        confirm_history_timezone,
        preview_activity,
        list_activities,
        get_activity,
        create_activity,
        create_pending_activity,
        update_pending_activity,
        list_pending_activities,
        preview_pending_activity,
        post_pending_activity,
        skip_pending_activity,
        list_recurring_activity_rules,
        create_recurring_activity_rule,
        update_recurring_activity_rule,
        archive_recurring_activity_rule,
        restore_recurring_activity_rule,
        generate_due_pending_activities,
        create_backup,
        inspect_backup,
        list_recovery_backups,
        inspect_recovery_backup,
        restore_backup,
        export_canonical_json,
        export_csv,
        preview_csv_import,
        commit_csv_import,
        list_import_batches,
        get_import_batch,
        list_maintenance_items,
        list_freshness_policies,
        update_freshness_policy,
        snooze_maintenance_item,
        list_benchmarks,
        create_benchmark,
        update_benchmark,
        archive_benchmark,
        restore_benchmark,
        list_benchmark_observations,
        append_benchmark_observation,
        set_default_benchmark,
        get_benchmark_comparison,
        reverse_activity,
        correct_activity,
        get_account_timeline,
        get_history_status,
        rebuild_history_snapshots,
        get_net_worth_trend,
        get_analytics_status,
        get_performance_summary,
        get_gain_summary,
        list_holding_gain_summaries,
        get_net_worth_attribution,
        list_holding_lots,
        list_unknown_basis_lots,
        list_cost_basis_declarations,
        declare_lot_cost_basis,
        revoke_lot_cost_basis,
        global_search
    ])
}
