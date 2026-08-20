use crate::{
    application::pending_service::{
        self, CreatePendingActivityInput, CreateRecurringActivityRuleInput,
        GenerateDuePendingActivitiesResultDto, ListPendingActivitiesInput, PendingActivityDto,
        PendingActivityPageDto, PendingActivityPostDto, PendingActivityPreviewDto,
        PendingActivityTimeInput, RecurringActivityRuleDto, UpdatePendingActivityInput,
        UpdateRecurringActivityRuleInput,
    },
    application::reference::{IdInput, ListFilterInput},
    error::CommandError,
    state::AppState,
};

pub async fn create_pending_activity_impl(
    state: &AppState,
    input: CreatePendingActivityInput,
) -> Result<PendingActivityDto, CommandError> {
    pending_service::create_pending_activity(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn update_pending_activity_impl(
    state: &AppState,
    input: UpdatePendingActivityInput,
) -> Result<PendingActivityDto, CommandError> {
    pending_service::update_pending_activity(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn list_pending_activities_impl(
    state: &AppState,
    input: ListPendingActivitiesInput,
) -> Result<PendingActivityPageDto, CommandError> {
    pending_service::list_pending_activities(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn preview_pending_activity_impl(
    state: &AppState,
    input: PendingActivityTimeInput,
) -> Result<PendingActivityPreviewDto, CommandError> {
    pending_service::preview_pending_activity(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn post_pending_activity_impl(
    state: &AppState,
    input: PendingActivityTimeInput,
) -> Result<PendingActivityPostDto, CommandError> {
    pending_service::post_pending_activity(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn skip_pending_activity_impl(
    state: &AppState,
    input: IdInput,
) -> Result<PendingActivityDto, CommandError> {
    pending_service::skip_pending_activity(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn create_recurring_activity_rule_impl(
    state: &AppState,
    input: CreateRecurringActivityRuleInput,
) -> Result<RecurringActivityRuleDto, CommandError> {
    pending_service::create_recurring_activity_rule(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn update_recurring_activity_rule_impl(
    state: &AppState,
    input: UpdateRecurringActivityRuleInput,
) -> Result<RecurringActivityRuleDto, CommandError> {
    pending_service::update_recurring_activity_rule(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn list_recurring_activity_rules_impl(
    state: &AppState,
    input: ListFilterInput,
) -> Result<Vec<RecurringActivityRuleDto>, CommandError> {
    pending_service::list_recurring_activity_rules(state, input.include_archived)
        .await
        .map_err(CommandError::from)
}

pub async fn archive_recurring_activity_rule_impl(
    state: &AppState,
    input: IdInput,
) -> Result<RecurringActivityRuleDto, CommandError> {
    pending_service::archive_recurring_activity_rule(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn restore_recurring_activity_rule_impl(
    state: &AppState,
    input: IdInput,
) -> Result<RecurringActivityRuleDto, CommandError> {
    pending_service::restore_recurring_activity_rule(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn generate_due_pending_activities_impl(
    state: &AppState,
) -> Result<GenerateDuePendingActivitiesResultDto, CommandError> {
    pending_service::generate_due_pending_activities(state)
        .await
        .map_err(CommandError::from)
}
