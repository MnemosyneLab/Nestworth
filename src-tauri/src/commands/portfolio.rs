use crate::{
    application::portfolio_service::{self, PortfolioDto},
    error::CommandError,
    state::AppState,
};

pub async fn get_portfolio_impl(state: &AppState) -> Result<PortfolioDto, CommandError> {
    portfolio_service::get_portfolio(state)
        .await
        .map_err(CommandError::from)
}
