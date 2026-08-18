use crate::{
    application::cash_service::{
        self, AccountCashRecordDto, AppendAccountCashInput, ListAccountCashInput,
    },
    error::CommandError,
    state::AppState,
};

pub async fn list_account_cash_impl(
    state: &AppState,
    input: ListAccountCashInput,
) -> Result<Vec<AccountCashRecordDto>, CommandError> {
    cash_service::list_account_cash(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn append_account_cash_impl(
    state: &AppState,
    input: AppendAccountCashInput,
) -> Result<AccountCashRecordDto, CommandError> {
    cash_service::append_account_cash(state, input)
        .await
        .map_err(CommandError::from)
}
