use crate::{
    application::{
        instrument_service::{
            self, CreateInstrumentInput, InstrumentRecordDto, UpdateInstrumentInput,
        },
        reference::{IdInput, ListFilterInput},
    },
    error::CommandError,
    state::AppState,
};

pub async fn list_instruments_impl(
    state: &AppState,
    input: ListFilterInput,
) -> Result<Vec<InstrumentRecordDto>, CommandError> {
    instrument_service::list_instruments(state, input.include_archived)
        .await
        .map_err(CommandError::from)
}

pub async fn get_instrument_impl(
    state: &AppState,
    input: IdInput,
) -> Result<InstrumentRecordDto, CommandError> {
    instrument_service::get_instrument(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn create_instrument_impl(
    state: &AppState,
    input: CreateInstrumentInput,
) -> Result<InstrumentRecordDto, CommandError> {
    instrument_service::create_instrument(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn update_instrument_impl(
    state: &AppState,
    input: UpdateInstrumentInput,
) -> Result<InstrumentRecordDto, CommandError> {
    instrument_service::update_instrument(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn archive_instrument_impl(
    state: &AppState,
    input: IdInput,
) -> Result<InstrumentRecordDto, CommandError> {
    instrument_service::archive_instrument(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn restore_instrument_impl(
    state: &AppState,
    input: IdInput,
) -> Result<InstrumentRecordDto, CommandError> {
    instrument_service::restore_instrument(state, &input.id)
        .await
        .map_err(CommandError::from)
}
