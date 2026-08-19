//! Net-worth attribution bridge for a closed snapshot period.
//!
//! Named components come from the ledger and daily decomposition. `unexplained`
//! is the residual and is never folded into a named bucket. Read-only. One
//! consistent transaction.

use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;
use serde::Serialize;
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    fx_conversion::{self, ConversionSpread},
    gain_service::SignedMoneyDto,
    history_repositories::DailyValuationSnapshotItemRecord,
    query_count,
    quote_service::{self, InstrumentQuoteRecordDto},
    reference::{begin_read_tx, finish_read_tx},
    return_service::{
        self, dates_inclusive, endpoint_facts, excluded_activity_ids, load_analytics_period_series,
        monetary_base, parse_decimal, scope_value, signed_amount, AccountMembership,
        AnalyticsPeriodSeries,
    },
};
use crate::{
    domain::{
        checked_add, checked_div, checked_mul, checked_sub, classify, classify_scope_flow,
        endpoint_in_scope, AnalyticsScope, CalendarDate, Classification, ComponentKind,
        CurrencyCode, LegComponent, LegFlowClassification, ScopeFlowActivity, ScopeFlowLeg,
        SignedMoney, Timestamp, UnitPrice,
    },
    error::AppError,
    state::AppState,
};

pub const METHOD_NOTE: &str = "Daily holding quantity change is valued at the previous close. The difference between a trade's execution price and that previous close is unexplained, not instrument movement. Monetary remeasurement is unexplained. Quantity remeasurement is unknown-basis flow. Flows are treated as available at the start of their local day.";
pub const REASON_PERIOD_UNAVAILABLE: &str = return_service::REASON_PERIOD_UNAVAILABLE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionComponents {
    pub external_contributions: Decimal,
    pub external_withdrawals: Decimal,
    pub instrument_movement: Decimal,
    pub currency_movement: Decimal,
    pub income: Decimal,
    pub fees: Decimal,
    pub debt_principal_movement: Decimal,
    pub conversion_spread: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonetaryDayDecomposition {
    pub delta_base: Decimal,
    pub flow_base: Decimal,
    pub currency: Decimal,
    pub market: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldingDayDecomposition {
    pub quantity: Decimal,
    pub price: Decimal,
    pub currency: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AvailableAttributionDto {
    pub start_on: String,
    pub end_on: String,
    pub start_net_worth: SignedMoneyDto,
    pub end_net_worth: SignedMoneyDto,
    pub delta: SignedMoneyDto,
    pub external_contributions: SignedMoneyDto,
    pub external_withdrawals: SignedMoneyDto,
    pub instrument_movement: SignedMoneyDto,
    pub currency_movement: SignedMoneyDto,
    pub income: SignedMoneyDto,
    pub fees: SignedMoneyDto,
    pub debt_principal_movement: SignedMoneyDto,
    pub conversion_spread: SignedMoneyDto,
    pub unexplained: SignedMoneyDto,
    pub unknown_basis_flow: SignedMoneyDto,
    pub basis_complete: bool,
    pub method_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UnavailableAttributionDto {
    pub reason: String,
    pub blocking_dates: Vec<String>,
    pub unconvertible_flow_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum NetWorthAttributionDto {
    Available(AvailableAttributionDto),
    Unavailable(UnavailableAttributionDto),
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ComponentKey {
    account_id: String,
    holding_id: Option<String>,
    instrument_id: Option<String>,
    kind: String,
    currency: String,
}

struct DayNativeFlow {
    native: Decimal,
    remeasurement: bool,
    unknown_basis: bool,
}

pub async fn get_net_worth_attribution(
    state: &AppState,
    scope: AnalyticsScope,
    start_on: &str,
    end_on: &str,
) -> Result<NetWorthAttributionDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_net_worth_attribution_in_tx(&mut tx, scope, start_on, end_on).await;
    finish_read_tx(tx, result).await
}

pub async fn get_net_worth_attribution_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    scope: AnalyticsScope,
    start_on: &str,
    end_on: &str,
) -> Result<NetWorthAttributionDto, AppError> {
    get_net_worth_attribution_at_in_tx(tx, scope, start_on, end_on, &Timestamp::now()).await
}

pub(crate) async fn get_net_worth_attribution_at_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    scope: AnalyticsScope,
    start_on: &str,
    end_on: &str,
    now: &Timestamp,
) -> Result<NetWorthAttributionDto, AppError> {
    query_count::record("attribution");
    let start_on = CalendarDate::parse(start_on)?;
    let end_on = CalendarDate::parse(end_on)?;
    if end_on < start_on {
        return Err(AppError::validation(
            "endOn",
            "The period end must be on or after the period start.",
        ));
    }
    let Some(series) = load_analytics_period_series(tx, start_on, end_on, now).await? else {
        return Ok(unavailable(Vec::new(), 0));
    };
    let t0 = series.start_on.to_ymd();
    let t1 = series.t1.to_ymd();
    let mut blocking = Vec::new();
    for date in [&t0, &t1] {
        match series.snapshots_by_date.get(date.as_str()) {
            Some(snapshot) if snapshot.is_complete => {}
            _ => blocking.push(date.clone()),
        }
    }
    blocking.sort();
    blocking.dedup();
    if !blocking.is_empty() {
        return Ok(unavailable(blocking, 0));
    }

    let quotes = quote_service::list_all_instrument_quotes(tx, &series.household_id).await?;
    let quotes_by_id: HashMap<String, InstrumentQuoteRecordDto> = quotes
        .into_iter()
        .map(|quote| (quote.id.clone(), quote))
        .collect();

    match build_bridge(scope, &series, &quotes_by_id)? {
        BridgeBuild::Unavailable {
            blocking_dates,
            unconvertible_flow_count,
        } => Ok(unavailable(blocking_dates, unconvertible_flow_count)),
        BridgeBuild::Available {
            start_value,
            end_value,
            parts,
            unexplained,
            unknown_basis_flow,
            basis_complete,
        } => Ok(NetWorthAttributionDto::Available(available_dto(
            series.start_on,
            series.t1,
            series.base,
            start_value,
            end_value,
            &parts,
            unexplained,
            unknown_basis_flow,
            basis_complete,
        )?)),
    }
}

/// Residual identity: `unexplained = ΔNW − (sum of named components)`.
/// Fees are positive costs and are subtracted. Never redistribute the residual.
pub fn unexplained_residual(
    delta: Decimal,
    parts: &AttributionComponents,
) -> Result<Decimal, AppError> {
    checked_sub(delta, named_component_sum(parts)?)
}

pub fn named_component_sum(parts: &AttributionComponents) -> Result<Decimal, AppError> {
    let without_fees = [
        parts.external_contributions,
        parts.external_withdrawals,
        parts.instrument_movement,
        parts.currency_movement,
        parts.income,
        parts.debt_principal_movement,
        parts.conversion_spread,
    ]
    .into_iter()
    .try_fold(Decimal::ZERO, checked_add)?;
    checked_sub(without_fees, parts.fees)
}

pub fn decompose_monetary_day(
    native_prev: Decimal,
    fx_prev: Decimal,
    native: Decimal,
    fx: Decimal,
    native_flow: Decimal,
) -> Result<MonetaryDayDecomposition, AppError> {
    let delta_base = checked_sub(checked_mul(native, fx)?, checked_mul(native_prev, fx_prev)?)?;
    let flow_base = checked_mul(native_flow, fx_prev)?;
    let currency = checked_mul(native, checked_sub(fx, fx_prev)?)?;
    let market = checked_sub(checked_sub(delta_base, flow_base)?, currency)?;
    Ok(MonetaryDayDecomposition {
        delta_base,
        flow_base,
        currency,
        market,
    })
}

pub fn decompose_holding_day(
    quantity_prev: Decimal,
    quantity: Decimal,
    price_prev: Decimal,
    price: Decimal,
    fx_prev: Decimal,
    fx: Decimal,
) -> Result<HoldingDayDecomposition, AppError> {
    let quantity_d = checked_mul(
        checked_mul(checked_sub(quantity, quantity_prev)?, price_prev)?,
        fx_prev,
    )?;
    let price_d = checked_mul(
        checked_mul(quantity, checked_sub(price, price_prev)?)?,
        fx_prev,
    )?;
    let currency = checked_mul(checked_mul(quantity, price)?, checked_sub(fx, fx_prev)?)?;
    Ok(HoldingDayDecomposition {
        quantity: quantity_d,
        price: price_d,
        currency,
    })
}

enum BridgeBuild {
    Available {
        start_value: Decimal,
        end_value: Decimal,
        parts: AttributionComponents,
        unexplained: Decimal,
        unknown_basis_flow: Decimal,
        basis_complete: bool,
    },
    Unavailable {
        blocking_dates: Vec<String>,
        unconvertible_flow_count: i64,
    },
}

fn build_bridge(
    scope: AnalyticsScope,
    series: &AnalyticsPeriodSeries,
    quotes_by_id: &HashMap<String, InstrumentQuoteRecordDto>,
) -> Result<BridgeBuild, AppError> {
    let t0 = series.start_on.to_ymd();
    let t1 = series.t1.to_ymd();
    let start_snapshot = series
        .snapshots_by_date
        .get(&t0)
        .ok_or(AppError::Internal)?;
    let end_snapshot = series
        .snapshots_by_date
        .get(&t1)
        .ok_or(AppError::Internal)?;
    let start_items = series
        .items_by_snapshot
        .get(&start_snapshot.id)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let end_items = series
        .items_by_snapshot
        .get(&end_snapshot.id)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let start_cutoff = Timestamp::parse(&start_snapshot.cutoff_at)?;
    let end_cutoff = Timestamp::parse(&end_snapshot.cutoff_at)?;
    let Some(start_value) = scope_value(
        scope,
        start_snapshot,
        start_items,
        &series.membership,
        &start_cutoff,
    )?
    else {
        return Ok(BridgeBuild::Unavailable {
            blocking_dates: vec![t0],
            unconvertible_flow_count: 0,
        });
    };
    let Some(end_value) = scope_value(
        scope,
        end_snapshot,
        end_items,
        &series.membership,
        &end_cutoff,
    )?
    else {
        return Ok(BridgeBuild::Unavailable {
            blocking_dates: vec![t1],
            unconvertible_flow_count: 0,
        });
    };

    let ledger = match accumulate_ledger_components(scope, series)? {
        Ok(ledger) => ledger,
        Err((blocking_dates, unconvertible_flow_count)) => {
            return Ok(BridgeBuild::Unavailable {
                blocking_dates,
                unconvertible_flow_count,
            });
        }
    };

    let days = dates_inclusive(series.start_on, series.t1);
    let mut instrument = Decimal::ZERO;
    let mut currency = Decimal::ZERO;
    let mut unknown_basis_flow = ledger.unknown_basis_flow;
    for window in days.windows(2) {
        let prev_on = window[0].to_ymd();
        let on = window[1].to_ymd();
        let Some(prev_snapshot) = series.snapshots_by_date.get(&prev_on) else {
            continue;
        };
        let Some(snapshot) = series.snapshots_by_date.get(&on) else {
            continue;
        };
        if !prev_snapshot.is_complete || !snapshot.is_complete {
            continue;
        }
        let prev_items = series
            .items_by_snapshot
            .get(&prev_snapshot.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let items = series
            .items_by_snapshot
            .get(&snapshot.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let prev_cutoff = Timestamp::parse(&prev_snapshot.cutoff_at)?;
        let cutoff = Timestamp::parse(&snapshot.cutoff_at)?;
        let prev_map = component_states(
            scope,
            prev_items,
            &series.membership,
            &prev_cutoff,
            quotes_by_id,
        )?;
        let curr_map = component_states(scope, items, &series.membership, &cutoff, quotes_by_id)?;
        let mut keys: HashSet<ComponentKey> = prev_map.keys().cloned().collect();
        keys.extend(curr_map.keys().cloned());
        let empty_flow = DayNativeFlow {
            native: Decimal::ZERO,
            remeasurement: false,
            unknown_basis: false,
        };
        for key in keys {
            let prev = prev_map.get(&key);
            let curr = curr_map.get(&key);
            let flow = ledger
                .native_flows
                .get(&(on.clone(), key.clone()))
                .unwrap_or(&empty_flow);
            let Some(decomp) = decompose_component(prev, curr, flow)? else {
                continue;
            };
            currency = checked_add(currency, decomp.currency)?;
            if flow.unknown_basis {
                unknown_basis_flow = checked_add(unknown_basis_flow, decomp.quantity_or_zero)?;
            } else if !flow.remeasurement {
                instrument = checked_add(instrument, decomp.instrument)?;
            }
        }
    }

    let parts = AttributionComponents {
        external_contributions: ledger.contributions,
        external_withdrawals: ledger.withdrawals,
        instrument_movement: instrument,
        currency_movement: currency,
        income: ledger.income,
        fees: ledger.fees,
        debt_principal_movement: ledger.debt,
        conversion_spread: ledger.spread,
    };
    let delta = checked_sub(end_value, start_value)?;
    let unexplained = unexplained_residual(delta, &parts)?;
    Ok(BridgeBuild::Available {
        start_value,
        end_value,
        parts,
        unexplained,
        unknown_basis_flow,
        basis_complete: ledger.basis_complete,
    })
}

struct LedgerTotals {
    contributions: Decimal,
    withdrawals: Decimal,
    income: Decimal,
    fees: Decimal,
    debt: Decimal,
    spread: Decimal,
    unknown_basis_flow: Decimal,
    basis_complete: bool,
    native_flows: HashMap<(String, ComponentKey), DayNativeFlow>,
}

struct ComponentDecomp {
    instrument: Decimal,
    currency: Decimal,
    quantity_or_zero: Decimal,
}

struct ComponentState {
    native: Decimal,
    #[allow(dead_code)]
    base: Decimal,
    fx: Decimal,
    price: Option<Decimal>,
    quantity: Option<Decimal>,
    is_liability: bool,
    is_holding: bool,
}

fn accumulate_ledger_components(
    scope: AnalyticsScope,
    series: &AnalyticsPeriodSeries,
) -> Result<Result<LedgerTotals, (Vec<String>, i64)>, AppError> {
    let excluded = excluded_activity_ids(&series.activities);
    let mut contributions = Decimal::ZERO;
    let mut withdrawals = Decimal::ZERO;
    let mut income = Decimal::ZERO;
    let mut fees = Decimal::ZERO;
    let mut debt = Decimal::ZERO;
    let mut spread = Decimal::ZERO;
    let mut unknown_basis_flow = Decimal::ZERO;
    let mut basis_complete = true;
    let mut native_flows: HashMap<(String, ComponentKey), DayNativeFlow> = HashMap::new();
    let mut unconvertible: Vec<String> = Vec::new();
    let mut unconvertible_count = 0_i64;

    for activity in &series.activities {
        if excluded.contains(&activity.id()) {
            continue;
        }
        let date_key = activity.effective_local_date().to_ymd();
        let cutoff = activity.effective_at();
        let mut legs = Vec::with_capacity(activity.legs().len());
        for leg in activity.legs() {
            let instrument_id = match leg.component() {
                LegComponent::HoldingQuantity { instrument_id, .. } => Some(*instrument_id),
                _ => None,
            };
            let facts = endpoint_facts(
                &series.membership,
                leg.account_id(),
                instrument_id,
                leg.component_kind(),
                cutoff,
            )?;
            legs.push(ScopeFlowLeg {
                role: leg.role(),
                component_kind: leg.component_kind(),
                endpoint_in_scope: endpoint_in_scope(scope, &facts),
                direction: leg.direction(),
            });
        }
        let classified = classify_scope_flow(
            scope,
            &ScopeFlowActivity {
                kind: activity.kind(),
                related_instrument_id: activity.related_instrument_id(),
                legs: legs.clone(),
            },
        );
        if !classified.basis_complete() {
            basis_complete = false;
        }

        match fx_conversion::overlay_for_activity_from_loaded(
            &series.household_id,
            series.base.as_str(),
            activity,
            &series.fx_quotes,
            &series.fx_observations,
            &series.current_preferences,
        )? {
            None => {}
            Some(overlay) => match fx_conversion::signed_spread(&overlay)? {
                ConversionSpread::Unconvertible => {
                    unconvertible.push(date_key.clone());
                    unconvertible_count += 1;
                }
                ConversionSpread::Signed(amount) => {
                    spread = checked_add(spread, amount)?;
                }
            },
        }

        for (index, (leg, classification)) in
            activity.legs().iter().zip(classified.legs()).enumerate()
        {
            let endpoint_in = legs[index].endpoint_in_scope;
            if matches!(classification, LegFlowClassification::NotInScope) {
                continue;
            }
            let wealth = classify(activity.kind(), leg.role());
            let converted = match classification {
                LegFlowClassification::UnknownBasisFlow => {
                    basis_complete = false;
                    match monetary_base(
                        activity,
                        leg,
                        &series.fx_quotes,
                        &series.fx_observations,
                        &series.current_preferences,
                        series.base,
                    )? {
                        Some(amount) => Some(amount),
                        None => {
                            add_native_flow(
                                &mut native_flows,
                                &date_key,
                                component_key(leg, None),
                                Decimal::ZERO,
                                false,
                                true,
                            )?;
                            continue;
                        }
                    }
                }
                _ => match monetary_base(
                    activity,
                    leg,
                    &series.fx_quotes,
                    &series.fx_observations,
                    &series.current_preferences,
                    series.base,
                )? {
                    Some(amount) => Some(amount),
                    None => {
                        unconvertible.push(date_key.clone());
                        unconvertible_count += 1;
                        continue;
                    }
                },
            };
            let Some(base_amount) = converted else {
                continue;
            };
            let signed_base = match classification {
                LegFlowClassification::SignedFlow { direction } => {
                    signed_amount(*direction, base_amount)
                }
                _ => signed_amount(leg.direction(), base_amount),
            };
            let native_opt = leg.component().money().ok();
            let native_signed = native_opt
                .map(|money| signed_amount(leg.direction(), money.amount()))
                .unwrap_or(Decimal::ZERO);
            let key = component_key(
                leg,
                native_opt.map(|money| money.currency().as_str().to_owned()),
            );
            let remeasurement = matches!(classification, LegFlowClassification::UnexplainedReturn);
            let unknown_basis = matches!(classification, LegFlowClassification::UnknownBasisFlow);
            if unknown_basis {
                unknown_basis_flow = checked_add(unknown_basis_flow, signed_base)?;
            }
            if !matches!(classification, LegFlowClassification::ZeroFlow) || native_opt.is_some() {
                add_native_flow(
                    &mut native_flows,
                    &date_key,
                    key,
                    native_signed,
                    remeasurement,
                    unknown_basis,
                )?;
            }

            if !endpoint_in
                && !matches!(
                    classification,
                    LegFlowClassification::Return | LegFlowClassification::UnknownBasisFlow
                )
            {
                continue;
            }
            match wealth {
                Classification::ExternalInflow => {
                    contributions = checked_add(contributions, signed_base)?;
                }
                Classification::ExternalOutflow => {
                    withdrawals = checked_add(withdrawals, signed_base)?;
                }
                Classification::Income => {
                    income = checked_add(income, signed_base)?;
                }
                Classification::Fee => {
                    fees = checked_add(fees, signed_base.abs())?;
                }
                Classification::DebtPrincipal => {
                    let facts = endpoint_facts(
                        &series.membership,
                        leg.account_id(),
                        None,
                        leg.component_kind(),
                        cutoff,
                    )?;
                    let nw_signed = if facts.is_liability {
                        -signed_base
                    } else {
                        signed_base
                    };
                    debt = checked_add(debt, nw_signed)?;
                }
                _ => {}
            }
        }
    }

    if unconvertible_count > 0 {
        unconvertible.sort();
        unconvertible.dedup();
        return Ok(Err((unconvertible, unconvertible_count)));
    }
    Ok(Ok(LedgerTotals {
        contributions,
        withdrawals,
        income,
        fees,
        debt,
        spread,
        unknown_basis_flow,
        basis_complete,
        native_flows,
    }))
}

fn add_native_flow(
    native_flows: &mut HashMap<(String, ComponentKey), DayNativeFlow>,
    date_key: &str,
    key: ComponentKey,
    native_signed: Decimal,
    remeasurement: bool,
    unknown_basis: bool,
) -> Result<(), AppError> {
    let entry = native_flows
        .entry((date_key.to_owned(), key))
        .or_insert(DayNativeFlow {
            native: Decimal::ZERO,
            remeasurement: false,
            unknown_basis: false,
        });
    entry.native = checked_add(entry.native, native_signed)?;
    entry.remeasurement |= remeasurement;
    entry.unknown_basis |= unknown_basis;
    Ok(())
}

fn component_key(leg: &crate::domain::ActivityLeg, currency: Option<String>) -> ComponentKey {
    match leg.component() {
        LegComponent::HoldingQuantity {
            instrument_id,
            holding_id,
            ..
        } => ComponentKey {
            account_id: leg.account_id().to_string(),
            holding_id: Some(holding_id.to_string()),
            instrument_id: Some(instrument_id.to_string()),
            kind: ComponentKind::HoldingQuantity.as_str().to_owned(),
            currency: String::new(),
        },
        _ => ComponentKey {
            account_id: leg.account_id().to_string(),
            holding_id: None,
            instrument_id: None,
            kind: leg.component_kind().as_str().to_owned(),
            currency: currency.unwrap_or_default(),
        },
    }
}

fn component_states(
    scope: AnalyticsScope,
    items: &[DailyValuationSnapshotItemRecord],
    membership: &AccountMembership,
    cutoff: &Timestamp,
    quotes_by_id: &HashMap<String, InstrumentQuoteRecordDto>,
) -> Result<HashMap<ComponentKey, ComponentState>, AppError> {
    let mut states = HashMap::new();
    for item in items {
        let account_id = crate::domain::AccountId::parse(&item.account_id)?;
        let instrument_id = item
            .instrument_id
            .as_deref()
            .map(crate::domain::InstrumentId::parse)
            .transpose()?;
        let kind = ComponentKind::parse(&item.component_kind)?;
        let facts = endpoint_facts(membership, account_id, instrument_id, kind, cutoff)?;
        if !endpoint_in_scope(scope, &facts) {
            continue;
        }
        if !item.is_complete {
            continue;
        }
        let Some(native) = item.native_amount.as_deref() else {
            continue;
        };
        let Some(base) = item.base_amount.as_deref() else {
            continue;
        };
        let currency = item.native_currency.clone().unwrap_or_default();
        let native = parse_decimal(native)?;
        let base = parse_decimal(base)?;
        let fx = if native.is_zero() {
            Decimal::ONE
        } else {
            checked_div(base, native)?
        };
        let is_holding = kind == ComponentKind::HoldingQuantity;
        let price = item
            .instrument_quote_id
            .as_ref()
            .and_then(|id| quotes_by_id.get(id))
            .map(|quote| UnitPrice::parse(&quote.unit_price).map(|price| price.amount()))
            .transpose()?;
        let quantity = match price {
            Some(price) if !price.is_zero() => Some(checked_div(native, price)?),
            _ => None,
        };
        let key = ComponentKey {
            account_id: item.account_id.clone(),
            holding_id: item.holding_id.clone(),
            instrument_id: item.instrument_id.clone(),
            kind: item.component_kind.clone(),
            currency: if is_holding { String::new() } else { currency },
        };
        states.insert(
            key,
            ComponentState {
                native,
                base,
                fx,
                price,
                quantity,
                is_liability: facts.is_liability,
                is_holding,
            },
        );
    }
    Ok(states)
}

fn decompose_component(
    prev: Option<&ComponentState>,
    curr: Option<&ComponentState>,
    flow: &DayNativeFlow,
) -> Result<Option<ComponentDecomp>, AppError> {
    let liability = curr
        .or(prev)
        .map(|state| state.is_liability)
        .unwrap_or(false);
    let sign = if liability {
        -Decimal::ONE
    } else {
        Decimal::ONE
    };
    let is_holding = curr.or(prev).is_some_and(|state| state.is_holding);
    let native_prev = prev.map(|state| state.native).unwrap_or(Decimal::ZERO);
    let native = curr.map(|state| state.native).unwrap_or(Decimal::ZERO);
    let fx_curr = curr.map(|state| state.fx);
    let fx_prev = prev
        .map(|state| state.fx)
        .or(fx_curr)
        .unwrap_or(Decimal::ONE);
    let fx = fx_curr.unwrap_or(fx_prev);
    if is_holding {
        let price_curr = curr.and_then(|state| state.price);
        let price_prev = prev
            .and_then(|state| state.price)
            .or(price_curr)
            .unwrap_or(Decimal::ZERO);
        let price = price_curr.unwrap_or(price_prev);
        if price.is_zero() && price_prev.is_zero() {
            let monetary = decompose_monetary_day(native_prev, fx_prev, native, fx, flow.native)?;
            return Ok(Some(ComponentDecomp {
                instrument: checked_mul(monetary.market, sign)?,
                currency: checked_mul(monetary.currency, sign)?,
                quantity_or_zero: Decimal::ZERO,
            }));
        }
        let qty_prev = prev
            .and_then(|state| state.quantity)
            .unwrap_or(Decimal::ZERO);
        let qty = curr
            .and_then(|state| state.quantity)
            .unwrap_or(Decimal::ZERO);
        let holding = decompose_holding_day(qty_prev, qty, price_prev, price, fx_prev, fx)?;
        Ok(Some(ComponentDecomp {
            instrument: checked_mul(holding.price, sign)?,
            currency: checked_mul(holding.currency, sign)?,
            quantity_or_zero: checked_mul(holding.quantity, sign)?,
        }))
    } else {
        let monetary = decompose_monetary_day(native_prev, fx_prev, native, fx, flow.native)?;
        Ok(Some(ComponentDecomp {
            instrument: checked_mul(monetary.market, sign)?,
            currency: checked_mul(monetary.currency, sign)?,
            quantity_or_zero: Decimal::ZERO,
        }))
    }
}

#[allow(clippy::too_many_arguments)]
fn available_dto(
    start_on: CalendarDate,
    end_on: CalendarDate,
    currency: CurrencyCode,
    start_value: Decimal,
    end_value: Decimal,
    parts: &AttributionComponents,
    unexplained: Decimal,
    unknown_basis_flow: Decimal,
    basis_complete: bool,
) -> Result<AvailableAttributionDto, AppError> {
    let delta = checked_sub(end_value, start_value)?;
    Ok(AvailableAttributionDto {
        start_on: start_on.to_ymd(),
        end_on: end_on.to_ymd(),
        start_net_worth: signed_dto(start_value, currency)?,
        end_net_worth: signed_dto(end_value, currency)?,
        delta: signed_dto(delta, currency)?,
        external_contributions: signed_dto(parts.external_contributions, currency)?,
        external_withdrawals: signed_dto(parts.external_withdrawals, currency)?,
        instrument_movement: signed_dto(parts.instrument_movement, currency)?,
        currency_movement: signed_dto(parts.currency_movement, currency)?,
        income: signed_dto(parts.income, currency)?,
        fees: signed_dto(checked_sub(Decimal::ZERO, parts.fees)?, currency)?,
        debt_principal_movement: signed_dto(parts.debt_principal_movement, currency)?,
        conversion_spread: signed_dto(parts.conversion_spread, currency)?,
        unexplained: signed_dto(unexplained, currency)?,
        unknown_basis_flow: signed_dto(unknown_basis_flow, currency)?,
        basis_complete,
        method_note: METHOD_NOTE.to_owned(),
    })
}

fn signed_dto(amount: Decimal, currency: CurrencyCode) -> Result<SignedMoneyDto, AppError> {
    let value = SignedMoney::from_canonical(amount, currency)?;
    Ok(SignedMoneyDto {
        amount: value.canonical_amount(),
        currency: value.currency().as_str().to_owned(),
    })
}

fn unavailable(
    blocking_dates: Vec<String>,
    unconvertible_flow_count: i64,
) -> NetWorthAttributionDto {
    NetWorthAttributionDto::Unavailable(UnavailableAttributionDto {
        reason: REASON_PERIOD_UNAVAILABLE.to_owned(),
        blocking_dates,
        unconvertible_flow_count: i32::try_from(unconvertible_flow_count).unwrap_or(i32::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        decompose_holding_day, decompose_monetary_day, get_net_worth_attribution,
        get_net_worth_attribution_at_in_tx, named_component_sum, unexplained_residual,
        AttributionComponents, NetWorthAttributionDto, METHOD_NOTE, REASON_PERIOD_UNAVAILABLE,
    };
    use crate::{
        application::{
            account_service::{self, CreateAccountInput, OwnershipShareInput},
            fx_conversion,
            history_query_service::{
                self, confirm_history_timezone, create_activity, ConfirmHistoryTimezoneInput,
                CreateActivityInput,
            },
            history_snapshot_service::{rebuild_history_snapshots, RebuildHistorySnapshotsInput},
            holding_service::{self, CreateHoldingInput},
            instrument_service::{self, CreateInstrumentInput},
            query_count,
            quote_service::{self, AppendManualFxQuoteInput, AppendManualInstrumentQuoteInput},
            reference::{begin_read_tx, finish_read_tx, require_household_tx},
        },
        domain::{AnalyticsScope, HistoryTimezone, Timestamp},
        infrastructure::{database::connect_writable, database_bootstrap::MIGRATOR},
        state::AppState,
        test_support::{cleanup, onboarded_state, test_path},
    };
    use rust_decimal::Decimal;
    use std::fs;
    use std::path::PathBuf;
    use std::str::FromStr;

    const TRANSFER: &str = "01a0188f-862a-7a60-98ad-450cc32e742a";
    const NOW: &str = "2026-08-19T04:00:00.000Z";

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).expect("decimal")
    }

    fn contract_parts() -> AttributionComponents {
        AttributionComponents {
            external_contributions: dec("10000"),
            external_withdrawals: Decimal::ZERO,
            instrument_movement: dec("5900"),
            currency_movement: dec("1800"),
            income: dec("500"),
            fees: dec("200"),
            debt_principal_movement: Decimal::ZERO,
            conversion_spread: dec("-100"),
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
        let path = test_path("v014-p6-attr", name);
        let _ = fs::remove_file(&path);
        apply_migrations(&path, &[1, 2, 3]).await;
        load_sql(&path, include_str!("../../test-fixtures/v0.1.3.sql")).await;
        let state = AppState::initialize(path.clone()).await;
        (state, path)
    }

    async fn attribution_at(
        state: &AppState,
        start_on: &str,
        end_on: &str,
    ) -> NetWorthAttributionDto {
        let db = state.writable_db().expect("db");
        let mut tx = begin_read_tx(db).await.expect("tx");
        let now = Timestamp::parse(NOW).expect("now");
        let result = get_net_worth_attribution_at_in_tx(
            &mut tx,
            AnalyticsScope::Household,
            start_on,
            end_on,
            &now,
        )
        .await;
        finish_read_tx(tx, result).await.expect("attribution")
    }

    fn owner(member_id: &str) -> OwnershipShareInput {
        OwnershipShareInput {
            member_id: member_id.to_owned(),
            percent: Some("100".to_owned()),
            share_bps: None,
        }
    }

    fn holdings_input(name: &str, member_id: &str) -> CreateAccountInput {
        CreateAccountInput {
            name: name.to_owned(),
            primary_category: "investment".to_owned(),
            secondary_category: "brokerage_account".to_owned(),
            default_currency: "CNY".to_owned(),
            institution_id: None,
            group_id: None,
            tracking_mode: Some("holdings".to_owned()),
            note: None,
            include_in_net_worth: true,
            include_in_investment: true,
            include_in_liquid_assets: false,
            opened_on: None,
            closed_on: None,
            owners: vec![owner(member_id)],
            initial_amount: None,
        }
    }

    fn bank_input(name: &str, member_id: &str, amount: &str, currency: &str) -> CreateAccountInput {
        CreateAccountInput {
            name: name.to_owned(),
            primary_category: "cash_equivalent".to_owned(),
            secondary_category: "bank_account".to_owned(),
            default_currency: currency.to_owned(),
            institution_id: None,
            group_id: None,
            tracking_mode: None,
            note: None,
            include_in_net_worth: true,
            include_in_investment: false,
            include_in_liquid_assets: true,
            opened_on: None,
            closed_on: None,
            owners: vec![owner(member_id)],
            initial_amount: Some(amount.to_owned()),
        }
    }

    fn liability_input(name: &str, member_id: &str, amount: &str) -> CreateAccountInput {
        let mut input = bank_input(name, member_id, amount, "CNY");
        input.primary_category = "liability".to_owned();
        input.secondary_category = "personal_debt".to_owned();
        input.include_in_liquid_assets = false;
        input
    }

    async fn member_id(state: &AppState) -> String {
        crate::application::member_service::list_members(state, false)
            .await
            .expect("members")[0]
            .id
            .clone()
    }

    async fn confirm_tz(state: &AppState) {
        let origin = history_query_service::get_history_origin(state)
            .await
            .expect("origin");
        if origin.timezone_confirmed {
            return;
        }
        confirm_history_timezone(
            state,
            ConfirmHistoryTimezoneInput {
                timezone: origin.timezone.clone(),
            },
        )
        .await
        .expect("confirm");
    }

    async fn set_origin_on(state: &AppState, local_date: &str) -> Timestamp {
        let origin = history_query_service::get_history_origin(state)
            .await
            .expect("origin");
        let timezone = HistoryTimezone::parse(&origin.timezone).expect("tz");
        let date = crate::domain::CalendarDate::parse(local_date).expect("date");
        let origin_at = date
            .pred()
            .map(|previous| crate::domain::closed_day_cutoff(timezone, previous).expect("cutoff"))
            .unwrap_or_else(|| {
                Timestamp::parse(&format!("{local_date}T00:00:00.000Z")).expect("ts")
            });
        sqlx::query("UPDATE history_origins SET origin_at = ?, origin_local_date = ?")
            .bind(origin_at.to_rfc3339())
            .bind(local_date)
            .execute(state.writable_db().expect("db"))
            .await
            .expect("origin backdate");
        sqlx::query(
            "UPDATE history_snapshot_state SET dirty_from = ?, last_completed_on = NULL, rebuild_status = 'idle'",
        )
        .bind(local_date)
        .execute(state.writable_db().expect("db"))
        .await
        .expect("dirty backdate");
        origin_at
    }

    async fn backdate_facts_to(state: &AppState, at: &Timestamp) {
        let timestamp = at.to_rfc3339();
        let database = state.writable_db().expect("db");
        for sql in [
            "UPDATE account_values SET effective_at = ?, created_at = ?",
            "UPDATE account_cash_values SET effective_at = ?, created_at = ?",
            "UPDATE holding_quantity_values SET effective_at = ?, created_at = ?",
            "UPDATE account_state_observations SET effective_at = ?, created_at = ?",
            "UPDATE holding_state_observations SET effective_at = ?, created_at = ?",
            "UPDATE fx_quotes SET quoted_at = ?, created_at = ?",
            "UPDATE instrument_quotes SET quoted_at = ?, created_at = ?",
        ] {
            sqlx::query(sql)
                .bind(&timestamp)
                .bind(&timestamp)
                .execute(database)
                .await
                .expect("fact timestamps");
        }
    }

    async fn sync_origin_account_values(state: &AppState) {
        sqlx::query(
            "INSERT INTO history_origin_account_values (origin_id, account_id, amount, currency, value_kind)
             SELECT o.id, v.account_id, v.amount, v.currency, v.value_kind
             FROM history_origins o
             JOIN accounts a ON a.household_id = o.household_id
             JOIN account_values v ON v.account_id = a.id
             JOIN (
                SELECT id,
                       ROW_NUMBER() OVER (
                           PARTITION BY account_id
                           ORDER BY effective_at DESC, created_at DESC, id DESC
                       ) AS rn
                FROM account_values
             ) latest ON latest.id = v.id AND latest.rn = 1
             WHERE a.tracking_mode IN ('balance', 'manual_value')
               AND NOT EXISTS (
                    SELECT 1
                    FROM history_origin_account_values captured
                    WHERE captured.origin_id = o.id
                      AND captured.account_id = a.id
               )",
        )
        .execute(state.writable_db().expect("db"))
        .await
        .expect("origin account values");
    }

    async fn rebuild_all(state: &AppState, label: &str) {
        loop {
            let result =
                rebuild_history_snapshots(state, RebuildHistorySnapshotsInput { cancel: false })
                    .await
                    .unwrap_or_else(|error| panic!("{label}: {error:?}"));
            if !result.remaining {
                break;
            }
        }
    }

    fn available(result: &NetWorthAttributionDto) -> &super::AvailableAttributionDto {
        match result {
            NetWorthAttributionDto::Available(value) => value,
            NetWorthAttributionDto::Unavailable(value) => {
                panic!("expected available, got {value:?}")
            }
        }
    }

    #[test]
    fn contract_bridge_residual_is_plus_100_and_sums_to_18000() {
        let parts = contract_parts();
        let unexplained = unexplained_residual(dec("18000"), &parts).expect("residual");
        assert_eq!(unexplained, dec("100"));
        let named = named_component_sum(&parts).expect("named");
        assert_eq!(checked_add_test(named, unexplained), dec("18000"));
        assert_eq!(parts.instrument_movement, dec("5900"));
        assert_ne!(parts.instrument_movement, dec("6000"));
        assert_eq!(parts.external_contributions, dec("10000"));
        assert_eq!(parts.currency_movement, dec("1800"));
        assert_eq!(parts.income, dec("500"));
        assert_eq!(parts.fees, dec("200"));
        assert_eq!(parts.conversion_spread, dec("-100"));
    }

    fn checked_add_test(left: Decimal, right: Decimal) -> Decimal {
        crate::domain::checked_add(left, right).expect("add")
    }

    #[test]
    fn residual_is_never_folded_into_a_named_bucket() {
        let mut parts = contract_parts();
        let unexplained = unexplained_residual(dec("18000"), &parts).expect("residual");
        parts.instrument_movement =
            crate::domain::checked_add(parts.instrument_movement, unexplained).expect("fold");
        let folded = unexplained_residual(dec("18000"), &parts).expect("after fold");
        assert_eq!(folded, Decimal::ZERO);
        assert_eq!(parts.instrument_movement, dec("6000"));
        let original = contract_parts();
        assert_eq!(
            unexplained_residual(dec("18000"), &original).expect("original"),
            dec("100")
        );
        assert_eq!(original.instrument_movement, dec("5900"));
    }

    #[test]
    fn same_day_execution_versus_previous_close_lands_in_unexplained() {
        let holding = decompose_holding_day(
            dec("2"),
            dec("3"),
            dec("100"),
            dec("105"),
            Decimal::ONE,
            Decimal::ONE,
        )
        .expect("holding");
        assert_eq!(holding.quantity, dec("100"));
        assert_eq!(holding.price, dec("15"));
        assert_eq!(holding.currency, Decimal::ZERO);
        let leftover = crate::domain::checked_sub(holding.quantity, dec("110")).expect("leftover");
        assert_eq!(leftover, dec("-10"));
        let delta = dec("5");
        let parts = AttributionComponents {
            external_contributions: Decimal::ZERO,
            external_withdrawals: Decimal::ZERO,
            instrument_movement: holding.price,
            currency_movement: holding.currency,
            income: Decimal::ZERO,
            fees: Decimal::ZERO,
            debt_principal_movement: Decimal::ZERO,
            conversion_spread: Decimal::ZERO,
        };
        let unexplained = unexplained_residual(delta, &parts).expect("residual");
        assert_eq!(unexplained, leftover);
        assert_eq!(parts.instrument_movement, dec("15"));
        assert_ne!(parts.instrument_movement, dec("5"));
    }

    #[test]
    fn monetary_remeasurement_is_unexplained_not_market() {
        let day = decompose_monetary_day(
            dec("100"),
            Decimal::ONE,
            dec("150"),
            Decimal::ONE,
            Decimal::ZERO,
        )
        .expect("day");
        assert_eq!(day.market, dec("50"));
        let parts = AttributionComponents {
            external_contributions: Decimal::ZERO,
            external_withdrawals: Decimal::ZERO,
            instrument_movement: Decimal::ZERO,
            currency_movement: day.currency,
            income: Decimal::ZERO,
            fees: Decimal::ZERO,
            debt_principal_movement: Decimal::ZERO,
            conversion_spread: Decimal::ZERO,
        };
        let unexplained = unexplained_residual(day.delta_base, &parts).expect("residual");
        assert_eq!(unexplained, day.market);
        assert_eq!(parts.instrument_movement, Decimal::ZERO);
    }

    #[test]
    fn attribution_module_does_not_use_binary_floats() {
        let code = include_str!("attribution_service.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("code");
        assert!(!code.contains("f32"));
        assert!(!code.contains("f64"));
    }

    #[test]
    fn fixture_same_day_complete_endpoints_balance_with_zero_residual_present() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("same-day").await;
            let result = attribution_at(&state, "2026-01-02", "2026-01-02").await;
            let value = available(&result);
            assert_eq!(value.start_net_worth.amount, "63190");
            assert_eq!(value.end_net_worth.amount, "63190");
            assert_eq!(value.delta.amount, "0");
            assert_eq!(value.unexplained.amount, "0");
            assert!(value.unexplained.amount == "0");
            assert_eq!(value.external_contributions.amount, "0");
            assert_eq!(value.external_withdrawals.amount, "0");
            assert_eq!(value.instrument_movement.amount, "0");
            assert_eq!(value.currency_movement.amount, "0");
            assert_eq!(value.income.amount, "0");
            assert_eq!(value.fees.amount, "0");
            assert_eq!(value.debt_principal_movement.amount, "0");
            assert_eq!(value.conversion_spread.amount, "0");
            assert_eq!(value.method_note, METHOD_NOTE);
            let reconstructed = named_from_dto(value);
            assert_eq!(
                unexplained_residual(dec(&value.delta.amount), &reconstructed).expect("id"),
                Decimal::ZERO
            );
            cleanup(&path);
        });
    }

    #[test]
    fn fixture_complete_endpoint_periods_balance_exactly() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("complete-periods").await;
            let db = state.writable_db().expect("db");
            let dates: Vec<(String, i64)> = sqlx::query_as(
                "SELECT s.snapshot_on, s.is_complete
                 FROM daily_valuation_snapshots s
                 JOIN (
                    SELECT snapshot_on, MAX(revision) AS revision
                    FROM daily_valuation_snapshots
                    GROUP BY snapshot_on
                 ) latest
                   ON latest.snapshot_on = s.snapshot_on
                  AND latest.revision = s.revision
                 ORDER BY s.snapshot_on",
            )
            .fetch_all(db)
            .await
            .expect("dates");
            assert!(
                dates
                    .iter()
                    .any(|(date, complete)| date == "2026-01-02" && *complete == 1),
                "{dates:?}"
            );
            let mut checked = 0_u32;
            for (start_on, start_complete) in &dates {
                for (end_on, end_complete) in &dates {
                    if end_on.as_str() < start_on.as_str() {
                        continue;
                    }
                    let result = attribution_at(&state, start_on, end_on).await;
                    if *start_complete == 1 && *end_complete == 1 {
                        let value = available(&result);
                        let reconstructed = named_from_dto(value);
                        assert_eq!(
                            unexplained_residual(dec(&value.delta.amount), &reconstructed)
                                .expect("identity"),
                            dec(&value.unexplained.amount),
                            "{start_on}..{end_on}"
                        );
                        assert_eq!(
                            checked_add_test(
                                named_component_sum(&reconstructed).expect("named"),
                                dec(&value.unexplained.amount),
                            ),
                            dec(&value.delta.amount),
                            "{start_on}..{end_on}"
                        );
                        checked += 1;
                    } else {
                        match result {
                            NetWorthAttributionDto::Unavailable(_) => {}
                            NetWorthAttributionDto::Available(value) => {
                                panic!(
                                    "expected unavailable for {start_on}..{end_on}, got {value:?}"
                                )
                            }
                        }
                    }
                }
            }
            assert!(
                checked >= 1,
                "at least the complete 2026-01-02 same-day period"
            );
            cleanup(&path);
        });
    }

    fn named_from_dto(value: &super::AvailableAttributionDto) -> AttributionComponents {
        AttributionComponents {
            external_contributions: dec(&value.external_contributions.amount),
            external_withdrawals: dec(&value.external_withdrawals.amount),
            instrument_movement: dec(&value.instrument_movement.amount),
            currency_movement: dec(&value.currency_movement.amount),
            income: dec(&value.income.amount),
            fees: -dec(&value.fees.amount),
            debt_principal_movement: dec(&value.debt_principal_movement.amount),
            conversion_spread: dec(&value.conversion_spread.amount),
        }
    }

    #[test]
    fn incomplete_endpoint_snapshots_are_unavailable_with_no_partial_bridge() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("incomplete-end").await;
            let to_third = attribution_at(&state, "2026-01-02", "2026-01-03").await;
            match to_third {
                NetWorthAttributionDto::Unavailable(value) => {
                    assert_eq!(value.reason, REASON_PERIOD_UNAVAILABLE);
                    assert!(value.blocking_dates.contains(&"2026-01-03".to_owned()));
                    assert_eq!(value.unconvertible_flow_count, 0);
                }
                NetWorthAttributionDto::Available(_) => panic!("expected unavailable"),
            }
            let to_fourth = attribution_at(&state, "2026-01-02", "2026-01-04").await;
            match to_fourth {
                NetWorthAttributionDto::Unavailable(value) => {
                    assert_eq!(value.reason, REASON_PERIOD_UNAVAILABLE);
                    assert!(value.blocking_dates.contains(&"2026-01-04".to_owned()));
                    assert!(!value.blocking_dates.is_empty());
                }
                NetWorthAttributionDto::Available(_) => panic!("expected unavailable"),
            }
            cleanup(&path);
        });
    }

    #[test]
    fn fixture_conversion_spread_overlay_is_9_43_cny_loss() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("spread-overlay").await;
            let db = state.writable_db().expect("db");
            let mut tx = begin_read_tx(db).await.expect("tx");
            let household = require_household_tx(&mut tx).await.expect("hh");
            let activity =
                crate::application::history_repositories::get_activity(&mut tx, TRANSFER)
                    .await
                    .expect("activity")
                    .expect("present");
            let overlay = fx_conversion::overlay_for_activity(
                &mut tx,
                &household.id,
                &household.base_currency,
                &activity,
            )
            .await
            .expect("overlay")
            .expect("cross-currency");
            finish_read_tx(tx, Ok::<(), crate::error::AppError>(()))
                .await
                .expect("end");
            assert_eq!(overlay.status, "computed");
            assert_eq!(overlay.spread_amount.as_deref(), Some("9.43"));
            assert_eq!(overlay.spread_effect.as_deref(), Some("loss"));
            assert_eq!(overlay.spread_currency.as_deref(), Some("CNY"));
            assert_eq!(overlay.source_base.as_deref(), Some("1000"));
            assert_eq!(overlay.destination_base.as_deref(), Some("990.57"));
            match fx_conversion::signed_spread(&overlay).expect("signed") {
                fx_conversion::ConversionSpread::Signed(amount) => assert_eq!(amount, dec("-9.43")),
                other => panic!("expected signed spread, got {other:?}"),
            }
            cleanup(&path);
        });
    }

    #[test]
    fn snapshot_items_and_activities_are_batched() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("batch-queries").await;
            let (result, families) = query_count::capture_async(|| {
                get_net_worth_attribution(
                    &state,
                    AnalyticsScope::Household,
                    "2026-01-02",
                    "2026-01-04",
                )
            })
            .await;
            result.expect("result");
            let items = families
                .iter()
                .filter(|family| **family == "snapshot_items")
                .count();
            let headers = families
                .iter()
                .filter(|family| **family == "activity_headers")
                .count();
            let legs = families
                .iter()
                .filter(|family| **family == "activity_legs")
                .count();
            let states = families
                .iter()
                .filter(|family| **family == "account_states")
                .count();
            assert_eq!(items, 1, "{families:?}");
            assert!(headers <= 1, "{families:?}");
            assert!(legs <= 1, "{families:?}");
            assert_eq!(states, 1, "{families:?}");
            cleanup(&path);
        });
    }

    #[test]
    fn attribution_read_writes_nothing() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("read-only").await;
            let db = state.writable_db().expect("db");
            let before_activities: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
                .fetch_one(db)
                .await
                .expect("count");
            let before_snapshots: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM daily_valuation_snapshots")
                    .fetch_one(db)
                    .await
                    .expect("count");
            get_net_worth_attribution(
                &state,
                AnalyticsScope::Household,
                "2026-01-02",
                "2026-01-04",
            )
            .await
            .expect("result");
            let after_activities: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
                .fetch_one(db)
                .await
                .expect("count");
            let after_snapshots: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM daily_valuation_snapshots")
                    .fetch_one(db)
                    .await
                    .expect("count");
            assert_eq!(before_activities, after_activities);
            assert_eq!(before_snapshots, after_snapshots);
            cleanup(&path);
        });
    }

    #[test]
    fn paired_debt_draw_nets_to_zero_unpaired_does_not() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("p6-debt").await;
            confirm_tz(&state).await;
            let timezone = HistoryTimezone::parse(
                &history_query_service::get_history_origin(&state)
                    .await
                    .expect("origin")
                    .timezone,
            )
            .expect("tz");
            let today = timezone.local_date(&Timestamp::now());
            let last_closed = today.pred().expect("yesterday");
            let t0 = last_closed.checked_add_days(-2).expect("t0");
            let t1 = last_closed;
            let flow_on = last_closed.checked_add_days(-1).expect("flow");
            let walt = member_id(&state).await;
            let cash =
                account_service::create_account(&state, bank_input("Cash", &walt, "2000", "CNY"))
                    .await
                    .expect("cash");
            let liability =
                account_service::create_account(&state, liability_input("Loan", &walt, "0"))
                    .await
                    .expect("loan");
            let origin_at = set_origin_on(&state, &t0.to_ymd()).await;
            backdate_facts_to(&state, &origin_at).await;
            rebuild_all(&state, "rebuild").await;
            create_activity(
                &state,
                CreateActivityInput::DebtDraw {
                    local_date: flow_on.to_ymd(),
                    local_time: "10:00".to_owned(),
                    ambiguous_offset: None,
                    note: None,
                    liability_account_id: liability.id.clone(),
                    principal_amount: "100".to_owned(),
                    principal_currency: "CNY".to_owned(),
                    cash_account_id: Some(cash.id.clone()),
                    cash_component: Some("account_value".to_owned()),
                    cash_amount: Some("100".to_owned()),
                    cash_currency: Some("CNY".to_owned()),
                    fx_rate: None,
                },
            )
            .await
            .expect("draw");
            rebuild_all(&state, "rebuild").await;
            let unpaired = attribution_at(&state, &t0.to_ymd(), &t1.to_ymd()).await;
            let unpaired = available(&unpaired);
            assert_ne!(unpaired.debt_principal_movement.amount, "0");
            assert!(unpaired.basis_complete);
            create_activity(
                &state,
                CreateActivityInput::DebtPayment {
                    local_date: flow_on.to_ymd(),
                    local_time: "11:00".to_owned(),
                    ambiguous_offset: None,
                    note: None,
                    liability_account_id: liability.id.clone(),
                    principal_amount: "100".to_owned(),
                    principal_currency: "CNY".to_owned(),
                    cash_account_id: cash.id.clone(),
                    cash_component: "account_value".to_owned(),
                    cash_amount: "100".to_owned(),
                    cash_currency: "CNY".to_owned(),
                    fx_rate: None,
                    fee_amount: None,
                    fee_kind: None,
                },
            )
            .await
            .expect("payment");
            rebuild_all(&state, "rebuild").await;
            let paired = attribution_at(&state, &t0.to_ymd(), &t1.to_ymd()).await;
            let paired = available(&paired);
            assert_eq!(paired.debt_principal_movement.amount, "0");
            cleanup(&path);
        });
    }

    #[test]
    fn monetary_remeasurement_lands_in_unexplained_not_instrument_movement() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("p6-qty-re").await;
            confirm_tz(&state).await;
            let timezone = HistoryTimezone::parse(
                &history_query_service::get_history_origin(&state)
                    .await
                    .expect("origin")
                    .timezone,
            )
            .expect("tz");
            let today = timezone.local_date(&Timestamp::now());
            let last_closed = today.pred().expect("yesterday");
            let t0 = last_closed.checked_add_days(-2).expect("t0");
            let t1 = last_closed;
            let flow_on = last_closed.checked_add_days(-1).expect("flow");
            let walt = member_id(&state).await;
            let cash =
                account_service::create_account(&state, bank_input("Cash", &walt, "1000", "CNY"))
                    .await
                    .expect("cash");
            let origin_at = set_origin_on(&state, &t0.to_ymd()).await;
            backdate_facts_to(&state, &origin_at).await;
            rebuild_all(&state, "rebuild").await;
            create_activity(
                &state,
                CreateActivityInput::BalanceAdjustment {
                    local_date: flow_on.to_ymd(),
                    local_time: "10:00".to_owned(),
                    ambiguous_offset: None,
                    note: None,
                    account_id: cash.id.clone(),
                    amount: "1050".to_owned(),
                    currency: "CNY".to_owned(),
                },
            )
            .await
            .expect("remeasure");
            rebuild_all(&state, "rebuild").await;
            let result = attribution_at(&state, &t0.to_ymd(), &t1.to_ymd()).await;
            let value = available(&result);
            assert_eq!(value.instrument_movement.amount, "0");
            assert_ne!(value.unexplained.amount, "0");
            assert_eq!(value.unexplained.amount, "50");
            cleanup(&path);
        });
    }

    #[test]
    fn quantity_remeasurement_is_unknown_basis_flow() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("p6-qty-unknown").await;
            confirm_tz(&state).await;
            let timezone = HistoryTimezone::parse(
                &history_query_service::get_history_origin(&state)
                    .await
                    .expect("origin")
                    .timezone,
            )
            .expect("tz");
            let today = timezone.local_date(&Timestamp::now());
            let last_closed = today.pred().expect("yesterday");
            let t0 = last_closed.checked_add_days(-2).expect("t0");
            let t1 = last_closed;
            let flow_on = last_closed.checked_add_days(-1).expect("flow");
            let walt = member_id(&state).await;
            let brokerage =
                account_service::create_account(&state, holdings_input("Broker", &walt))
                    .await
                    .expect("broker");
            let instrument = instrument_service::create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "Index".to_owned(),
                    symbol: Some("IDX".to_owned()),
                    instrument_type: "etf".to_owned(),
                    quote_currency: "CNY".to_owned(),
                    market_code: None,
                    country_code: None,
                    isin: None,
                    provider_key: None,
                    provider_symbol: None,
                    quote_preference: Some("manual".to_owned()),
                    note: None,
                },
            )
            .await
            .expect("instrument");
            let holding = holding_service::create_holding(
                &state,
                CreateHoldingInput {
                    account_id: brokerage.id,
                    instrument_id: instrument.id.clone(),
                    quantity: "1".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("holding");
            quote_service::append_manual_instrument_quote(
                &state,
                AppendManualInstrumentQuoteInput {
                    instrument_id: instrument.id,
                    unit_price: "10".to_owned(),
                    quoted_at: None,
                },
            )
            .await
            .expect("quote");
            let origin_at = set_origin_on(&state, &t0.to_ymd()).await;
            backdate_facts_to(&state, &origin_at).await;
            rebuild_all(&state, "rebuild").await;
            create_activity(
                &state,
                CreateActivityInput::PositionAdjustment {
                    local_date: flow_on.to_ymd(),
                    local_time: "10:00".to_owned(),
                    ambiguous_offset: None,
                    note: None,
                    holding_id: holding.id,
                    quantity: "2".to_owned(),
                },
            )
            .await
            .expect("adjust");
            rebuild_all(&state, "rebuild").await;
            let result = attribution_at(&state, &t0.to_ymd(), &t1.to_ymd()).await;
            let value = available(&result);
            assert!(!value.basis_complete);
            assert_ne!(value.unknown_basis_flow.amount, "0");
            cleanup(&path);
        });
    }

    #[test]
    fn conversion_spread_on_complete_endpoints_matches_9_43_loss() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("p6-spread").await;
            confirm_tz(&state).await;
            let timezone = HistoryTimezone::parse(
                &history_query_service::get_history_origin(&state)
                    .await
                    .expect("origin")
                    .timezone,
            )
            .expect("tz");
            let today = timezone.local_date(&Timestamp::now());
            let last_closed = today.pred().expect("yesterday");
            let t0 = last_closed.checked_add_days(-2).expect("t0");
            let t1 = last_closed;
            let flow_on = last_closed.checked_add_days(-1).expect("flow");
            let walt = member_id(&state).await;
            let source = account_service::create_account(
                &state,
                bank_input("CNY Cash", &walt, "1000", "CNY"),
            )
            .await
            .expect("cny");
            let dest =
                account_service::create_account(&state, bank_input("SGD Cash", &walt, "0", "SGD"))
                    .await
                    .expect("sgd");
            quote_service::append_manual_fx_quote(
                &state,
                AppendManualFxQuoteInput {
                    base_currency: "SGD".to_owned(),
                    quote_currency: "CNY".to_owned(),
                    rate: "5.3".to_owned(),
                    quoted_at: None,
                },
            )
            .await
            .expect("fx");
            let origin_at = set_origin_on(&state, &t0.to_ymd()).await;
            backdate_facts_to(&state, &origin_at).await;
            sync_origin_account_values(&state).await;
            rebuild_all(&state, "rebuild-before-transfer").await;
            create_activity(
                &state,
                CreateActivityInput::Transfer {
                    local_date: flow_on.to_ymd(),
                    local_time: "10:00".to_owned(),
                    ambiguous_offset: None,
                    note: None,
                    source_account_id: source.id.clone(),
                    source_component: "account_value".to_owned(),
                    source_amount: "1000".to_owned(),
                    source_currency: "CNY".to_owned(),
                    destination_account_id: dest.id.clone(),
                    destination_component: "account_value".to_owned(),
                    destination_amount: "186.9".to_owned(),
                    destination_currency: "SGD".to_owned(),
                    source_holding_id: None,
                    destination_holding_id: None,
                    quantity: None,
                    fee_amount: None,
                    fee_kind: None,
                },
            )
            .await
            .expect("transfer");
            rebuild_all(&state, "rebuild-after-transfer").await;
            let result = attribution_at(&state, &t0.to_ymd(), &t1.to_ymd()).await;
            let value = available(&result);
            assert_eq!(value.conversion_spread.amount, "-9.43");
            cleanup(&path);
        });
    }

    #[test]
    fn unconvertible_in_period_flow_is_unavailable_with_no_partial_numbers() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("p6-unconv").await;
            confirm_tz(&state).await;
            let timezone = HistoryTimezone::parse(
                &history_query_service::get_history_origin(&state)
                    .await
                    .expect("origin")
                    .timezone,
            )
            .expect("tz");
            let today = timezone.local_date(&Timestamp::now());
            let last_closed = today.pred().expect("yesterday");
            let t0 = last_closed.checked_add_days(-2).expect("t0");
            let t1 = last_closed;
            let flow_on = last_closed.checked_add_days(-1).expect("flow");
            set_origin_on(&state, &t0.to_ymd()).await;
            let walt = member_id(&state).await;
            let cny = account_service::create_account(
                &state,
                bank_input("CNY Cash", &walt, "1000", "CNY"),
            )
            .await
            .expect("cny");
            let sgd =
                account_service::create_account(&state, bank_input("SGD Cash", &walt, "0", "SGD"))
                    .await
                    .expect("sgd");
            rebuild_all(&state, "rebuild").await;
            create_activity(
                &state,
                CreateActivityInput::Transfer {
                    local_date: flow_on.to_ymd(),
                    local_time: "10:00".to_owned(),
                    ambiguous_offset: None,
                    note: None,
                    source_account_id: cny.id.clone(),
                    source_component: "account_value".to_owned(),
                    source_amount: "100".to_owned(),
                    source_currency: "CNY".to_owned(),
                    destination_account_id: sgd.id.clone(),
                    destination_component: "account_value".to_owned(),
                    destination_amount: "20".to_owned(),
                    destination_currency: "SGD".to_owned(),
                    source_holding_id: None,
                    destination_holding_id: None,
                    quantity: None,
                    fee_amount: None,
                    fee_kind: None,
                },
            )
            .await
            .expect("transfer");
            let result = attribution_at(&state, &t0.to_ymd(), &t1.to_ymd()).await;
            match result {
                NetWorthAttributionDto::Unavailable(value) => {
                    assert_eq!(value.reason, REASON_PERIOD_UNAVAILABLE);
                    assert!(value.unconvertible_flow_count >= 1);
                    assert!(value.blocking_dates.contains(&flow_on.to_ymd()));
                }
                NetWorthAttributionDto::Available(_) => {
                    panic!("expected unavailable, no partial bridge")
                }
            }
            cleanup(&path);
        });
    }
}
