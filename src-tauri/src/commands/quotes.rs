use crate::{
    application::{
        instrument_service::InstrumentRecordDto,
        portfolio_service,
        quote_service::{
            self, AppendManualFxQuoteInput, AppendManualInstrumentQuoteInput, FxPairStatusDto,
            FxQuoteRecordDto, InstrumentQuoteRecordDto, ListFxQuotesInput,
            ListInstrumentQuotesInput, SetFxQuotePreferenceInput,
            SetInstrumentQuotePreferenceInput,
        },
    },
    error::CommandError,
    state::AppState,
};

pub async fn list_instrument_quotes_impl(
    state: &AppState,
    input: ListInstrumentQuotesInput,
) -> Result<Vec<InstrumentQuoteRecordDto>, CommandError> {
    quote_service::list_instrument_quotes(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn append_manual_instrument_quote_impl(
    state: &AppState,
    input: AppendManualInstrumentQuoteInput,
) -> Result<InstrumentQuoteRecordDto, CommandError> {
    quote_service::append_manual_instrument_quote(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn set_instrument_quote_preference_impl(
    state: &AppState,
    input: SetInstrumentQuotePreferenceInput,
) -> Result<InstrumentRecordDto, CommandError> {
    quote_service::set_instrument_quote_preference(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn list_required_fx_impl(state: &AppState) -> Result<Vec<FxPairStatusDto>, CommandError> {
    portfolio_service::list_required_fx(state)
        .await
        .map_err(CommandError::from)
}

pub async fn list_fx_quotes_impl(
    state: &AppState,
    input: ListFxQuotesInput,
) -> Result<Vec<FxQuoteRecordDto>, CommandError> {
    quote_service::list_fx_quotes(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn append_manual_fx_quote_impl(
    state: &AppState,
    input: AppendManualFxQuoteInput,
) -> Result<FxQuoteRecordDto, CommandError> {
    quote_service::append_manual_fx_quote(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn set_fx_quote_preference_impl(
    state: &AppState,
    input: SetFxQuotePreferenceInput,
) -> Result<FxPairStatusDto, CommandError> {
    quote_service::set_fx_quote_preference(state, input)
        .await
        .map_err(CommandError::from)
}
