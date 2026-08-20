use crate::{
    application::{
        analytics_query_service::{
            self, AnalyticsStatusDto, CostBasisDeclarationIpcDto, CostBasisDeclarationPageDto,
            DeclareLotCostBasisInput, GainSummaryIpcDto, GetAnalyticsStatusInput,
            GetGainSummaryInput, GetNetWorthAttributionInput, GetPerformanceSummaryInput,
            HoldingGainSummaryListDto, HoldingLotPageDto, ListCostBasisDeclarationsInput,
            ListHoldingGainSummariesInput, ListHoldingLotsInput, ListUnknownBasisLotsInput,
            NetWorthAttributionIpcDto, RevokeLotCostBasisInput,
        },
        return_service::PerformanceSummaryDto,
    },
    error::CommandError,
    state::AppState,
};

pub async fn get_analytics_status_impl(
    state: &AppState,
    input: GetAnalyticsStatusInput,
) -> Result<AnalyticsStatusDto, CommandError> {
    analytics_query_service::get_analytics_status(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn get_performance_summary_impl(
    state: &AppState,
    input: GetPerformanceSummaryInput,
) -> Result<PerformanceSummaryDto, CommandError> {
    analytics_query_service::get_performance_summary(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn get_gain_summary_impl(
    state: &AppState,
    input: GetGainSummaryInput,
) -> Result<GainSummaryIpcDto, CommandError> {
    analytics_query_service::get_gain_summary(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn list_holding_gain_summaries_impl(
    state: &AppState,
    input: ListHoldingGainSummariesInput,
) -> Result<HoldingGainSummaryListDto, CommandError> {
    analytics_query_service::list_holding_gain_summaries(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn get_net_worth_attribution_impl(
    state: &AppState,
    input: GetNetWorthAttributionInput,
) -> Result<NetWorthAttributionIpcDto, CommandError> {
    analytics_query_service::get_net_worth_attribution(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn list_holding_lots_impl(
    state: &AppState,
    input: ListHoldingLotsInput,
) -> Result<HoldingLotPageDto, CommandError> {
    analytics_query_service::list_holding_lots(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn list_unknown_basis_lots_impl(
    state: &AppState,
    input: ListUnknownBasisLotsInput,
) -> Result<HoldingLotPageDto, CommandError> {
    analytics_query_service::list_unknown_basis_lots(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn list_cost_basis_declarations_impl(
    state: &AppState,
    input: ListCostBasisDeclarationsInput,
) -> Result<CostBasisDeclarationPageDto, CommandError> {
    analytics_query_service::list_cost_basis_declarations(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn declare_lot_cost_basis_impl(
    state: &AppState,
    input: DeclareLotCostBasisInput,
) -> Result<CostBasisDeclarationIpcDto, CommandError> {
    analytics_query_service::declare_lot_cost_basis(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn revoke_lot_cost_basis_impl(
    state: &AppState,
    input: RevokeLotCostBasisInput,
) -> Result<CostBasisDeclarationIpcDto, CommandError> {
    analytics_query_service::revoke_lot_cost_basis(state, input)
        .await
        .map_err(CommandError::from)
}
