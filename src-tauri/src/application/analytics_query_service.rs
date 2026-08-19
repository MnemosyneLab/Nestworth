//! IPC-facing analytics queries.
//!
//! Adds specta DTOs and dispatches to existing free functions. No service struct.

use std::collections::{HashMap, HashSet};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    account_service::{self, AccountRecordDto, MoneyDto},
    analytics_repositories,
    attribution_service::{self, AvailableAttributionDto, NetWorthAttributionDto},
    cost_basis_service::{self, CostBasisDeclarationDto},
    currency_decomposition,
    gain_service::{self, SignedMoneyDto},
    history_repositories,
    income_fee_service::{self, FeeBucketDto, IncomeBucketDto},
    query_count,
    reference::{begin_read_tx, finish_read_tx, require_household_tx},
    return_service::{self, PerformanceSummaryDto},
    valuation_service::ValuationSnapshot,
};
use crate::{
    domain::{
        round_to_money_scale, AccountId, ActivityLegId, AnalyticsScope, BasisStatus, CalendarDate,
        HistoryTimezone, HoldingId, InstrumentId, LotRef, Money, OpenLot, SignedMoney, Timestamp,
    },
    error::AppError,
    state::AppState,
};

const DEFAULT_PAGE_SIZE: i32 = 50;
const MAX_PAGE_SIZE: i32 = 100;
pub const REASON_PERIOD_UNAVAILABLE: &str = "ANALYTICS_PERIOD_UNAVAILABLE";
pub const REASON_INPUT_INCOMPLETE: &str = "ANALYTICS_INPUT_INCOMPLETE";
pub const REASON_UNKNOWN_BASIS: &str = "UNKNOWN_BASIS";
const SOURCE_ORIGIN: &str = "originHolding";
const SOURCE_ACQUISITION: &str = "acquisition";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AnalyticsScopeDto {
    Household,
    Portfolio,
    #[serde(rename_all = "camelCase")]
    Account {
        account_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Instrument {
        instrument_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AnalyticsPeriodDto {
    OneMonth,
    ThreeMonths,
    OneYear,
    All,
    #[serde(rename_all = "camelCase")]
    Custom {
        start_local_date: String,
        end_local_date: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LotRefSourceKind {
    OriginHolding,
    Acquisition,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LotRefDto {
    pub source_kind: LotRefSourceKind,
    pub source_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DateRangeAvailabilityDto {
    #[serde(rename_all = "camelCase")]
    Available {
        start_local_date: String,
        end_local_date: String,
    },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        reason: String,
        blocking_dates: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DateAvailabilityDto {
    #[serde(rename_all = "camelCase")]
    Available { value: String },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        reason: String,
        blocking_dates: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MoneyAvailabilityDto {
    #[serde(rename_all = "camelCase")]
    Available { value: MoneyDto },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        reason: String,
        blocking_dates: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SignedMoneyAvailabilityDto {
    #[serde(rename_all = "camelCase")]
    Available { value: SignedMoneyDto },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        reason: String,
        blocking_dates: Vec<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GetAnalyticsStatusInput {
    pub scope: AnalyticsScopeDto,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GetPerformanceSummaryInput {
    pub scope: AnalyticsScopeDto,
    pub period: AnalyticsPeriodDto,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GetGainSummaryInput {
    pub scope: AnalyticsScopeDto,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GetNetWorthAttributionInput {
    pub scope: AnalyticsScopeDto,
    pub period: AnalyticsPeriodDto,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListHoldingLotsInput {
    pub scope: AnalyticsScopeDto,
    pub cursor: Option<String>,
    pub limit: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListUnknownBasisLotsInput {
    pub scope: AnalyticsScopeDto,
    pub cursor: Option<String>,
    pub limit: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListCostBasisDeclarationsInput {
    pub scope: AnalyticsScopeDto,
    pub cursor: Option<String>,
    pub limit: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeclareLotCostBasisInput {
    pub lot_ref: LotRefDto,
    pub instrument_id: String,
    pub declared_cost: String,
    pub declared_currency: String,
    pub acquired_on: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RevokeLotCostBasisInput {
    pub lot_ref: LotRefDto,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsStatusDto {
    pub usable_history: DateRangeAvailabilityDto,
    pub earliest_complete_snapshot_on: DateAvailabilityDto,
    pub blocking_dates: Vec<String>,
    pub unknown_basis_lot_count: i32,
    pub unknown_basis_value: MoneyAvailabilityDto,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GainSummaryIpcDto {
    pub realized_gross: SignedMoneyAvailabilityDto,
    pub realized_net: SignedMoneyAvailabilityDto,
    pub allocated_fees: SignedMoneyAvailabilityDto,
    pub unrealized_gross: SignedMoneyAvailabilityDto,
    pub unexplained_disposal: SignedMoneyAvailabilityDto,
    pub basis_complete: bool,
    pub input_complete: bool,
    pub decomposition_complete: bool,
    pub unknown_basis_quantity: String,
    pub unknown_basis_value: MoneyAvailabilityDto,
    pub instrument_movement: SignedMoneyAvailabilityDto,
    pub currency_movement: SignedMoneyAvailabilityDto,
    pub income: Vec<IncomeBucketDto>,
    pub fees: Vec<FeeBucketDto>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[allow(clippy::large_enum_variant)]
pub enum NetWorthAttributionIpcDto {
    #[serde(rename_all = "camelCase")]
    Available { value: AvailableAttributionDto },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        reason: String,
        blocking_dates: Vec<String>,
        unconvertible_flow_count: i32,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HoldingLotDto {
    pub lot_ref: LotRefDto,
    pub account_id: String,
    pub instrument_id: String,
    pub acquired_at: String,
    pub quantity_remaining: String,
    pub original_quantity: String,
    pub cost: MoneyAvailabilityDto,
    pub basis: String,
    pub is_declared: bool,
    pub current_value: MoneyAvailabilityDto,
    pub unrealized_gross: SignedMoneyAvailabilityDto,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HoldingLotPageDto {
    pub items: Vec<HoldingLotDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CostBasisDeclarationIpcDto {
    pub id: String,
    pub household_id: String,
    pub lot_ref: LotRefDto,
    pub instrument_id: String,
    pub declared_cost: Option<String>,
    pub declared_currency: Option<String>,
    pub acquired_on: Option<String>,
    pub revokes: Option<String>,
    pub is_revocation: bool,
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CostBasisDeclarationPageDto {
    pub items: Vec<CostBasisDeclarationIpcDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

pub async fn get_analytics_status(
    state: &AppState,
    input: GetAnalyticsStatusInput,
) -> Result<AnalyticsStatusDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_analytics_status_in_tx(&mut tx, input).await;
    finish_read_tx(tx, result).await
}

pub async fn get_performance_summary(
    state: &AppState,
    input: GetPerformanceSummaryInput,
) -> Result<PerformanceSummaryDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_performance_summary_in_tx(&mut tx, input).await;
    finish_read_tx(tx, result).await
}

pub async fn get_gain_summary(
    state: &AppState,
    input: GetGainSummaryInput,
) -> Result<GainSummaryIpcDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_gain_summary_in_tx(&mut tx, input).await;
    finish_read_tx(tx, result).await
}

pub async fn get_net_worth_attribution(
    state: &AppState,
    input: GetNetWorthAttributionInput,
) -> Result<NetWorthAttributionIpcDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_net_worth_attribution_in_tx(&mut tx, input).await;
    finish_read_tx(tx, result).await
}

pub async fn list_holding_lots(
    state: &AppState,
    input: ListHoldingLotsInput,
) -> Result<HoldingLotPageDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = list_holding_lots_in_tx(&mut tx, input, false).await;
    finish_read_tx(tx, result).await
}

pub async fn list_unknown_basis_lots(
    state: &AppState,
    input: ListUnknownBasisLotsInput,
) -> Result<HoldingLotPageDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = list_holding_lots_in_tx(
        &mut tx,
        ListHoldingLotsInput {
            scope: input.scope,
            cursor: input.cursor,
            limit: input.limit,
        },
        true,
    )
    .await;
    finish_read_tx(tx, result).await
}

pub async fn list_cost_basis_declarations(
    state: &AppState,
    input: ListCostBasisDeclarationsInput,
) -> Result<CostBasisDeclarationPageDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = list_cost_basis_declarations_in_tx(&mut tx, input).await;
    finish_read_tx(tx, result).await
}

pub async fn declare_lot_cost_basis(
    state: &AppState,
    input: DeclareLotCostBasisInput,
) -> Result<CostBasisDeclarationIpcDto, AppError> {
    let declared =
        cost_basis_service::declare_lot_cost_basis(state, to_declare_input(input)?).await?;
    ipc_declaration(declared)
}

pub async fn revoke_lot_cost_basis(
    state: &AppState,
    input: RevokeLotCostBasisInput,
) -> Result<CostBasisDeclarationIpcDto, AppError> {
    let keys = lot_ref_keys(parse_lot_ref_dto(&input.lot_ref)?)?;
    let revoked = cost_basis_service::revoke_lot_cost_basis(
        state,
        cost_basis_service::RevokeLotCostBasisInput {
            origin_holding_id: keys.0,
            activity_leg_id: keys.1,
        },
    )
    .await?;
    ipc_declaration(revoked)
}

async fn get_analytics_status_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: GetAnalyticsStatusInput,
) -> Result<AnalyticsStatusDto, AppError> {
    query_count::record("analytics_status");
    let household = require_household_tx(tx).await?;
    let scope = resolve_scope(tx, &household.id, &input.scope).await?;
    let origin = history_repositories::get_origin_by_household(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    let timezone = HistoryTimezone::parse(&origin.timezone)?;
    let origin_date = CalendarDate::parse(&origin.origin_local_date)?;
    let today = timezone.local_date(&Timestamp::now());
    let last_closed = today.pred();
    let earliest = history_repositories::earliest_complete_snapshot_on(tx, &household.id).await?;
    let range_end = last_closed.unwrap_or(origin_date);
    let snapshots = history_repositories::list_latest_snapshots_in_range(
        tx,
        &household.id,
        &origin_date.to_ymd(),
        &range_end.to_ymd(),
    )
    .await?;
    let complete: HashMap<String, bool> = snapshots
        .into_iter()
        .map(|snapshot| (snapshot.snapshot_on, snapshot.is_complete))
        .collect();
    let walk_start = earliest
        .as_deref()
        .map(CalendarDate::parse)
        .transpose()?
        .unwrap_or(origin_date);
    let mut blocking = Vec::new();
    if let Some(end) = last_closed {
        if walk_start <= end {
            for date in return_service::dates_inclusive(walk_start, end) {
                let key = date.to_ymd();
                match complete.get(&key) {
                    Some(true) => {}
                    _ => blocking.push(key),
                }
            }
        }
    }
    let earliest_complete_snapshot_on = match earliest.filter(|value| !value.is_empty()) {
        Some(value) => DateAvailabilityDto::Available { value },
        None => DateAvailabilityDto::Unavailable {
            reason: REASON_PERIOD_UNAVAILABLE.to_owned(),
            blocking_dates: blocking.clone(),
        },
    };
    let usable_history = match (
        &earliest_complete_snapshot_on,
        last_closed.map(CalendarDate::to_ymd),
    ) {
        (DateAvailabilityDto::Available { value: start }, Some(end))
            if start.as_str() <= end.as_str() =>
        {
            DateRangeAvailabilityDto::Available {
                start_local_date: start.clone(),
                end_local_date: end,
            }
        }
        _ => DateRangeAvailabilityDto::Unavailable {
            reason: REASON_PERIOD_UNAVAILABLE.to_owned(),
            blocking_dates: blocking.clone(),
        },
    };
    let ledger = cost_basis_service::load_effective_lot_ledger_in_tx(tx, &household.id).await?;
    let accounts = account_service::list_account_records_in_tx(tx, &household.id, true).await?;
    let snapshot = ValuationSnapshot::load(tx, &household.id, &household.base_currency).await?;
    let now = Timestamp::now();
    let gain = gain_service::summarize_gain(&ledger, &snapshot, &accounts, scope, &now)?;
    let unknown_basis_lot_count = i32::try_from(count_unknown_lots(&ledger, &accounts, scope)?)
        .map_err(|_| AppError::Internal)?;
    let unknown_basis_value = money_option_availability(
        gain.unknown_basis_value,
        gain.input_complete || gain.unknown_basis_quantity == "0",
    );
    tracing::info!(
        event = "analytics.status",
        scope_kind = scope_kind_label(&input.scope),
        unknown_lot_count = unknown_basis_lot_count,
        blocking_date_count = blocking.len() as i64,
        "analytics status loaded"
    );
    Ok(AnalyticsStatusDto {
        usable_history,
        earliest_complete_snapshot_on,
        blocking_dates: blocking,
        unknown_basis_lot_count,
        unknown_basis_value,
    })
}

async fn get_performance_summary_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: GetPerformanceSummaryInput,
) -> Result<PerformanceSummaryDto, AppError> {
    let resolved = resolve_scope_period(tx, &input.scope, &input.period).await?;
    tracing::info!(
        event = "analytics.performance",
        scope_kind = scope_kind_label(&input.scope),
        period_kind = period_kind_label(&input.period),
        "performance summary loaded"
    );
    return_service::get_performance_summary_in_tx(
        tx,
        resolved.scope,
        &resolved.start.to_ymd(),
        &resolved.end.to_ymd(),
    )
    .await
}

async fn get_gain_summary_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: GetGainSummaryInput,
) -> Result<GainSummaryIpcDto, AppError> {
    let household = require_household_tx(tx).await?;
    let scope = resolve_scope(tx, &household.id, &input.scope).await?;
    let gain = gain_service::get_gain_summary_in_tx(tx, scope).await?;
    let income_fees = income_fee_service::get_income_fee_totals_in_tx(tx, scope).await?;
    let decomposition = currency_decomposition::get_currency_decomposition_in_tx(tx, scope).await?;
    tracing::info!(
        event = "analytics.gain",
        scope_kind = scope_kind_label(&input.scope),
        "gain summary loaded"
    );
    Ok(GainSummaryIpcDto {
        realized_gross: signed_option_availability(gain.realized_gross, gain.input_complete),
        realized_net: signed_option_availability(gain.realized_net, gain.input_complete),
        allocated_fees: signed_option_availability(gain.allocated_fees, gain.input_complete),
        unrealized_gross: signed_option_availability(gain.unrealized_gross, gain.input_complete),
        unexplained_disposal: signed_option_availability(
            gain.unexplained_disposal_value,
            gain.input_complete,
        ),
        basis_complete: gain.basis_complete,
        input_complete: gain.input_complete,
        decomposition_complete: decomposition.basis_complete && decomposition.input_complete,
        unknown_basis_quantity: gain.unknown_basis_quantity,
        unknown_basis_value: money_option_availability(
            gain.unknown_basis_value,
            gain.input_complete,
        ),
        instrument_movement: signed_option_availability(
            decomposition.instrument_movement,
            decomposition.input_complete,
        ),
        currency_movement: signed_option_availability(
            decomposition.currency_movement,
            decomposition.input_complete,
        ),
        income: income_fees.income,
        fees: income_fees.fees,
    })
}

async fn get_net_worth_attribution_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: GetNetWorthAttributionInput,
) -> Result<NetWorthAttributionIpcDto, AppError> {
    let resolved = resolve_scope_period(tx, &input.scope, &input.period).await?;
    let result = attribution_service::get_net_worth_attribution_in_tx(
        tx,
        resolved.scope,
        &resolved.start.to_ymd(),
        &resolved.end.to_ymd(),
    )
    .await?;
    tracing::info!(
        event = "analytics.attribution",
        scope_kind = scope_kind_label(&input.scope),
        period_kind = period_kind_label(&input.period),
        "attribution loaded"
    );
    Ok(match result {
        NetWorthAttributionDto::Available(value) => NetWorthAttributionIpcDto::Available { value },
        NetWorthAttributionDto::Unavailable(value) => NetWorthAttributionIpcDto::Unavailable {
            reason: value.reason,
            blocking_dates: value.blocking_dates,
            unconvertible_flow_count: value.unconvertible_flow_count,
        },
    })
}

async fn list_holding_lots_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: ListHoldingLotsInput,
    unknown_only: bool,
) -> Result<HoldingLotPageDto, AppError> {
    query_count::record(if unknown_only {
        "unknown_basis_lots"
    } else {
        "holding_lots"
    });
    let household = require_household_tx(tx).await?;
    let scope = resolve_scope(tx, &household.id, &input.scope).await?;
    let limit = page_limit(input.limit)?;
    let cursor = input.cursor.as_deref().map(decode_lot_cursor).transpose()?;
    let ledger = cost_basis_service::load_effective_lot_ledger_in_tx(tx, &household.id).await?;
    let accounts = account_service::list_account_records_in_tx(tx, &household.id, true).await?;
    let snapshot = ValuationSnapshot::load(tx, &household.id, &household.base_currency).await?;
    let accounts_by_id: HashMap<&str, &AccountRecordDto> = accounts
        .iter()
        .map(|account| (account.id.as_str(), account))
        .collect();
    let mut lots = Vec::new();
    let mut seen_refs = HashSet::<(String, String)>::new();
    for lot in ledger.open_lots() {
        if unknown_only && lot.basis() != BasisStatus::Unknown {
            continue;
        }
        if !gain_service::in_scope(
            scope,
            lot.account_id(),
            lot.instrument_id(),
            &accounts_by_id,
        )? {
            continue;
        }
        let dto = lot_dto(&ledger, &snapshot, lot)?;
        let identity = (
            source_kind_dto_label(&dto.lot_ref.source_kind).to_owned(),
            dto.lot_ref.source_id.clone(),
        );
        if !seen_refs.insert(identity) {
            continue;
        }
        lots.push(dto);
    }
    lots.sort_by_key(lot_dto_sort_key);
    if let Some(cursor) = &cursor {
        lots.retain(|lot| lot_dto_sort_key(lot) > *cursor);
    }
    let has_more = lots.len() as i64 > limit;
    if has_more {
        lots.truncate(limit as usize);
    }
    let next_cursor = has_more
        .then(|| lots.last().map(encode_lot_cursor))
        .flatten();
    tracing::info!(
        event = "analytics.lots",
        scope_kind = scope_kind_label(&input.scope),
        lot_count = lots.len() as i64,
        "holding lots listed"
    );
    Ok(HoldingLotPageDto {
        items: lots,
        next_cursor,
        has_more,
    })
}

async fn list_cost_basis_declarations_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: ListCostBasisDeclarationsInput,
) -> Result<CostBasisDeclarationPageDto, AppError> {
    query_count::record("declaration_page");
    let household = require_household_tx(tx).await?;
    let scope = resolve_scope(tx, &household.id, &input.scope).await?;
    let limit = page_limit(input.limit)?;
    let cursor = input
        .cursor
        .as_deref()
        .map(decode_declaration_cursor)
        .transpose()?;
    let records =
        analytics_repositories::list_declarations_for_household(tx, &household.id).await?;
    let ledger = cost_basis_service::load_effective_lot_ledger_in_tx(tx, &household.id).await?;
    let accounts = account_service::list_account_records_in_tx(tx, &household.id, true).await?;
    let accounts_by_id: HashMap<&str, &AccountRecordDto> = accounts
        .iter()
        .map(|account| (account.id.as_str(), account))
        .collect();
    let mut items = Vec::new();
    for record in records {
        let dto = ipc_declaration_from_parts(
            record.id.clone(),
            record.household_id.clone(),
            record.origin_holding_id.clone(),
            record.activity_leg_id.clone(),
            record.instrument_id.clone(),
            record.declared_cost.clone(),
            record.declared_currency.clone(),
            record.acquired_on.clone(),
            record.revokes.clone(),
            record.is_revocation,
            record.note.clone(),
            record.created_at.clone(),
        )?;
        if !declaration_in_scope(scope, &ledger, &accounts_by_id, &dto)? {
            continue;
        }
        let key = (record.created_at.clone(), record.id.clone());
        if let Some(cursor) = &cursor {
            if key >= *cursor {
                continue;
            }
        }
        items.push(dto);
    }
    items.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    let has_more = items.len() as i64 > limit;
    if has_more {
        items.truncate(limit as usize);
    }
    let next_cursor = has_more
        .then(|| items.last().map(encode_declaration_cursor))
        .flatten();
    tracing::info!(
        event = "analytics.declarations",
        scope_kind = scope_kind_label(&input.scope),
        declaration_count = items.len() as i64,
        "cost-basis declarations listed"
    );
    Ok(CostBasisDeclarationPageDto {
        items,
        next_cursor,
        has_more,
    })
}

struct ResolvedPeriod {
    scope: AnalyticsScope,
    start: CalendarDate,
    end: CalendarDate,
}

async fn resolve_scope_period(
    tx: &mut Transaction<'_, Sqlite>,
    scope: &AnalyticsScopeDto,
    period: &AnalyticsPeriodDto,
) -> Result<ResolvedPeriod, AppError> {
    let household = require_household_tx(tx).await?;
    let scope = resolve_scope(tx, &household.id, scope).await?;
    let origin = history_repositories::get_origin_by_household(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    if !origin.timezone_confirmed {
        return Err(AppError::HistoryTimezoneConfirmationRequired);
    }
    let timezone = HistoryTimezone::parse(&origin.timezone)?;
    let origin_date = CalendarDate::parse(&origin.origin_local_date)?;
    let today = timezone.local_date(&Timestamp::now());
    let (start, end) = resolve_period(period, origin_date, today)?;
    Ok(ResolvedPeriod { scope, start, end })
}

async fn resolve_scope(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    scope: &AnalyticsScopeDto,
) -> Result<AnalyticsScope, AppError> {
    match scope {
        AnalyticsScopeDto::Household => Ok(AnalyticsScope::Household),
        AnalyticsScopeDto::Portfolio => Ok(AnalyticsScope::Portfolio),
        AnalyticsScopeDto::Account { account_id } => {
            let parsed = AccountId::parse(account_id.trim())?;
            if history_repositories::household_account_exists(tx, household_id, &parsed.to_string())
                .await?
            {
                Ok(AnalyticsScope::Account(parsed))
            } else {
                Err(AppError::not_found("account", account_id))
            }
        }
        AnalyticsScopeDto::Instrument { instrument_id } => {
            let parsed = InstrumentId::parse(instrument_id.trim())?;
            if history_repositories::household_instrument_exists(
                tx,
                household_id,
                &parsed.to_string(),
            )
            .await?
            {
                Ok(AnalyticsScope::Instrument(parsed))
            } else {
                Err(AppError::not_found("instrument", instrument_id))
            }
        }
    }
}

fn resolve_period(
    period: &AnalyticsPeriodDto,
    origin_date: CalendarDate,
    today: CalendarDate,
) -> Result<(CalendarDate, CalendarDate), AppError> {
    match period {
        AnalyticsPeriodDto::OneMonth => Ok((preset_start(30, origin_date, today), today)),
        AnalyticsPeriodDto::ThreeMonths => Ok((preset_start(90, origin_date, today), today)),
        AnalyticsPeriodDto::OneYear => Ok((preset_start(365, origin_date, today), today)),
        AnalyticsPeriodDto::All => Ok((origin_date, today)),
        AnalyticsPeriodDto::Custom {
            start_local_date,
            end_local_date,
        } => {
            let start = CalendarDate::parse(start_local_date)?;
            let end = CalendarDate::parse(end_local_date)?;
            if end < start {
                return Err(AppError::validation(
                    "endLocalDate",
                    "The period end must be on or after the period start.",
                ));
            }
            Ok((start, end))
        }
    }
}

fn preset_start(days: i64, origin_date: CalendarDate, today: CalendarDate) -> CalendarDate {
    today
        .checked_add_days(-days)
        .unwrap_or(origin_date)
        .max(origin_date)
}

fn page_limit(limit: Option<i32>) -> Result<i64, AppError> {
    let limit = i64::from(limit.unwrap_or(DEFAULT_PAGE_SIZE));
    if limit < 1 {
        return Err(AppError::validation(
            "limit",
            "Page size must be at least 1.",
        ));
    }
    if limit > i64::from(MAX_PAGE_SIZE) {
        return Err(AppError::validation(
            "limit",
            "Page size cannot exceed 100.",
        ));
    }
    Ok(limit)
}

fn parse_lot_ref_dto(value: &LotRefDto) -> Result<LotRef, AppError> {
    match value.source_kind {
        LotRefSourceKind::OriginHolding => {
            Ok(LotRef::OriginHolding(HoldingId::parse(&value.source_id)?))
        }
        LotRefSourceKind::Acquisition => {
            Ok(LotRef::Acquisition(ActivityLegId::parse(&value.source_id)?))
        }
    }
}

fn lot_ref_dto(lot_ref: LotRef) -> LotRefDto {
    match lot_ref {
        LotRef::OriginHolding(id) => LotRefDto {
            source_kind: LotRefSourceKind::OriginHolding,
            source_id: id.to_string(),
        },
        LotRef::Acquisition(id) => LotRefDto {
            source_kind: LotRefSourceKind::Acquisition,
            source_id: id.to_string(),
        },
    }
}

fn lot_ref_keys(lot_ref: LotRef) -> Result<(Option<String>, Option<String>), AppError> {
    Ok(match lot_ref {
        LotRef::OriginHolding(id) => (Some(id.to_string()), None),
        LotRef::Acquisition(id) => (None, Some(id.to_string())),
    })
}

fn to_declare_input(
    input: DeclareLotCostBasisInput,
) -> Result<cost_basis_service::DeclareLotCostBasisInput, AppError> {
    let keys = lot_ref_keys(parse_lot_ref_dto(&input.lot_ref)?)?;
    Ok(cost_basis_service::DeclareLotCostBasisInput {
        origin_holding_id: keys.0,
        activity_leg_id: keys.1,
        instrument_id: input.instrument_id,
        declared_cost: input.declared_cost,
        declared_currency: input.declared_currency,
        acquired_on: input.acquired_on,
        note: input.note,
    })
}

fn ipc_declaration(
    record: CostBasisDeclarationDto,
) -> Result<CostBasisDeclarationIpcDto, AppError> {
    ipc_declaration_from_parts(
        record.id,
        record.household_id,
        record.origin_holding_id,
        record.activity_leg_id,
        record.instrument_id,
        record.declared_cost,
        record.declared_currency,
        record.acquired_on,
        record.revokes,
        record.is_revocation,
        record.note,
        record.created_at,
    )
}

#[allow(clippy::too_many_arguments)]
fn ipc_declaration_from_parts(
    id: String,
    household_id: String,
    origin_holding_id: Option<String>,
    activity_leg_id: Option<String>,
    instrument_id: String,
    declared_cost: Option<String>,
    declared_currency: Option<String>,
    acquired_on: Option<String>,
    revokes: Option<String>,
    is_revocation: bool,
    note: Option<String>,
    created_at: String,
) -> Result<CostBasisDeclarationIpcDto, AppError> {
    let lot_ref = match (origin_holding_id.as_deref(), activity_leg_id.as_deref()) {
        (Some(origin), None) => LotRefDto {
            source_kind: LotRefSourceKind::OriginHolding,
            source_id: origin.to_owned(),
        },
        (None, Some(leg)) => LotRefDto {
            source_kind: LotRefSourceKind::Acquisition,
            source_id: leg.to_owned(),
        },
        _ => {
            return Err(AppError::invalid_cost_basis_declaration(
                "Declare exactly one of an origin holding or an activity leg.",
            ))
        }
    };
    Ok(CostBasisDeclarationIpcDto {
        id,
        household_id,
        lot_ref,
        instrument_id,
        declared_cost,
        declared_currency,
        acquired_on,
        revokes,
        is_revocation,
        note,
        created_at,
    })
}

fn declaration_in_scope(
    scope: AnalyticsScope,
    ledger: &crate::domain::LotLedger,
    accounts: &HashMap<&str, &AccountRecordDto>,
    dto: &CostBasisDeclarationIpcDto,
) -> Result<bool, AppError> {
    let lot_ref = parse_lot_ref_dto(&dto.lot_ref)?;
    let Some(opening) = ledger.opening(lot_ref) else {
        return Ok(false);
    };
    let account_id = ledger
        .open_lots()
        .iter()
        .find(|lot| lot.lot_ref() == lot_ref)
        .map(OpenLot::account_id)
        .or_else(|| {
            ledger
                .consumptions()
                .iter()
                .rev()
                .find(|consumption| consumption.lot_ref() == lot_ref)
                .map(|consumption| consumption.account_id())
        });
    let Some(account_id) = account_id else {
        return Ok(match scope {
            AnalyticsScope::Household => true,
            AnalyticsScope::Instrument(instrument_id) => opening.instrument_id() == instrument_id,
            AnalyticsScope::Portfolio | AnalyticsScope::Account(_) => false,
        });
    };
    gain_service::in_scope(scope, account_id, opening.instrument_id(), accounts)
}

fn count_unknown_lots(
    ledger: &crate::domain::LotLedger,
    accounts: &[AccountRecordDto],
    scope: AnalyticsScope,
) -> Result<i64, AppError> {
    let accounts_by_id: HashMap<&str, &AccountRecordDto> = accounts
        .iter()
        .map(|account| (account.id.as_str(), account))
        .collect();
    let mut count = 0_i64;
    for lot in ledger.open_lots() {
        if lot.basis() != BasisStatus::Unknown {
            continue;
        }
        if gain_service::in_scope(
            scope,
            lot.account_id(),
            lot.instrument_id(),
            &accounts_by_id,
        )? {
            count += 1;
        }
    }
    Ok(count)
}

fn lot_dto(
    ledger: &crate::domain::LotLedger,
    snapshot: &ValuationSnapshot,
    lot: &OpenLot,
) -> Result<HoldingLotDto, AppError> {
    let opening = ledger.opening(lot.lot_ref());
    let is_declared = opening.is_some_and(|opening| opening.is_declared());
    let basis = match lot.basis() {
        BasisStatus::Known => "known",
        BasisStatus::Unknown => "unknown",
    };
    let cost = match (lot.cost_remaining(), lot.cost_currency()) {
        (Some(amount), Some(currency)) => MoneyAvailabilityDto::Available {
            value: money_dto(amount, currency)?,
        },
        _ => MoneyAvailabilityDto::Unavailable {
            reason: REASON_UNKNOWN_BASIS.to_owned(),
            blocking_dates: Vec::new(),
        },
    };
    let current_native = gain_service::native_holding_value(
        snapshot,
        lot.instrument_id(),
        lot.quantity_remaining(),
    )?;
    let current_value = match current_native {
        Some(value) => MoneyAvailabilityDto::Available {
            value: MoneyDto {
                amount: value.canonical_amount(),
                currency: value.currency().as_str().to_owned(),
            },
        },
        None => MoneyAvailabilityDto::Unavailable {
            reason: REASON_INPUT_INCOMPLETE.to_owned(),
            blocking_dates: Vec::new(),
        },
    };
    let unrealized_gross = match (current_native, lot.cost_remaining()) {
        (Some(native), Some(cost)) => {
            let amount = crate::domain::checked_sub(native.amount(), cost)?;
            SignedMoneyAvailabilityDto::Available {
                value: signed_dto(amount, native.currency())?,
            }
        }
        (None, _) => SignedMoneyAvailabilityDto::Unavailable {
            reason: REASON_INPUT_INCOMPLETE.to_owned(),
            blocking_dates: Vec::new(),
        },
        (_, None) => SignedMoneyAvailabilityDto::Unavailable {
            reason: REASON_UNKNOWN_BASIS.to_owned(),
            blocking_dates: Vec::new(),
        },
    };
    Ok(HoldingLotDto {
        lot_ref: lot_ref_dto(lot.lot_ref()),
        account_id: lot.account_id().to_string(),
        instrument_id: lot.instrument_id().to_string(),
        acquired_at: lot.acquired_at().to_rfc3339(),
        quantity_remaining: lot.quantity_remaining().canonical(),
        original_quantity: lot.original_quantity().canonical(),
        cost,
        basis: basis.to_owned(),
        is_declared,
        current_value,
        unrealized_gross,
    })
}

fn lot_dto_sort_key(lot: &HoldingLotDto) -> (String, String, String, String) {
    (
        lot.acquired_at.clone(),
        source_kind_dto_label(&lot.lot_ref.source_kind).to_owned(),
        lot.lot_ref.source_id.clone(),
        lot.account_id.clone(),
    )
}

fn encode_lot_cursor(lot: &HoldingLotDto) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}\n{}\n{}\n{}",
        lot.acquired_at,
        source_kind_dto_label(&lot.lot_ref.source_kind),
        lot.lot_ref.source_id,
        lot.account_id
    ))
}

fn decode_lot_cursor(value: &str) -> Result<(String, String, String, String), AppError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| AppError::validation("cursor", "The lot cursor is invalid."))?;
    let text = String::from_utf8(decoded)
        .map_err(|_| AppError::validation("cursor", "The lot cursor is invalid."))?;
    let mut parts = text.splitn(4, '\n');
    let acquired_at = parts
        .next()
        .ok_or_else(|| AppError::validation("cursor", "The lot cursor is invalid."))?;
    let source_kind = parts
        .next()
        .ok_or_else(|| AppError::validation("cursor", "The lot cursor is invalid."))?;
    let source_id = parts
        .next()
        .ok_or_else(|| AppError::validation("cursor", "The lot cursor is invalid."))?;
    let account_id = parts
        .next()
        .ok_or_else(|| AppError::validation("cursor", "The lot cursor is invalid."))?;
    if source_kind != SOURCE_ORIGIN && source_kind != SOURCE_ACQUISITION {
        return Err(AppError::validation("cursor", "The lot cursor is invalid."));
    }
    Ok((
        acquired_at.to_owned(),
        source_kind.to_owned(),
        source_id.to_owned(),
        account_id.to_owned(),
    ))
}

fn encode_declaration_cursor(item: &CostBasisDeclarationIpcDto) -> String {
    URL_SAFE_NO_PAD.encode(format!("{}\n{}", item.created_at, item.id))
}

fn decode_declaration_cursor(value: &str) -> Result<(String, String), AppError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| AppError::validation("cursor", "The declaration cursor is invalid."))?;
    let text = String::from_utf8(decoded)
        .map_err(|_| AppError::validation("cursor", "The declaration cursor is invalid."))?;
    let mut parts = text.splitn(2, '\n');
    let created_at = parts
        .next()
        .ok_or_else(|| AppError::validation("cursor", "The declaration cursor is invalid."))?;
    let id = parts
        .next()
        .ok_or_else(|| AppError::validation("cursor", "The declaration cursor is invalid."))?;
    Ok((created_at.to_owned(), id.to_owned()))
}

fn source_kind_dto_label(kind: &LotRefSourceKind) -> &'static str {
    match kind {
        LotRefSourceKind::OriginHolding => SOURCE_ORIGIN,
        LotRefSourceKind::Acquisition => SOURCE_ACQUISITION,
    }
}

fn scope_kind_label(scope: &AnalyticsScopeDto) -> &'static str {
    match scope {
        AnalyticsScopeDto::Household => "household",
        AnalyticsScopeDto::Portfolio => "portfolio",
        AnalyticsScopeDto::Account { .. } => "account",
        AnalyticsScopeDto::Instrument { .. } => "instrument",
    }
}

fn period_kind_label(period: &AnalyticsPeriodDto) -> &'static str {
    match period {
        AnalyticsPeriodDto::OneMonth => "oneMonth",
        AnalyticsPeriodDto::ThreeMonths => "threeMonths",
        AnalyticsPeriodDto::OneYear => "oneYear",
        AnalyticsPeriodDto::All => "all",
        AnalyticsPeriodDto::Custom { .. } => "custom",
    }
}

fn signed_option_availability(
    value: Option<SignedMoneyDto>,
    input_complete: bool,
) -> SignedMoneyAvailabilityDto {
    match value {
        Some(value) => SignedMoneyAvailabilityDto::Available { value },
        None if input_complete => SignedMoneyAvailabilityDto::Unavailable {
            reason: REASON_UNKNOWN_BASIS.to_owned(),
            blocking_dates: Vec::new(),
        },
        None => SignedMoneyAvailabilityDto::Unavailable {
            reason: REASON_INPUT_INCOMPLETE.to_owned(),
            blocking_dates: Vec::new(),
        },
    }
}

fn money_option_availability(value: Option<MoneyDto>, complete: bool) -> MoneyAvailabilityDto {
    match value {
        Some(value) => MoneyAvailabilityDto::Available { value },
        None if complete => MoneyAvailabilityDto::Unavailable {
            reason: REASON_UNKNOWN_BASIS.to_owned(),
            blocking_dates: Vec::new(),
        },
        None => MoneyAvailabilityDto::Unavailable {
            reason: REASON_INPUT_INCOMPLETE.to_owned(),
            blocking_dates: Vec::new(),
        },
    }
}

fn money_dto(
    amount: rust_decimal::Decimal,
    currency: crate::domain::CurrencyCode,
) -> Result<MoneyDto, AppError> {
    let money = Money::from_canonical(round_to_money_scale(amount)?, currency)?;
    Ok(MoneyDto {
        amount: money.canonical_amount(),
        currency: money.currency().as_str().to_owned(),
    })
}

fn signed_dto(
    amount: rust_decimal::Decimal,
    currency: crate::domain::CurrencyCode,
) -> Result<SignedMoneyDto, AppError> {
    let value = SignedMoney::from_canonical(amount, currency)?;
    Ok(SignedMoneyDto {
        amount: value.canonical_amount(),
        currency: value.currency().as_str().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            analytics_repositories::CostBasisDeclarationRecord,
            reference::{begin_write_tx, require_household_id_tx},
        },
        error::{AppError, ErrorCode},
        infrastructure::{database::connect_writable, database_bootstrap::MIGRATOR},
        test_support::{cleanup, test_path, UNKNOWN_UUID},
    };
    use std::{collections::HashSet, fs, path::PathBuf};

    const ORIGIN_QQQ_HOLDING: &str = "30303030-3030-4303-8303-303030303030";
    const QQQ: &str = "20202020-2020-4202-8202-202020202020";
    const BROKERAGE: &str = "99999999-9999-4999-8999-999999999999";
    const OPENING_XLF_LEG: &str = "01a0188f-862c-7c93-9999-26697ff52022";

    fn household_scope() -> AnalyticsScopeDto {
        AnalyticsScopeDto::Household
    }

    fn custom_period(start: &str, end: &str) -> AnalyticsPeriodDto {
        AnalyticsPeriodDto::Custom {
            start_local_date: start.to_owned(),
            end_local_date: end.to_owned(),
        }
    }

    async fn apply_migrations(path: &std::path::Path, versions: &[i64]) {
        let pool = connect_writable(path, true)
            .await
            .expect("fixture database should open");
        for version in versions {
            let migration = MIGRATOR
                .iter()
                .find(|item| item.version == *version)
                .unwrap_or_else(|| panic!("migration {version} should exist"))
                .clone();
            let mut conn = pool.acquire().await.expect("connection");
            sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
                .await
                .expect("migration metadata table should be created");
            sqlx::migrate::Migrate::apply(&mut *conn, &migration)
                .await
                .expect("released schema should apply");
        }
        pool.close().await;
    }

    async fn load_sql(path: &std::path::Path, sql: &str) {
        let pool = connect_writable(path, false)
            .await
            .expect("fixture database should open");
        sqlx::raw_sql(sql)
            .execute(&pool)
            .await
            .expect("fixture should load");
        pool.close().await;
    }

    async fn load_v013(name: &str) -> (AppState, PathBuf) {
        let path = test_path("v014-p7", name);
        let _ = fs::remove_file(&path);
        apply_migrations(&path, &[1, 2, 3]).await;
        load_sql(&path, include_str!("../../test-fixtures/v0.1.3.sql")).await;
        let state = AppState::initialize(path.clone()).await;
        (state, path)
    }

    async fn scalar_i64(state: &AppState, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(state.writable_db().expect("writable"))
            .await
            .expect("scalar")
    }

    async fn table_fingerprint(state: &AppState) -> String {
        let db = state.writable_db().expect("writable");
        let mut parts = Vec::new();
        for (label, sql) in [
            (
                "activities",
                "SELECT COUNT(*) || ':' || COALESCE(GROUP_CONCAT(id, ','), '') FROM (SELECT id FROM activities ORDER BY id)",
            ),
            (
                "legs",
                "SELECT COUNT(*) || ':' || COALESCE(GROUP_CONCAT(id, ','), '') FROM (SELECT id FROM activity_legs ORDER BY id)",
            ),
            (
                "declarations",
                "SELECT COUNT(*) || ':' || COALESCE(GROUP_CONCAT(id, ','), '') FROM (SELECT id FROM cost_basis_declarations ORDER BY id)",
            ),
            (
                "snapshots",
                "SELECT COUNT(*) || ':' || COALESCE(GROUP_CONCAT(id || ':' || CAST(revision AS TEXT), ','), '') FROM (SELECT id, revision FROM daily_valuation_snapshots ORDER BY id)",
            ),
            (
                "dirty",
                "SELECT COALESCE(GROUP_CONCAT(household_id || ':' || IFNULL(dirty_from, '') || ':' || rebuild_status, ','), '') FROM history_snapshot_state",
            ),
            (
                "holdings",
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || quantity, ','), '') FROM (SELECT id, quantity FROM holdings ORDER BY id)",
            ),
            (
                "account_values",
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || amount || ':' || IFNULL(activity_id, ''), ','), '') FROM (SELECT id, amount, activity_id FROM account_values ORDER BY id)",
            ),
        ] {
            let value: String = sqlx::query_scalar(sql)
                .fetch_one(db)
                .await
                .unwrap_or_else(|_| panic!("{label} fingerprint"));
            parts.push(format!("{label}={value}"));
        }
        parts.join("|")
    }

    fn assert_availability_not_zero_null_or_empty(value: &serde_json::Value, path: &str) {
        let kind = value["kind"]
            .as_str()
            .unwrap_or_else(|| panic!("{path} should be tagged"));
        match kind {
            "available" => {
                if let Some(inner) = value.get("value") {
                    assert!(!inner.is_null(), "{path} available value must not be null");
                    if let Some(text) = inner.as_str() {
                        assert!(!text.is_empty(), "{path} available value must not be empty");
                    }
                    if let Some(amount) = inner.get("amount") {
                        assert!(
                            amount.as_str().is_some_and(|text| !text.is_empty()),
                            "{path} available amount must not be empty"
                        );
                    }
                }
            }
            "unavailable" => {
                assert!(
                    value.get("amount").is_none() && value.get("cumulative").is_none(),
                    "{path} unavailable must not encode a result"
                );
                let reason = value["reason"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{path} unavailable reason"));
                assert!(
                    !reason.is_empty(),
                    "{path} unavailable reason must not be empty"
                );
            }
            other => panic!("{path} unexpected kind {other}"),
        }
    }

    fn assert_error_shielded(error: AppError) {
        let command = error.into_command_error();
        let payload = serde_json::to_string(&command).expect("json");
        for needle in [
            "SELECT ",
            " FROM ",
            ".db",
            "/tmp/",
            "QQQ",
            "note text",
            "1500.25",
            "0.0404",
            "cost=",
            "gain=",
            "rate=",
            "quantity=",
            "symbol=",
        ] {
            assert!(
                !payload.contains(needle),
                "command error leaked {needle}: {payload}"
            );
        }
    }

    fn family_count(families: &[&'static str], name: &str) -> usize {
        families.iter().filter(|family| **family == name).count()
    }

    fn assert_bounded_families(families: &[&'static str]) {
        let mut counts = HashMap::new();
        for family in families {
            *counts.entry(*family).or_insert(0_usize) += 1;
        }
        for (family, count) in counts {
            assert!(
                count <= 4,
                "query family {family} ran {count} times: {families:?}"
            );
        }
        assert!(
            family_count(families, "snapshot_items") <= 2,
            "snapshot items queried too often: {families:?}"
        );
        assert!(
            family_count(families, "activity_legs") <= 4,
            "activity legs queried too often: {families:?}"
        );
        assert!(
            family_count(families, "activity_headers") <= 4,
            "activity headers queried too often: {families:?}"
        );
    }

    #[test]
    fn read_commands_write_nothing() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("read-only").await;
            let before = table_fingerprint(&state).await;
            get_analytics_status(
                &state,
                GetAnalyticsStatusInput {
                    scope: household_scope(),
                },
            )
            .await
            .expect("status");
            get_performance_summary(
                &state,
                GetPerformanceSummaryInput {
                    scope: household_scope(),
                    period: custom_period("2026-01-02", "2026-01-04"),
                },
            )
            .await
            .expect("performance");
            get_gain_summary(
                &state,
                GetGainSummaryInput {
                    scope: household_scope(),
                },
            )
            .await
            .expect("gain");
            get_net_worth_attribution(
                &state,
                GetNetWorthAttributionInput {
                    scope: household_scope(),
                    period: custom_period("2026-01-02", "2026-01-04"),
                },
            )
            .await
            .expect("attribution");
            list_holding_lots(
                &state,
                ListHoldingLotsInput {
                    scope: household_scope(),
                    cursor: None,
                    limit: Some(50),
                },
            )
            .await
            .expect("lots");
            list_unknown_basis_lots(
                &state,
                ListUnknownBasisLotsInput {
                    scope: household_scope(),
                    cursor: None,
                    limit: Some(50),
                },
            )
            .await
            .expect("unknown lots");
            list_cost_basis_declarations(
                &state,
                ListCostBasisDeclarationsInput {
                    scope: household_scope(),
                    cursor: None,
                    limit: Some(50),
                },
            )
            .await
            .expect("declarations");
            assert_eq!(table_fingerprint(&state).await, before);
            cleanup(&path);
        });
    }

    #[test]
    fn availability_unions_never_use_zero_null_or_empty() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("availability").await;
            let status = get_analytics_status(
                &state,
                GetAnalyticsStatusInput {
                    scope: household_scope(),
                },
            )
            .await
            .expect("status");
            let status_json = serde_json::to_value(&status).expect("status json");
            assert_availability_not_zero_null_or_empty(
                &status_json["usableHistory"],
                "usableHistory",
            );
            assert_availability_not_zero_null_or_empty(
                &status_json["earliestCompleteSnapshotOn"],
                "earliestCompleteSnapshotOn",
            );
            assert_availability_not_zero_null_or_empty(
                &status_json["unknownBasisValue"],
                "unknownBasisValue",
            );
            let performance = get_performance_summary(
                &state,
                GetPerformanceSummaryInput {
                    scope: household_scope(),
                    period: custom_period("2026-01-02", "2026-01-04"),
                },
            )
            .await
            .expect("performance");
            let performance_json = serde_json::to_value(&performance).expect("performance json");
            assert_availability_not_zero_null_or_empty(&performance_json["twr"], "twr");
            assert_availability_not_zero_null_or_empty(&performance_json["xirr"], "xirr");
            match performance.twr {
                return_service::TwrResultDto::Unavailable { reason, .. } => {
                    assert_eq!(reason, REASON_PERIOD_UNAVAILABLE);
                }
                return_service::TwrResultDto::Available { cumulative, .. } => {
                    assert_ne!(cumulative, "0");
                    assert!(!cumulative.is_empty());
                }
            }
            let lots = list_unknown_basis_lots(
                &state,
                ListUnknownBasisLotsInput {
                    scope: household_scope(),
                    cursor: None,
                    limit: Some(50),
                },
            )
            .await
            .expect("unknown lots");
            assert!(!lots.items.is_empty());
            for lot in &lots.items {
                let cost = serde_json::to_value(&lot.cost).expect("cost json");
                assert_availability_not_zero_null_or_empty(&cost, "lot.cost");
                assert!(
                    !matches!(
                        lot.cost,
                        MoneyAvailabilityDto::Available { ref value } if value.amount == "0"
                            || value.amount.is_empty()
                    ),
                    "unknown-basis cost must not be encoded as zero"
                );
            }
            cleanup(&path);
        });
    }

    #[test]
    fn scope_period_and_page_size_are_bounded() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("bounds").await;
            let missing_account = get_gain_summary(
                &state,
                GetGainSummaryInput {
                    scope: AnalyticsScopeDto::Account {
                        account_id: UNKNOWN_UUID.to_owned(),
                    },
                },
            )
            .await
            .expect_err("missing account");
            assert!(matches!(missing_account, AppError::NotFound { .. }));
            let missing_instrument = get_gain_summary(
                &state,
                GetGainSummaryInput {
                    scope: AnalyticsScopeDto::Instrument {
                        instrument_id: UNKNOWN_UUID.to_owned(),
                    },
                },
            )
            .await
            .expect_err("missing instrument");
            assert!(matches!(missing_instrument, AppError::NotFound { .. }));
            let inverted = get_performance_summary(
                &state,
                GetPerformanceSummaryInput {
                    scope: household_scope(),
                    period: custom_period("2026-01-04", "2026-01-02"),
                },
            )
            .await
            .expect_err("inverted period");
            assert!(matches!(inverted, AppError::Validation { .. }));
            let oversized = list_holding_lots(
                &state,
                ListHoldingLotsInput {
                    scope: household_scope(),
                    cursor: None,
                    limit: Some(101),
                },
            )
            .await
            .expect_err("page bound");
            assert!(matches!(oversized, AppError::Validation { field, .. } if field == "limit"));
            get_gain_summary(
                &state,
                GetGainSummaryInput {
                    scope: AnalyticsScopeDto::Account {
                        account_id: BROKERAGE.to_owned(),
                    },
                },
            )
            .await
            .expect("same household account");
            get_gain_summary(
                &state,
                GetGainSummaryInput {
                    scope: AnalyticsScopeDto::Instrument {
                        instrument_id: QQQ.to_owned(),
                    },
                },
            )
            .await
            .expect("same household instrument");
            cleanup(&path);
        });
    }

    #[test]
    fn lot_and_declaration_cursors_have_no_gaps_or_duplicates() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("cursors").await;
            let database = state.writable_db().expect("writable");
            let mut tx = begin_write_tx(database).await.expect("tx");
            let household_id = require_household_id_tx(&mut tx).await.expect("household");
            let created_at = "2026-08-19T00:00:00.000Z";
            for id in [
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2",
            ] {
                analytics_repositories::insert_declaration(
                    &mut tx,
                    &CostBasisDeclarationRecord {
                        id: id.to_owned(),
                        household_id: household_id.clone(),
                        origin_holding_id: Some(ORIGIN_QQQ_HOLDING.to_owned()),
                        activity_leg_id: None,
                        instrument_id: QQQ.to_owned(),
                        declared_cost: Some("1500".to_owned()),
                        declared_currency: Some("USD".to_owned()),
                        acquired_on: None,
                        revokes: None,
                        is_revocation: false,
                        note: None,
                        created_at: created_at.to_owned(),
                    },
                )
                .await
                .expect("insert");
            }
            let _ = tx.commit().await;

            let mut seen_lots = HashSet::new();
            let mut cursor = None;
            loop {
                let page = list_unknown_basis_lots(
                    &state,
                    ListUnknownBasisLotsInput {
                        scope: household_scope(),
                        cursor: cursor.clone(),
                        limit: Some(1),
                    },
                )
                .await
                .expect("lot page");
                for item in &page.items {
                    let key = format!(
                        "{}:{}",
                        source_kind_dto_label(&item.lot_ref.source_kind),
                        item.lot_ref.source_id
                    );
                    assert!(seen_lots.insert(key.clone()), "duplicate lot {key}");
                }
                if !page.has_more {
                    break;
                }
                cursor = page.next_cursor;
                assert!(cursor.is_some(), "has_more requires a next cursor");
            }
            let all_lots = list_unknown_basis_lots(
                &state,
                ListUnknownBasisLotsInput {
                    scope: household_scope(),
                    cursor: None,
                    limit: Some(100),
                },
            )
            .await
            .expect("all lots");
            let all_keys: HashSet<_> = all_lots
                .items
                .iter()
                .map(|item| {
                    format!(
                        "{}:{}",
                        source_kind_dto_label(&item.lot_ref.source_kind),
                        item.lot_ref.source_id
                    )
                })
                .collect();
            assert_eq!(seen_lots, all_keys);

            let mut seen_declarations = HashSet::new();
            let mut cursor = None;
            loop {
                let page = list_cost_basis_declarations(
                    &state,
                    ListCostBasisDeclarationsInput {
                        scope: household_scope(),
                        cursor,
                        limit: Some(1),
                    },
                )
                .await
                .expect("declaration page");
                for item in &page.items {
                    assert!(
                        seen_declarations.insert(item.id.clone()),
                        "duplicate declaration {}",
                        item.id
                    );
                }
                if !page.has_more {
                    break;
                }
                cursor = page.next_cursor;
            }
            assert_eq!(seen_declarations.len(), 2);
            cleanup(&path);
        });
    }

    #[test]
    fn query_counts_are_bounded_for_status_gain_return_attribution_and_lists() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("query-count").await;
            let (status, status_families) = query_count::capture_async(|| {
                get_analytics_status(
                    &state,
                    GetAnalyticsStatusInput {
                        scope: household_scope(),
                    },
                )
            })
            .await;
            status.expect("status");
            assert_eq!(family_count(&status_families, "snapshots_range"), 1);
            assert_eq!(
                family_count(&status_families, "snapshots_first_complete"),
                1
            );
            assert_bounded_families(&status_families);

            let (gain, gain_families) = query_count::capture_async(|| {
                get_gain_summary(
                    &state,
                    GetGainSummaryInput {
                        scope: household_scope(),
                    },
                )
            })
            .await;
            gain.expect("gain");
            assert!(family_count(&gain_families, "gain_summary") >= 1);
            assert_bounded_families(&gain_families);

            let (performance, return_families) = query_count::capture_async(|| {
                get_performance_summary(
                    &state,
                    GetPerformanceSummaryInput {
                        scope: household_scope(),
                        period: custom_period("2026-01-02", "2026-01-04"),
                    },
                )
            })
            .await;
            performance.expect("performance");
            assert_eq!(family_count(&return_families, "snapshot_items"), 1);
            assert_bounded_families(&return_families);

            let (attribution, attribution_families) = query_count::capture_async(|| {
                get_net_worth_attribution(
                    &state,
                    GetNetWorthAttributionInput {
                        scope: household_scope(),
                        period: custom_period("2026-01-02", "2026-01-04"),
                    },
                )
            })
            .await;
            attribution.expect("attribution");
            assert_eq!(family_count(&attribution_families, "snapshot_items"), 1);
            assert_bounded_families(&attribution_families);

            let (lots, lot_families) = query_count::capture_async(|| {
                list_holding_lots(
                    &state,
                    ListHoldingLotsInput {
                        scope: household_scope(),
                        cursor: None,
                        limit: Some(50),
                    },
                )
            })
            .await;
            lots.expect("lots");
            assert_eq!(family_count(&lot_families, "activity_legs"), 1);
            assert_bounded_families(&lot_families);

            let (declarations, declaration_families) = query_count::capture_async(|| {
                list_cost_basis_declarations(
                    &state,
                    ListCostBasisDeclarationsInput {
                        scope: household_scope(),
                        cursor: None,
                        limit: Some(50),
                    },
                )
            })
            .await;
            declarations.expect("declarations");
            assert!(
                family_count(
                    &declaration_families,
                    "cost_basis_declaration_list_household"
                ) <= 2
                    && family_count(
                        &declaration_families,
                        "cost_basis_declaration_list_household"
                    ) >= 1,
                "{declaration_families:?}"
            );
            assert_bounded_families(&declaration_families);
            cleanup(&path);
        });
    }

    #[test]
    fn new_errors_expose_no_sql_or_financial_details() {
        assert_error_shielded(AppError::AnalyticsPeriodUnavailable {
            reason: "SELECT cost, gain, rate FROM lots WHERE symbol = 'QQQ'".to_owned(),
            blocking_dates: vec!["/tmp/nestworth.db".to_owned()],
        });
        assert_error_shielded(AppError::analytics_input_incomplete(
            "quantity=3 note text path=/tmp/db",
        ));
        assert_error_shielded(AppError::ReturnNotComputable {
            reason: "rate=0.0404 cost=1500.25".to_owned(),
        });
        assert_error_shielded(AppError::invalid_cost_basis_declaration(
            "The declared instrument does not match this lot.",
        ));
        assert_error_shielded(AppError::CostBasisLotNotFound);
        assert_eq!(
            AppError::invalid_cost_basis_declaration("x")
                .into_command_error()
                .code,
            ErrorCode::InvalidCostBasisDeclaration
        );
        assert_eq!(
            AppError::CostBasisLotNotFound.into_command_error().code,
            ErrorCode::CostBasisLotNotFound
        );
        assert_eq!(
            AppError::AnalyticsPeriodUnavailable {
                reason: "x".to_owned(),
                blocking_dates: Vec::new(),
            }
            .into_command_error()
            .code,
            ErrorCode::AnalyticsPeriodUnavailable
        );
        assert_eq!(
            AppError::ReturnNotComputable {
                reason: "x".to_owned(),
            }
            .into_command_error()
            .code,
            ErrorCode::ReturnNotComputable
        );
        assert_eq!(
            AppError::analytics_input_incomplete("x")
                .into_command_error()
                .code,
            ErrorCode::AnalyticsInputIncomplete
        );
    }

    #[test]
    fn declare_and_revoke_are_the_only_writes_and_skip_ledger() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("declare-ipc").await;
            let before = table_fingerprint(&state).await;
            let declared = declare_lot_cost_basis(
                &state,
                DeclareLotCostBasisInput {
                    lot_ref: LotRefDto {
                        source_kind: LotRefSourceKind::OriginHolding,
                        source_id: ORIGIN_QQQ_HOLDING.to_owned(),
                    },
                    instrument_id: QQQ.to_owned(),
                    declared_cost: "1500".to_owned(),
                    declared_currency: "USD".to_owned(),
                    acquired_on: None,
                    note: None,
                },
            )
            .await
            .expect("declare");
            let after_declare = table_fingerprint(&state).await;
            assert_ne!(after_declare, before);
            assert!(after_declare.contains("declarations=1:"));
            assert_eq!(
                scalar_i64(&state, "SELECT COUNT(*) FROM activities").await,
                scalar_i64_from_fingerprint(&before, "activities")
            );
            revoke_lot_cost_basis(
                &state,
                RevokeLotCostBasisInput {
                    lot_ref: LotRefDto {
                        source_kind: LotRefSourceKind::OriginHolding,
                        source_id: ORIGIN_QQQ_HOLDING.to_owned(),
                    },
                },
            )
            .await
            .expect("revoke");
            assert_eq!(
                scalar_i64(&state, "SELECT COUNT(*) FROM cost_basis_declarations").await,
                2
            );
            assert_eq!(declared.lot_ref.source_id, ORIGIN_QQQ_HOLDING);
            let acquisition = declare_lot_cost_basis(
                &state,
                DeclareLotCostBasisInput {
                    lot_ref: LotRefDto {
                        source_kind: LotRefSourceKind::Acquisition,
                        source_id: OPENING_XLF_LEG.to_owned(),
                    },
                    instrument_id: "27272727-2727-4272-8272-272727272727".to_owned(),
                    declared_cost: "0".to_owned(),
                    declared_currency: "USD".to_owned(),
                    acquired_on: None,
                    note: None,
                },
            )
            .await
            .expect("opening lot");
            assert_eq!(acquisition.declared_cost.as_deref(), Some("0"));
            cleanup(&path);
        });
    }

    fn scalar_i64_from_fingerprint(fingerprint: &str, label: &str) -> i64 {
        fingerprint
            .split('|')
            .find_map(|part| {
                part.strip_prefix(&format!("{label}="))
                    .and_then(|value| value.split(':').next())
                    .and_then(|count| count.parse().ok())
            })
            .unwrap_or(0)
    }
}
