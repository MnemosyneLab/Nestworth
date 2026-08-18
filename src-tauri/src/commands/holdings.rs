use crate::{
    application::{
        holding_service::{
            self, CreateHoldingInput, HoldingRecordDto, ListHoldingsInput, UpdateHoldingInput,
        },
        reference::IdInput,
    },
    error::CommandError,
    state::AppState,
};

pub async fn list_holdings_impl(
    state: &AppState,
    input: ListHoldingsInput,
) -> Result<Vec<HoldingRecordDto>, CommandError> {
    holding_service::list_holdings(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn create_holding_impl(
    state: &AppState,
    input: CreateHoldingInput,
) -> Result<HoldingRecordDto, CommandError> {
    holding_service::create_holding(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn update_holding_impl(
    state: &AppState,
    input: UpdateHoldingInput,
) -> Result<HoldingRecordDto, CommandError> {
    holding_service::update_holding(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn archive_holding_impl(
    state: &AppState,
    input: IdInput,
) -> Result<HoldingRecordDto, CommandError> {
    holding_service::archive_holding(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn restore_holding_impl(
    state: &AppState,
    input: IdInput,
) -> Result<HoldingRecordDto, CommandError> {
    holding_service::restore_holding(state, &input.id)
        .await
        .map_err(CommandError::from)
}
