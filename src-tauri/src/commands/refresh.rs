use crate::{
    application::refresh_service::{
        self, ProviderInstrumentDto, RefreshInstrumentInput, RefreshResultDto,
        SearchProviderInstrumentsInput,
    },
    error::CommandError,
    state::AppState,
};

pub async fn search_provider_instruments_impl(
    state: &AppState,
    input: SearchProviderInstrumentsInput,
) -> Result<Vec<ProviderInstrumentDto>, CommandError> {
    refresh_service::search_provider_instruments(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn refresh_instrument_impl(
    state: &AppState,
    input: RefreshInstrumentInput,
) -> Result<RefreshResultDto, CommandError> {
    refresh_service::refresh_instrument(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn refresh_required_fx_impl(state: &AppState) -> Result<RefreshResultDto, CommandError> {
    refresh_service::refresh_required_fx(state)
        .await
        .map_err(CommandError::from)
}

pub async fn refresh_all_impl(state: &AppState) -> Result<RefreshResultDto, CommandError> {
    refresh_service::refresh_all(state)
        .await
        .map_err(CommandError::from)
}
