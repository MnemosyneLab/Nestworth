use tauri::State;
use tauri_specta::{collect_commands, Builder};

use crate::{
    application::{
        account_service::{
            AccountRecordDto, CreateAccountInput, UpdateAccountInput, UpdateAccountValueInput,
        },
        cash_service::{AccountCashRecordDto, AppendAccountCashInput, ListAccountCashInput},
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
        media_service::{GetMediaInput, MediaAssetDto, SetMediaInput},
        member_service::{CreateMemberInput, MemberRecordDto, UpdateMemberInput},
        onboarding_service::CompleteOnboardingInput,
        overview_service::OverviewDto,
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
        settings_service::{AppSettingsDto, DeleteAllDataInput, UpdateSettingsInput},
    },
    commands::{
        accounts::{
            archive_account_impl, create_account_impl, get_account_impl, list_accounts_impl,
            restore_account_impl, update_account_impl, update_account_value_impl,
        },
        bootstrap::{bootstrap_impl, BootstrapDto},
        cash::{append_account_cash_impl, list_account_cash_impl},
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
        portfolio::get_portfolio_impl,
        quotes::{
            append_manual_fx_quote_impl, append_manual_instrument_quote_impl, list_fx_quotes_impl,
            list_instrument_quotes_impl, list_required_fx_impl, set_fx_quote_preference_impl,
            set_instrument_quote_preference_impl,
        },
        refresh::{
            refresh_all_impl, refresh_instrument_impl, refresh_required_fx_impl,
            search_provider_instruments_impl,
        },
        settings::{delete_all_data_impl, get_settings_impl, update_settings_impl},
    },
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn bootstrap(
    state: State<'_, AppState>,
) -> Result<BootstrapDto, crate::error::CommandError> {
    bootstrap_impl(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn complete_onboarding(
    state: State<'_, AppState>,
    input: CompleteOnboardingInput,
) -> Result<(), crate::error::CommandError> {
    complete_onboarding_impl(state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_members(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<MemberRecordDto>, crate::error::CommandError> {
    list_members_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_member(
    state: State<'_, AppState>,
    input: CreateMemberInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    create_member_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_member(
    state: State<'_, AppState>,
    input: UpdateMemberInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    update_member_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn archive_member(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    archive_member_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_member(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    restore_member_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_institutions(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<InstitutionRecordDto>, crate::error::CommandError> {
    list_institutions_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_institution(
    state: State<'_, AppState>,
    input: CreateInstitutionInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    create_institution_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_institution(
    state: State<'_, AppState>,
    input: UpdateInstitutionInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    update_institution_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn archive_institution(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    archive_institution_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_institution(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    restore_institution_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_groups(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<GroupRecordDto>, crate::error::CommandError> {
    list_groups_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_group(
    state: State<'_, AppState>,
    input: CreateGroupInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    create_group_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_group(
    state: State<'_, AppState>,
    input: UpdateGroupInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    update_group_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn archive_group(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    archive_group_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_group(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    restore_group_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_accounts(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<AccountRecordDto>, crate::error::CommandError> {
    list_accounts_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_account(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    get_account_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_account(
    state: State<'_, AppState>,
    input: CreateAccountInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    create_account_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_account(
    state: State<'_, AppState>,
    input: UpdateAccountInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    update_account_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_account_value(
    state: State<'_, AppState>,
    input: UpdateAccountValueInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    update_account_value_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn archive_account(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    archive_account_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_account(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    restore_account_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_overview(
    state: State<'_, AppState>,
) -> Result<OverviewDto, crate::error::CommandError> {
    get_overview_impl(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_member_avatar(
    state: State<'_, AppState>,
    input: SetMediaInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    set_member_avatar_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_institution_logo(
    state: State<'_, AppState>,
    input: SetMediaInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    set_institution_logo_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_group_logo(
    state: State<'_, AppState>,
    input: SetMediaInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    set_group_logo_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_account_logo(
    state: State<'_, AppState>,
    input: SetMediaInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    set_account_logo_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_instrument_logo(
    state: State<'_, AppState>,
    input: SetMediaInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    set_instrument_logo_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_instruments(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<InstrumentRecordDto>, crate::error::CommandError> {
    list_instruments_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_instrument(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    get_instrument_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_instrument(
    state: State<'_, AppState>,
    input: CreateInstrumentInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    create_instrument_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_instrument(
    state: State<'_, AppState>,
    input: UpdateInstrumentInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    update_instrument_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn archive_instrument(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    archive_instrument_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_instrument(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    restore_instrument_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_holdings(
    state: State<'_, AppState>,
    input: ListHoldingsInput,
) -> Result<Vec<HoldingRecordDto>, crate::error::CommandError> {
    list_holdings_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_holding(
    state: State<'_, AppState>,
    input: CreateHoldingInput,
) -> Result<HoldingRecordDto, crate::error::CommandError> {
    create_holding_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_holding(
    state: State<'_, AppState>,
    input: UpdateHoldingInput,
) -> Result<HoldingRecordDto, crate::error::CommandError> {
    update_holding_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn archive_holding(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<HoldingRecordDto, crate::error::CommandError> {
    archive_holding_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_holding(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<HoldingRecordDto, crate::error::CommandError> {
    restore_holding_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_account_cash(
    state: State<'_, AppState>,
    input: ListAccountCashInput,
) -> Result<Vec<AccountCashRecordDto>, crate::error::CommandError> {
    list_account_cash_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn append_account_cash(
    state: State<'_, AppState>,
    input: AppendAccountCashInput,
) -> Result<AccountCashRecordDto, crate::error::CommandError> {
    append_account_cash_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_instrument_quotes(
    state: State<'_, AppState>,
    input: ListInstrumentQuotesInput,
) -> Result<Vec<InstrumentQuoteRecordDto>, crate::error::CommandError> {
    list_instrument_quotes_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn append_manual_instrument_quote(
    state: State<'_, AppState>,
    input: AppendManualInstrumentQuoteInput,
) -> Result<InstrumentQuoteRecordDto, crate::error::CommandError> {
    append_manual_instrument_quote_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_instrument_quote_preference(
    state: State<'_, AppState>,
    input: SetInstrumentQuotePreferenceInput,
) -> Result<InstrumentRecordDto, crate::error::CommandError> {
    set_instrument_quote_preference_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_required_fx(
    state: State<'_, AppState>,
) -> Result<Vec<FxPairStatusDto>, crate::error::CommandError> {
    list_required_fx_impl(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_fx_quotes(
    state: State<'_, AppState>,
    input: ListFxQuotesInput,
) -> Result<Vec<FxQuoteRecordDto>, crate::error::CommandError> {
    list_fx_quotes_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn append_manual_fx_quote(
    state: State<'_, AppState>,
    input: AppendManualFxQuoteInput,
) -> Result<FxQuoteRecordDto, crate::error::CommandError> {
    append_manual_fx_quote_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_fx_quote_preference(
    state: State<'_, AppState>,
    input: SetFxQuotePreferenceInput,
) -> Result<FxPairStatusDto, crate::error::CommandError> {
    set_fx_quote_preference_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_portfolio(
    state: State<'_, AppState>,
) -> Result<PortfolioDto, crate::error::CommandError> {
    get_portfolio_impl(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn search_provider_instruments(
    state: State<'_, AppState>,
    input: SearchProviderInstrumentsInput,
) -> Result<Vec<ProviderInstrumentDto>, crate::error::CommandError> {
    search_provider_instruments_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn refresh_instrument(
    state: State<'_, AppState>,
    input: RefreshInstrumentInput,
) -> Result<RefreshResultDto, crate::error::CommandError> {
    refresh_instrument_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn refresh_required_fx(
    state: State<'_, AppState>,
) -> Result<RefreshResultDto, crate::error::CommandError> {
    refresh_required_fx_impl(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn refresh_all(
    state: State<'_, AppState>,
) -> Result<RefreshResultDto, crate::error::CommandError> {
    refresh_all_impl(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_media(
    state: State<'_, AppState>,
    input: GetMediaInput,
) -> Result<MediaAssetDto, crate::error::CommandError> {
    get_media_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_settings(
    state: State<'_, AppState>,
) -> Result<AppSettingsDto, crate::error::CommandError> {
    get_settings_impl(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_settings(
    state: State<'_, AppState>,
    input: UpdateSettingsInput,
) -> Result<AppSettingsDto, crate::error::CommandError> {
    update_settings_impl(&state, input).await
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
    get_history_origin_impl(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn confirm_history_timezone(
    state: State<'_, AppState>,
    input: ConfirmHistoryTimezoneInput,
) -> Result<HistoryOriginDto, crate::error::CommandError> {
    confirm_history_timezone_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn preview_activity(
    state: State<'_, AppState>,
    input: CreateActivityInput,
) -> Result<ActivityPreviewDto, crate::error::CommandError> {
    preview_activity_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_activities(
    state: State<'_, AppState>,
    input: ListActivitiesInput,
) -> Result<ActivityPageDto, crate::error::CommandError> {
    list_activities_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_activity(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<ActivityDetailDto, crate::error::CommandError> {
    get_activity_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_activity(
    state: State<'_, AppState>,
    input: CreateActivityInput,
) -> Result<ActivityDetailDto, crate::error::CommandError> {
    create_activity_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn reverse_activity(
    state: State<'_, AppState>,
    input: ReverseActivityInput,
) -> Result<ActivityDetailDto, crate::error::CommandError> {
    reverse_activity_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn correct_activity(
    state: State<'_, AppState>,
    input: CorrectActivityInput,
) -> Result<PostedCorrectionDto, crate::error::CommandError> {
    correct_activity_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_account_timeline(
    state: State<'_, AppState>,
    input: GetAccountTimelineInput,
) -> Result<AccountTimelinePageDto, crate::error::CommandError> {
    get_account_timeline_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_history_status(
    state: State<'_, AppState>,
) -> Result<HistoryStatusDto, crate::error::CommandError> {
    get_history_status_impl(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn rebuild_history_snapshots(
    state: State<'_, AppState>,
    input: RebuildHistorySnapshotsInput,
) -> Result<RebuildHistorySnapshotsResultDto, crate::error::CommandError> {
    rebuild_history_snapshots_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_net_worth_trend(
    state: State<'_, AppState>,
    input: GetNetWorthTrendInput,
) -> Result<NetWorthTrendDto, crate::error::CommandError> {
    get_net_worth_trend_impl(&state, input).await
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
        refresh_instrument,
        refresh_required_fx,
        refresh_all,
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
        reverse_activity,
        correct_activity,
        get_account_timeline,
        get_history_status,
        rebuild_history_snapshots,
        get_net_worth_trend
    ])
}
