use crate::{
    application::maintenance_service::{
        self, FreshnessPolicyDto, MaintenancePageDto, MaintenanceSnoozeDto,
        SnoozeMaintenanceItemInput, UpdateFreshnessPolicyInput,
    },
    error::CommandError,
    state::AppState,
};

pub async fn list_maintenance_items_impl(
    state: &AppState,
) -> Result<MaintenancePageDto, CommandError> {
    maintenance_service::list_maintenance_items(state)
        .await
        .map_err(CommandError::from)
}

pub async fn list_freshness_policies_impl(
    state: &AppState,
    include_archived: bool,
) -> Result<Vec<FreshnessPolicyDto>, CommandError> {
    maintenance_service::list_freshness_policies(state, include_archived)
        .await
        .map_err(CommandError::from)
}

pub async fn update_freshness_policy_impl(
    state: &AppState,
    input: UpdateFreshnessPolicyInput,
) -> Result<FreshnessPolicyDto, CommandError> {
    maintenance_service::update_freshness_policy(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn snooze_maintenance_item_impl(
    state: &AppState,
    input: SnoozeMaintenanceItemInput,
) -> Result<MaintenanceSnoozeDto, CommandError> {
    maintenance_service::snooze_maintenance_item(state, input)
        .await
        .map_err(CommandError::from)
}
