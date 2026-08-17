use crate::{
    application::overview_service::{self, OverviewDto},
    error::CommandError,
    state::AppState,
};

pub async fn get_overview_impl(state: &AppState) -> Result<OverviewDto, CommandError> {
    overview_service::get_overview(state)
        .await
        .map_err(CommandError::from)
}
