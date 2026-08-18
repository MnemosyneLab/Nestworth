use crate::{
    application::{
        account_service::{
            self, AccountRecordDto, CreateAccountInput, UpdateAccountInput, UpdateAccountValueInput,
        },
        reference::{IdInput, ListFilterInput},
    },
    error::CommandError,
    state::AppState,
};

pub async fn list_accounts_impl(
    state: &AppState,
    input: ListFilterInput,
) -> Result<Vec<AccountRecordDto>, CommandError> {
    account_service::list_accounts(state, input.include_archived)
        .await
        .map_err(CommandError::from)
}

pub async fn get_account_impl(
    state: &AppState,
    input: IdInput,
) -> Result<AccountRecordDto, CommandError> {
    account_service::get_account(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn create_account_impl(
    state: &AppState,
    input: CreateAccountInput,
) -> Result<AccountRecordDto, CommandError> {
    account_service::create_account(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn update_account_impl(
    state: &AppState,
    input: UpdateAccountInput,
) -> Result<AccountRecordDto, CommandError> {
    account_service::update_account(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn update_account_value_impl(
    state: &AppState,
    input: UpdateAccountValueInput,
) -> Result<AccountRecordDto, CommandError> {
    account_service::update_account_value(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn archive_account_impl(
    state: &AppState,
    input: IdInput,
) -> Result<AccountRecordDto, CommandError> {
    account_service::archive_account(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn restore_account_impl(
    state: &AppState,
    input: IdInput,
) -> Result<AccountRecordDto, CommandError> {
    account_service::restore_account(state, &input.id)
        .await
        .map_err(CommandError::from)
}
