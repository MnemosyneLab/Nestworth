use crate::{
    application::{
        history_query_service::{
            self, AccountTimelinePageDto, ActivityDetailDto, ActivityPageDto, ActivityPreviewDto,
            ConfirmHistoryTimezoneInput, CorrectActivityInput, CreateActivityInput,
            GetAccountTimelineInput, HistoryOriginDto, ListActivitiesInput, PostedCorrectionDto,
            ReverseActivityInput,
        },
        history_snapshot_service::{
            self, GetNetWorthTrendInput, HistoryStatusDto, NetWorthTrendDto,
            RebuildHistorySnapshotsInput, RebuildHistorySnapshotsResultDto,
        },
        reference::IdInput,
    },
    error::CommandError,
    state::AppState,
};

pub async fn get_history_origin_impl(state: &AppState) -> Result<HistoryOriginDto, CommandError> {
    history_query_service::get_history_origin(state)
        .await
        .map_err(CommandError::from)
}

pub async fn confirm_history_timezone_impl(
    state: &AppState,
    input: ConfirmHistoryTimezoneInput,
) -> Result<HistoryOriginDto, CommandError> {
    history_query_service::confirm_history_timezone(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn preview_activity_impl(
    state: &AppState,
    input: CreateActivityInput,
) -> Result<ActivityPreviewDto, CommandError> {
    history_query_service::preview_activity(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn list_activities_impl(
    state: &AppState,
    input: ListActivitiesInput,
) -> Result<ActivityPageDto, CommandError> {
    history_query_service::list_activities(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn get_activity_impl(
    state: &AppState,
    input: IdInput,
) -> Result<ActivityDetailDto, CommandError> {
    history_query_service::get_activity(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn create_activity_impl(
    state: &AppState,
    input: CreateActivityInput,
) -> Result<ActivityDetailDto, CommandError> {
    history_query_service::create_activity(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn reverse_activity_impl(
    state: &AppState,
    input: ReverseActivityInput,
) -> Result<ActivityDetailDto, CommandError> {
    history_query_service::reverse_activity(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn correct_activity_impl(
    state: &AppState,
    input: CorrectActivityInput,
) -> Result<PostedCorrectionDto, CommandError> {
    history_query_service::correct_activity(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn get_account_timeline_impl(
    state: &AppState,
    input: GetAccountTimelineInput,
) -> Result<AccountTimelinePageDto, CommandError> {
    history_query_service::get_account_timeline(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn get_history_status_impl(state: &AppState) -> Result<HistoryStatusDto, CommandError> {
    history_snapshot_service::get_history_status(state)
        .await
        .map_err(CommandError::from)
}

pub async fn rebuild_history_snapshots_impl(
    state: &AppState,
    input: RebuildHistorySnapshotsInput,
) -> Result<RebuildHistorySnapshotsResultDto, CommandError> {
    history_snapshot_service::rebuild_history_snapshots(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn get_net_worth_trend_impl(
    state: &AppState,
    input: GetNetWorthTrendInput,
) -> Result<NetWorthTrendDto, CommandError> {
    history_snapshot_service::get_net_worth_trend(state, input)
        .await
        .map_err(CommandError::from)
}
