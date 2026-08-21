use crate::{
    application::{
        market_data::MarketDataCapabilitiesDto,
        market_data_history_service::{
            backfill_all_history, backfill_instrument_history, backfill_required_fx_history,
            BackfillHistoryRangeInput, BackfillInstrumentHistoryInput,
        },
        refresh_service::{
            self, ProviderInstrumentDto, RefreshInstrumentInput, RefreshResultDto,
            SearchProviderInstrumentsInput,
        },
    },
    error::CommandError,
    state::AppState,
};

pub async fn get_market_data_capabilities_impl(
    state: &AppState,
) -> Result<MarketDataCapabilitiesDto, CommandError> {
    Ok(state.market_data().capabilities_dto())
}

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

pub async fn backfill_instrument_history_impl(
    state: &AppState,
    input: BackfillInstrumentHistoryInput,
) -> Result<RefreshResultDto, CommandError> {
    backfill_instrument_history(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn backfill_required_fx_history_impl(
    state: &AppState,
    input: BackfillHistoryRangeInput,
) -> Result<RefreshResultDto, CommandError> {
    backfill_required_fx_history(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn backfill_all_history_impl(
    state: &AppState,
    input: BackfillHistoryRangeInput,
) -> Result<RefreshResultDto, CommandError> {
    backfill_all_history(state, input)
        .await
        .map_err(CommandError::from)
}
