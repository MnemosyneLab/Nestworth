use crate::{
    application::search_service::{self, GlobalSearchInput, GlobalSearchResultDto},
    error::CommandError,
    state::AppState,
};

pub async fn global_search_impl(
    state: &AppState,
    input: GlobalSearchInput,
) -> Result<Vec<GlobalSearchResultDto>, CommandError> {
    search_service::global_search(state, input)
        .await
        .map_err(CommandError::from)
}
