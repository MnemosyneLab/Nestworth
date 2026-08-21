//! Daily-linked TWR and XIRR over closed-day snapshots.
//!
//! Read-only. One consistent transaction. No provider call and no snapshot rebuild.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::Serialize;
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    currency_decomposition::native_to_base_rate,
    history_repositories::{
        self, AccountStateObservationRecord, DailyValuationSnapshotItemRecord,
        DailyValuationSnapshotRecord, FxPreferenceObservationRecord, OriginAccountStateRecord,
    },
    query_count,
    quote_service::{self, FxQuoteRecordDto},
    reference::{begin_read_tx, finish_read_tx, require_household_tx},
};
use crate::{
    domain::{
        checked_add, checked_div, checked_mul, checked_powd, checked_sub, classify_scope_flow,
        endpoint_in_scope, round_to_money_scale, solve_xirr, Activity, ActivityId, AnalyticsScope,
        CalendarDate, ComponentKind, CurrencyCode, Direction, FxPair, HistoryTimezone,
        LegFlowClassification, Money, PrimaryCategory, QuoteSourceKind, ReturnRate,
        ScopeEndpointFacts, ScopeFlowActivity, ScopeFlowLeg, Timestamp, XirrCashflow, XirrError,
    },
    error::AppError,
    state::AppState,
};

pub const METHOD_TWR: &str = "twr";
pub const METHOD_XIRR: &str = "xirr";
pub const FLOW_ASSUMPTION_START_OF_DAY: &str = "startOfDay";
pub const REASON_PERIOD_UNAVAILABLE: &str = "ANALYTICS_PERIOD_UNAVAILABLE";
pub const REASON_NOT_COMPUTABLE: &str = "RETURN_NOT_COMPUTABLE";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TwrResultDto {
    #[serde(rename_all = "camelCase")]
    Available {
        method: String,
        flow_assumption: String,
        cumulative: String,
        annualized: Option<String>,
        skipped_days: i32,
        linked_days: i32,
    },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        reason: String,
        blocking_dates: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum XirrResultDto {
    #[serde(rename_all = "camelCase")]
    Available { method: String, annual_rate: String },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        reason: String,
        blocking_dates: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceSummaryDto {
    pub twr: TwrResultDto,
    pub xirr: XirrResultDto,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedDay {
    pub value_prev: Decimal,
    pub value: Decimal,
    pub flow: Decimal,
    pub attributed_return: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwrChain {
    pub cumulative: Decimal,
    pub skipped_days: i64,
    pub linked_days: i64,
}

pub async fn get_performance_summary(
    state: &AppState,
    scope: AnalyticsScope,
    start_on: &str,
    end_on: &str,
) -> Result<PerformanceSummaryDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_performance_summary_in_tx(&mut tx, scope, start_on, end_on).await;
    finish_read_tx(tx, result).await
}

pub async fn get_performance_summary_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    scope: AnalyticsScope,
    start_on: &str,
    end_on: &str,
) -> Result<PerformanceSummaryDto, AppError> {
    get_performance_summary_at_in_tx(tx, scope, start_on, end_on, &Timestamp::now()).await
}

pub(crate) async fn get_performance_summary_at_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    scope: AnalyticsScope,
    start_on: &str,
    end_on: &str,
    now: &Timestamp,
) -> Result<PerformanceSummaryDto, AppError> {
    query_count::record("return_summary");
    let start_on = CalendarDate::parse(start_on)?;
    let end_on = CalendarDate::parse(end_on)?;
    if end_on < start_on {
        return Err(AppError::validation(
            "endOn",
            "The period end must be on or after the period start.",
        ));
    }
    let Some(series) = load_analytics_period_series(tx, start_on, end_on, now).await? else {
        return Ok(period_unavailable(Vec::new()));
    };
    let first_complete =
        history_repositories::earliest_complete_snapshot_on(tx, &series.household_id)
            .await?
            .filter(|value| !value.is_empty())
            .map(|value| CalendarDate::parse(&value))
            .transpose()?;
    let Some(first_complete) = first_complete else {
        return Ok(period_unavailable(vec![start_on.to_ymd()]));
    };
    if start_on < first_complete {
        return Ok(period_unavailable(vec![start_on.to_ymd()]));
    }

    let required_days = dates_inclusive(series.start_on, series.t1);
    let mut blocking = Vec::new();
    for date in &required_days {
        let key = date.to_ymd();
        match series.snapshots_by_date.get(&key) {
            Some(snapshot) if snapshot.is_complete => {}
            _ => blocking.push(key),
        }
    }
    if !blocking.is_empty() {
        return Ok(period_unavailable(blocking));
    }

    let mut values = Vec::with_capacity(required_days.len());
    for date in &required_days {
        let snapshot = series
            .snapshots_by_date
            .get(&date.to_ymd())
            .ok_or(AppError::Internal)?;
        let cutoff = Timestamp::parse(&snapshot.cutoff_at)?;
        let items = series
            .items_by_snapshot
            .get(&snapshot.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        match scope_value(scope, snapshot, items, &series.membership, &cutoff)? {
            Some(value) => values.push(value),
            None => return Ok(period_unavailable(vec![date.to_ymd()])),
        }
    }

    let mut flows = vec![Decimal::ZERO; required_days.len().saturating_sub(1)];
    let mut attributed = vec![Decimal::ZERO; required_days.len().saturating_sub(1)];
    let date_index: HashMap<String, usize> = required_days
        .iter()
        .enumerate()
        .map(|(index, date)| (date.to_ymd(), index))
        .collect();
    match accumulate_flows(
        scope,
        &series.activities,
        &series.membership,
        &series.fx_quotes,
        &series.fx_observations,
        &series.current_preferences,
        series.base,
        &date_index,
        &mut flows,
        &mut attributed,
    )? {
        None => {}
        Some(unconvertible) => return Ok(period_unavailable(unconvertible)),
    }
    let t1 = series.t1;

    let mut days = Vec::new();
    for (index, flow) in flows.iter().enumerate() {
        days.push(LinkedDay {
            value_prev: values[index],
            value: values[index + 1],
            flow: *flow,
            attributed_return: attributed[index],
        });
    }
    let chain = chain_linked_days(&days)?;
    let period_days = t1
        .as_naive_date()
        .signed_duration_since(start_on.as_naive_date())
        .num_days();
    let twr = available_twr(&chain, period_days)?;

    let mut cashflows = vec![XirrCashflow {
        date: start_on,
        amount: -values[0],
    }];
    for (index, date) in required_days.iter().enumerate().skip(1) {
        let flow_index = index - 1;
        let mut amount = checked_add(-flows[flow_index], attributed[flow_index])?;
        if index + 1 == required_days.len() {
            amount = checked_add(amount, *values.last().ok_or(AppError::Internal)?)?;
        }
        if !amount.is_zero() {
            cashflows.push(XirrCashflow {
                date: *date,
                amount,
            });
        } else if index + 1 == required_days.len() {
            cashflows.push(XirrCashflow {
                date: *date,
                amount: Decimal::ZERO,
            });
        }
    }
    if required_days.len() == 1 {
        cashflows.push(XirrCashflow {
            date: t1,
            amount: values[0],
        });
    }
    let xirr = match solve_xirr(&cashflows) {
        Ok(rate) => available_xirr(rate)?,
        Err(XirrError::NoSignChange | XirrError::NotComputable) => XirrResultDto::Unavailable {
            reason: REASON_NOT_COMPUTABLE.to_owned(),
            blocking_dates: Vec::new(),
        },
    };
    Ok(PerformanceSummaryDto { twr, xirr })
}

pub fn daily_linked_return(
    value_prev: Decimal,
    value: Decimal,
    flow: Decimal,
    attributed_return: Decimal,
) -> Result<Option<Decimal>, AppError> {
    let denominator = checked_add(value_prev, flow)?;
    if denominator <= Decimal::ZERO {
        return Ok(None);
    }
    let numerator = checked_add(
        checked_sub(checked_sub(value, value_prev)?, flow)?,
        attributed_return,
    )?;
    Ok(Some(checked_div(numerator, denominator)?))
}

pub fn chain_daily_returns(daily: &[Decimal]) -> Result<Decimal, AppError> {
    let mut product = Decimal::ONE;
    for rate in daily {
        product = checked_mul(product, checked_add(Decimal::ONE, *rate)?)?;
    }
    checked_sub(product, Decimal::ONE)
}

pub fn chain_linked_days(days: &[LinkedDay]) -> Result<TwrChain, AppError> {
    let mut linked = Vec::new();
    let mut skipped_days = 0;
    for day in days {
        match daily_linked_return(day.value_prev, day.value, day.flow, day.attributed_return)? {
            Some(rate) => linked.push(rate),
            None => skipped_days += 1,
        }
    }
    Ok(TwrChain {
        cumulative: chain_daily_returns(&linked)?,
        skipped_days,
        linked_days: i64::try_from(linked.len()).map_err(|_| AppError::Internal)?,
    })
}

pub fn annualize_return(twr: Decimal, days: i64) -> Result<Option<Decimal>, AppError> {
    if days < 365 {
        return Ok(None);
    }
    let growth = checked_add(Decimal::ONE, twr)?;
    if growth <= Decimal::ZERO {
        return Ok(None);
    }
    let exponent = checked_div(Decimal::from(365), Decimal::from(days))?;
    Ok(Some(checked_sub(
        checked_powd(growth, exponent)?,
        Decimal::ONE,
    )?))
}

fn available_twr(chain: &TwrChain, period_days: i64) -> Result<TwrResultDto, AppError> {
    let cumulative = ReturnRate::from_canonical(chain.cumulative)?;
    let annualized = annualize_return(chain.cumulative, period_days)?
        .map(ReturnRate::from_canonical)
        .transpose()?
        .map(ReturnRate::canonical);
    Ok(TwrResultDto::Available {
        method: METHOD_TWR.to_owned(),
        flow_assumption: FLOW_ASSUMPTION_START_OF_DAY.to_owned(),
        cumulative: cumulative.canonical(),
        annualized,
        skipped_days: i32::try_from(chain.skipped_days).map_err(|_| AppError::Internal)?,
        linked_days: i32::try_from(chain.linked_days).map_err(|_| AppError::Internal)?,
    })
}

fn available_xirr(rate: Decimal) -> Result<XirrResultDto, AppError> {
    let annual_rate = ReturnRate::from_canonical(rate)?;
    Ok(XirrResultDto::Available {
        method: METHOD_XIRR.to_owned(),
        annual_rate: annual_rate.canonical(),
    })
}

fn period_unavailable(blocking_dates: Vec<String>) -> PerformanceSummaryDto {
    let unavailable_twr = TwrResultDto::Unavailable {
        reason: REASON_PERIOD_UNAVAILABLE.to_owned(),
        blocking_dates: blocking_dates.clone(),
    };
    let unavailable_xirr = XirrResultDto::Unavailable {
        reason: REASON_PERIOD_UNAVAILABLE.to_owned(),
        blocking_dates,
    };
    PerformanceSummaryDto {
        twr: unavailable_twr,
        xirr: unavailable_xirr,
    }
}

pub(crate) struct AnalyticsPeriodSeries {
    pub household_id: String,
    pub base: CurrencyCode,
    pub start_on: CalendarDate,
    pub t1: CalendarDate,
    pub snapshots_by_date: HashMap<String, DailyValuationSnapshotRecord>,
    pub items_by_snapshot: HashMap<String, Vec<DailyValuationSnapshotItemRecord>>,
    pub activities: Vec<crate::domain::Activity>,
    pub membership: AccountMembership,
    pub fx_quotes: Vec<FxQuoteRecordDto>,
    pub fx_observations: Vec<FxPreferenceObservationRecord>,
    pub current_preferences: HashMap<FxPair, QuoteSourceKind>,
}

pub(crate) async fn load_analytics_period_series(
    tx: &mut Transaction<'_, Sqlite>,
    start_on: CalendarDate,
    end_on: CalendarDate,
    now: &Timestamp,
) -> Result<Option<AnalyticsPeriodSeries>, AppError> {
    let household = require_household_tx(tx).await?;
    let origin = history_repositories::get_origin_by_household(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    if !origin.timezone_confirmed {
        return Err(AppError::HistoryTimezoneConfirmationRequired);
    }
    let timezone = HistoryTimezone::parse(&origin.timezone)?;
    let today = timezone.local_date(now);
    let Some(last_closed) = today.pred() else {
        return Ok(None);
    };
    let t1 = if end_on < last_closed {
        end_on
    } else {
        last_closed
    };
    if start_on > t1 {
        return Ok(None);
    }
    let snapshots = history_repositories::list_latest_snapshots_in_range(
        tx,
        &household.id,
        &start_on.to_ymd(),
        &t1.to_ymd(),
    )
    .await?;
    let snapshot_ids: Vec<String> = snapshots
        .iter()
        .map(|snapshot| snapshot.id.clone())
        .collect();
    let items = history_repositories::list_snapshot_items_for_ids(tx, &snapshot_ids).await?;
    let mut items_by_snapshot: HashMap<String, Vec<DailyValuationSnapshotItemRecord>> =
        HashMap::new();
    for item in items {
        items_by_snapshot
            .entry(item.snapshot_id.clone())
            .or_default()
            .push(item);
    }
    let snapshots_by_date: HashMap<String, DailyValuationSnapshotRecord> = snapshots
        .into_iter()
        .map(|snapshot| (snapshot.snapshot_on.clone(), snapshot))
        .collect();
    let flow_start = start_on.succ().unwrap_or(start_on);
    let activities = if flow_start <= t1 {
        history_repositories::list_activities_in_local_date_range(
            tx,
            &household.id,
            &flow_start.to_ymd(),
            &t1.to_ymd(),
        )
        .await?
    } else {
        Vec::new()
    };
    let origin_states = history_repositories::list_origin_account_states(tx, &origin.id).await?;
    let observations =
        history_repositories::list_account_state_observations_for_household(tx, &household.id)
            .await?;
    let fx_quotes = quote_service::list_all_fx_quotes(tx, &household.id).await?;
    let fx_observations =
        history_repositories::list_fx_preference_observations(tx, &household.id).await?;
    let current_preferences = quote_service::list_fx_preferences(tx, &household.id)
        .await?
        .into_iter()
        .collect();
    let base = CurrencyCode::parse(&household.base_currency)?;
    Ok(Some(AnalyticsPeriodSeries {
        household_id: household.id,
        base,
        start_on,
        t1,
        snapshots_by_date,
        items_by_snapshot,
        activities,
        membership: AccountMembership {
            origin_states,
            observations,
        },
        fx_quotes,
        fx_observations,
        current_preferences,
    }))
}

pub(crate) struct AccountMembership {
    origin_states: Vec<OriginAccountStateRecord>,
    observations: Vec<AccountStateObservationRecord>,
}

pub(crate) fn scope_value(
    scope: AnalyticsScope,
    snapshot: &DailyValuationSnapshotRecord,
    items: &[DailyValuationSnapshotItemRecord],
    membership: &AccountMembership,
    cutoff: &Timestamp,
) -> Result<Option<Decimal>, AppError> {
    if matches!(scope, AnalyticsScope::Household) {
        return Ok(Some(parse_decimal(&snapshot.net_worth_amount)?));
    }
    let mut total = Decimal::ZERO;
    for item in items {
        let account_id = crate::domain::AccountId::parse(&item.account_id)?;
        let instrument_id = item
            .instrument_id
            .as_deref()
            .map(crate::domain::InstrumentId::parse)
            .transpose()?;
        let facts = endpoint_facts(
            membership,
            account_id,
            instrument_id,
            ComponentKind::parse(&item.component_kind)?,
            cutoff,
        )?;
        if !endpoint_in_scope(scope, &facts) {
            continue;
        }
        let Some(base_amount) = item.base_amount.as_deref() else {
            return Ok(None);
        };
        total = checked_add(total, parse_decimal(base_amount)?)?;
    }
    Ok(Some(total))
}

#[allow(clippy::too_many_arguments)]
fn accumulate_flows(
    scope: AnalyticsScope,
    activities: &[Activity],
    membership: &AccountMembership,
    quotes: &[FxQuoteRecordDto],
    observations: &[FxPreferenceObservationRecord],
    current_preferences: &HashMap<FxPair, QuoteSourceKind>,
    base: CurrencyCode,
    date_index: &HashMap<String, usize>,
    flows: &mut [Decimal],
    attributed: &mut [Decimal],
) -> Result<Option<Vec<String>>, AppError> {
    let excluded = excluded_activity_ids(activities);
    let mut unconvertible = Vec::new();
    for activity in activities {
        if excluded.contains(&activity.id()) {
            continue;
        }
        let date_key = activity.effective_local_date().to_ymd();
        let Some(&date_pos) = date_index.get(&date_key) else {
            continue;
        };
        if date_pos == 0 {
            continue;
        }
        let flow_index = date_pos - 1;
        let cutoff = activity.effective_at();
        let mut legs = Vec::with_capacity(activity.legs().len());
        for leg in activity.legs() {
            let instrument_id = match leg.component() {
                crate::domain::LegComponent::HoldingQuantity { instrument_id, .. } => {
                    Some(*instrument_id)
                }
                _ => None,
            };
            let facts = endpoint_facts(
                membership,
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
        for (index, (leg, classification)) in
            activity.legs().iter().zip(classified.legs()).enumerate()
        {
            let endpoint_in = legs[index].endpoint_in_scope;
            match classification {
                LegFlowClassification::SignedFlow { direction } => {
                    match monetary_base(
                        activity,
                        leg,
                        quotes,
                        observations,
                        current_preferences,
                        base,
                    )? {
                        Some(amount) => {
                            let signed = signed_amount(*direction, amount);
                            flows[flow_index] = checked_add(flows[flow_index], signed)?;
                        }
                        None => unconvertible.push(date_key.clone()),
                    }
                }
                LegFlowClassification::UnknownBasisFlow => {
                    match monetary_base(
                        activity,
                        leg,
                        quotes,
                        observations,
                        current_preferences,
                        base,
                    )? {
                        Some(amount) => {
                            let signed = signed_amount(leg.direction(), amount);
                            flows[flow_index] = checked_add(flows[flow_index], signed)?;
                        }
                        None => unconvertible.push(date_key.clone()),
                    }
                }
                LegFlowClassification::Return if !endpoint_in => {
                    match monetary_base(
                        activity,
                        leg,
                        quotes,
                        observations,
                        current_preferences,
                        base,
                    )? {
                        Some(amount) => {
                            let signed = signed_amount(leg.direction(), amount);
                            attributed[flow_index] = checked_add(attributed[flow_index], signed)?;
                        }
                        None => unconvertible.push(date_key.clone()),
                    }
                }
                _ => {}
            }
        }
    }
    if unconvertible.is_empty() {
        Ok(None)
    } else {
        unconvertible.sort();
        unconvertible.dedup();
        Ok(Some(unconvertible))
    }
}

pub(crate) fn signed_amount(direction: Direction, amount: Decimal) -> Decimal {
    match direction {
        Direction::Increase => amount,
        Direction::Decrease => -amount,
    }
}

pub(crate) fn monetary_base(
    activity: &Activity,
    leg: &crate::domain::ActivityLeg,
    quotes: &[FxQuoteRecordDto],
    observations: &[FxPreferenceObservationRecord],
    current_preferences: &HashMap<FxPair, QuoteSourceKind>,
    base: CurrencyCode,
) -> Result<Option<Decimal>, AppError> {
    let money = match leg.component().money() {
        Ok(amount) => amount,
        Err(_) => match activity
            .legs()
            .iter()
            .find_map(|other| other.component().money().ok())
        {
            Some(amount) => amount,
            None => return Ok(None),
        },
    };
    convert_to_base(
        money,
        base,
        quotes,
        observations,
        current_preferences,
        activity.effective_at(),
    )
}

pub(crate) fn convert_to_base(
    native: Money,
    household_base: CurrencyCode,
    quotes: &[FxQuoteRecordDto],
    observations: &[FxPreferenceObservationRecord],
    current_preferences: &HashMap<FxPair, QuoteSourceKind>,
    cutoff: &Timestamp,
) -> Result<Option<Decimal>, AppError> {
    let Some(rate) = native_to_base_rate(
        quotes,
        observations,
        current_preferences,
        native.currency(),
        household_base,
        cutoff,
    )?
    else {
        return Ok(None);
    };
    Ok(Some(round_to_money_scale(checked_mul(
        native.amount(),
        rate,
    )?)?))
}

pub(crate) fn endpoint_facts(
    membership: &AccountMembership,
    account_id: crate::domain::AccountId,
    instrument_id: Option<crate::domain::InstrumentId>,
    component_kind: ComponentKind,
    cutoff: &Timestamp,
) -> Result<ScopeEndpointFacts, AppError> {
    let key = account_id.to_string();
    let observation = membership
        .observations
        .iter()
        .filter(|row| row.account_id == key)
        .filter(|row| Timestamp::parse(&row.effective_at).is_ok_and(|at| at <= *cutoff))
        .max_by(|left, right| {
            left.effective_at
                .cmp(&right.effective_at)
                .then(left.created_at.cmp(&right.created_at))
                .then(left.id.cmp(&right.id))
        });
    if let Some(row) = observation {
        return Ok(ScopeEndpointFacts {
            account_id,
            instrument_id,
            component_kind,
            included_in_net_worth: row.include_in_net_worth,
            included_in_investment: row.include_in_investment,
            is_liability: PrimaryCategory::parse(&row.primary_category)?
                == PrimaryCategory::Liability,
            is_active: row.archived_at.is_none(),
        });
    }
    if let Some(row) = membership
        .origin_states
        .iter()
        .find(|row| row.account_id == key)
    {
        return Ok(ScopeEndpointFacts {
            account_id,
            instrument_id,
            component_kind,
            included_in_net_worth: row.include_in_net_worth,
            included_in_investment: row.include_in_investment,
            is_liability: PrimaryCategory::parse(&row.primary_category)?
                == PrimaryCategory::Liability,
            is_active: row.archived_at.is_none(),
        });
    }
    Ok(ScopeEndpointFacts {
        account_id,
        instrument_id,
        component_kind,
        included_in_net_worth: false,
        included_in_investment: false,
        is_liability: false,
        is_active: false,
    })
}

pub(crate) fn excluded_activity_ids(activities: &[Activity]) -> HashSet<ActivityId> {
    let mut excluded = HashSet::new();
    for activity in activities {
        if let Some(original) = activity.reverses() {
            excluded.insert(original);
            excluded.insert(activity.id());
        }
    }
    excluded
}

pub(crate) fn dates_inclusive(start: CalendarDate, end: CalendarDate) -> Vec<CalendarDate> {
    let mut dates = Vec::new();
    let mut current = start;
    while current <= end {
        dates.push(current);
        match current.succ() {
            Some(next) => current = next,
            None => break,
        }
    }
    dates
}

pub(crate) fn parse_decimal(value: &str) -> Result<Decimal, AppError> {
    Decimal::from_str(value).map_err(|_| AppError::Internal)
}

#[cfg(test)]
mod tests {
    use super::{
        annualize_return, available_xirr, chain_daily_returns, chain_linked_days,
        daily_linked_return, get_performance_summary, get_performance_summary_at_in_tx, LinkedDay,
        TwrResultDto, XirrResultDto, FLOW_ASSUMPTION_START_OF_DAY, METHOD_TWR, METHOD_XIRR,
        REASON_NOT_COMPUTABLE, REASON_PERIOD_UNAVAILABLE,
    };
    use crate::{
        application::{
            query_count,
            reference::{begin_read_tx, finish_read_tx},
        },
        domain::{
            solve_xirr, AnalyticsScope, CalendarDate, HistoryTimezone, ReturnRate, Timestamp,
            XirrCashflow,
        },
        infrastructure::{database::connect_writable, database_bootstrap::MIGRATOR},
        state::AppState,
        test_support::{cleanup, test_path},
    };
    use rust_decimal::Decimal;
    use std::fs;
    use std::path::PathBuf;
    use std::str::FromStr;

    const QQQ: &str = "20202020-2020-4202-8202-202020202020";
    const BROKERAGE: &str = "99999999-9999-4999-8999-999999999999";

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).expect("decimal")
    }

    fn date(value: &str) -> CalendarDate {
        CalendarDate::parse(value).expect("date")
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
        let path = test_path("v014-p5-ret", name);
        let _ = fs::remove_file(&path);
        apply_migrations(&path, &[1, 2, 3]).await;
        load_sql(&path, include_str!("../../test-fixtures/v0.1.3.sql")).await;
        let state = AppState::initialize(path.clone()).await;
        (state, path)
    }

    fn contract_days() -> Vec<LinkedDay> {
        vec![
            LinkedDay {
                value_prev: dec("100000"),
                value: dec("112200"),
                flow: dec("10000"),
                attributed_return: Decimal::ZERO,
            },
            LinkedDay {
                value_prev: dec("112200"),
                value: dec("114444"),
                flow: Decimal::ZERO,
                attributed_return: Decimal::ZERO,
            },
        ]
    }

    #[test]
    fn contract_twr_chains_two_percent_days_to_four_point_zero_four() {
        let day_one =
            daily_linked_return(dec("100000"), dec("112200"), dec("10000"), Decimal::ZERO)
                .expect("day1")
                .expect("linked");
        let day_two =
            daily_linked_return(dec("112200"), dec("114444"), Decimal::ZERO, Decimal::ZERO)
                .expect("day2")
                .expect("linked");
        assert_eq!(day_one, dec("0.02"));
        assert_eq!(day_two, dec("0.02"));
        let chained = chain_daily_returns(&[day_one, day_two]).expect("chain");
        assert_eq!(
            ReturnRate::from_canonical(chained)
                .expect("rate")
                .canonical(),
            "0.0404"
        );
        let from_days = chain_linked_days(&contract_days()).expect("days");
        assert_eq!(
            ReturnRate::from_canonical(from_days.cumulative)
                .expect("rate")
                .canonical(),
            "0.0404"
        );
        assert_eq!(from_days.skipped_days, 0);
        assert_eq!(from_days.linked_days, 2);
    }

    #[test]
    fn four_scopes_share_one_chaining_function() {
        let days = contract_days();
        let expected = chain_linked_days(&days).expect("chain");
        for _scope in [
            AnalyticsScope::Household,
            AnalyticsScope::Portfolio,
            AnalyticsScope::Account(crate::domain::AccountId::parse(BROKERAGE).expect("acct")),
            AnalyticsScope::Instrument(crate::domain::InstrumentId::parse(QQQ).expect("qqq")),
        ] {
            let got = chain_linked_days(&days).expect("shared");
            assert_eq!(got, expected);
        }
    }

    #[test]
    fn attributed_dividend_exceeds_price_only_return() {
        let price_only = chain_linked_days(&[LinkedDay {
            value_prev: dec("14490"),
            value: dec("14490"),
            flow: Decimal::ZERO,
            attributed_return: Decimal::ZERO,
        }])
        .expect("price");
        let with_dividend = chain_linked_days(&[LinkedDay {
            value_prev: dec("14490"),
            value: dec("14490"),
            flow: Decimal::ZERO,
            attributed_return: dec("69"),
        }])
        .expect("total");
        assert_eq!(price_only.cumulative, Decimal::ZERO);
        assert!(with_dividend.cumulative > price_only.cumulative);
    }

    #[test]
    fn zero_and_negative_denominators_are_skipped_not_zero_return() {
        let skipped = chain_linked_days(&[
            LinkedDay {
                value_prev: Decimal::ZERO,
                value: dec("10"),
                flow: Decimal::ZERO,
                attributed_return: Decimal::ZERO,
            },
            LinkedDay {
                value_prev: dec("5"),
                value: dec("6"),
                flow: dec("-5"),
                attributed_return: Decimal::ZERO,
            },
            LinkedDay {
                value_prev: dec("100"),
                value: dec("102"),
                flow: Decimal::ZERO,
                attributed_return: Decimal::ZERO,
            },
        ])
        .expect("skip");
        assert_eq!(skipped.skipped_days, 2);
        assert_eq!(skipped.linked_days, 1);
        assert_eq!(skipped.cumulative, dec("0.02"));
        assert_ne!(skipped.cumulative, Decimal::ZERO);
    }

    #[test]
    fn annualization_is_withheld_below_365_days_and_present_at_365() {
        let twr = dec("0.0404");
        assert_eq!(annualize_return(twr, 364).expect("short"), None);
        let annualized = annualize_return(twr, 365).expect("year").expect("present");
        assert_eq!(
            ReturnRate::from_canonical(annualized)
                .expect("rate")
                .canonical(),
            "0.0404"
        );
    }

    #[test]
    fn naive_simple_change_is_absent_from_return_modules() {
        let percent = ["4.", "444%"].concat();
        let fraction = ["0.0", "4444"].concat();
        for source in [
            include_str!("return_service.rs"),
            include_str!("../domain/xirr.rs"),
        ] {
            assert!(!source.contains(&percent));
            assert!(!source.contains(&fraction));
        }
    }

    #[test]
    fn return_modules_do_not_use_binary_floats() {
        for source in [
            include_str!("return_service.rs"),
            include_str!("../domain/xirr.rs"),
        ] {
            let production = source.split("#[cfg(test)]").next().expect("code");
            assert!(!production.contains("f32"));
            assert!(!production.contains("f64"));
        }
    }

    #[test]
    fn incomplete_snapshot_days_make_the_period_unavailable() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("incomplete-days").await;
            let summary = get_performance_summary(
                &state,
                AnalyticsScope::Household,
                "2026-01-02",
                "2026-01-04",
            )
            .await
            .expect("summary");
            match summary.twr {
                TwrResultDto::Unavailable {
                    reason,
                    blocking_dates,
                } => {
                    assert_eq!(reason, REASON_PERIOD_UNAVAILABLE);
                    assert!(blocking_dates.contains(&"2026-01-03".to_owned()));
                    assert!(blocking_dates.contains(&"2026-01-04".to_owned()));
                    assert!(!blocking_dates.contains(&"2026-01-02".to_owned()));
                }
                TwrResultDto::Available { .. } => panic!("expected unavailable TWR"),
            }
            match summary.xirr {
                XirrResultDto::Unavailable {
                    reason,
                    blocking_dates,
                } => {
                    assert_eq!(reason, REASON_PERIOD_UNAVAILABLE);
                    assert!(blocking_dates.contains(&"2026-01-03".to_owned()));
                    assert!(blocking_dates.contains(&"2026-01-04".to_owned()));
                }
                XirrResultDto::Available { .. } => panic!("expected unavailable XIRR"),
            }
            cleanup(&path);
        });
    }

    #[test]
    fn period_before_first_complete_snapshot_is_unavailable() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("before-first").await;
            let summary = get_performance_summary(
                &state,
                AnalyticsScope::Household,
                "2026-01-01",
                "2026-01-02",
            )
            .await
            .expect("summary");
            match summary.twr {
                TwrResultDto::Unavailable { reason, .. } => {
                    assert_eq!(reason, REASON_PERIOD_UNAVAILABLE);
                }
                TwrResultDto::Available { .. } => panic!("expected unavailable"),
            }
            cleanup(&path);
        });
    }

    #[test]
    fn current_local_day_is_never_a_blocking_date() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("exclude-today").await;
            let timezone = HistoryTimezone::parse("Asia/Singapore").expect("tz");
            let today = timezone.local_date(&Timestamp::now());
            let summary = get_performance_summary(
                &state,
                AnalyticsScope::Household,
                "2026-01-02",
                &today.to_ymd(),
            )
            .await
            .expect("summary");
            match summary.twr {
                TwrResultDto::Unavailable { blocking_dates, .. } => {
                    assert!(!blocking_dates.contains(&today.to_ymd()));
                    assert!(blocking_dates.contains(&"2026-01-03".to_owned()));
                    assert!(blocking_dates.contains(&"2026-01-04".to_owned()));
                }
                TwrResultDto::Available { .. } => panic!("expected unavailable"),
            }
            cleanup(&path);
        });
    }

    #[test]
    fn snapshot_items_and_activities_are_batched() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("batch-queries").await;
            let (summary, families) = query_count::capture_async(|| {
                get_performance_summary(
                    &state,
                    AnalyticsScope::Portfolio,
                    "2026-01-02",
                    "2026-01-04",
                )
            })
            .await;
            summary.expect("summary");
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
    fn return_read_writes_nothing() {
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
            let before_items: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM daily_valuation_snapshot_items")
                    .fetch_one(db)
                    .await
                    .expect("count");
            get_performance_summary(
                &state,
                AnalyticsScope::Household,
                "2026-01-02",
                "2026-01-04",
            )
            .await
            .expect("summary");
            let after_activities: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
                .fetch_one(db)
                .await
                .expect("count");
            let after_snapshots: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM daily_valuation_snapshots")
                    .fetch_one(db)
                    .await
                    .expect("count");
            let after_items: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM daily_valuation_snapshot_items")
                    .fetch_one(db)
                    .await
                    .expect("count");
            assert_eq!(before_activities, after_activities);
            assert_eq!(before_snapshots, after_snapshots);
            assert_eq!(before_items, after_items);
            cleanup(&path);
        });
    }

    #[test]
    fn same_day_complete_snapshot_is_computable_and_excludes_live_today() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("same-day").await;
            let db = state.writable_db().expect("db");
            let mut tx = begin_read_tx(db).await.expect("tx");
            let now = Timestamp::parse("2026-08-19T04:00:00.000Z").expect("now");
            let result = get_performance_summary_at_in_tx(
                &mut tx,
                AnalyticsScope::Household,
                "2026-01-02",
                "2026-01-02",
                &now,
            )
            .await;
            let summary = finish_read_tx(tx, result).await.expect("summary");
            match summary.twr {
                TwrResultDto::Available {
                    method,
                    flow_assumption,
                    cumulative,
                    annualized,
                    skipped_days,
                    linked_days,
                } => {
                    assert_eq!(method, METHOD_TWR);
                    assert_eq!(flow_assumption, FLOW_ASSUMPTION_START_OF_DAY);
                    assert_eq!(cumulative, "0");
                    assert_eq!(annualized, None);
                    assert_eq!(skipped_days, 0);
                    assert_eq!(linked_days, 0);
                }
                TwrResultDto::Unavailable {
                    reason,
                    blocking_dates,
                } => {
                    panic!("expected available TWR, got {reason} {blocking_dates:?}");
                }
            }
            match summary.xirr {
                XirrResultDto::Available { annual_rate, .. } => {
                    assert_eq!(annual_rate, "0");
                }
                XirrResultDto::Unavailable {
                    reason,
                    blocking_dates,
                } => {
                    panic!("expected available XIRR, got {reason} {blocking_dates:?}");
                }
            }
            cleanup(&path);
        });
    }

    #[test]
    fn xirr_unit_contract_rates_round_once() {
        let ten = solve_xirr(&[
            XirrCashflow {
                date: date("2020-01-01"),
                amount: dec("-100000"),
            },
            XirrCashflow {
                date: date("2020-12-31"),
                amount: dec("110000"),
            },
        ])
        .expect("ten");
        assert_eq!(
            ReturnRate::from_canonical(ten).expect("r").canonical(),
            "0.1"
        );
        let two = solve_xirr(&[
            XirrCashflow {
                date: date("2020-01-01"),
                amount: dec("-100000"),
            },
            XirrCashflow {
                date: date("2020-12-31"),
                amount: dec("-100000"),
            },
            XirrCashflow {
                date: date("2021-12-31"),
                amount: dec("230000"),
            },
        ])
        .expect("two");
        assert_eq!(
            ReturnRate::from_canonical(two).expect("r").canonical(),
            "0.096872"
        );
        assert_eq!(METHOD_XIRR, "xirr");
        assert_eq!(REASON_NOT_COMPUTABLE, "RETURN_NOT_COMPUTABLE");
    }

    #[test]
    fn xirr_available_dto_exposes_solver_annual_rate_without_reannualizing() {
        let two_year = solve_xirr(&[
            XirrCashflow {
                date: date("2020-01-01"),
                amount: dec("-100"),
            },
            XirrCashflow {
                date: date("2021-12-31"),
                amount: dec("121"),
            },
        ])
        .expect("two-year");
        let dto = available_xirr(two_year).expect("dto");
        match dto {
            XirrResultDto::Available {
                method,
                annual_rate,
            } => {
                assert_eq!(method, METHOD_XIRR);
                assert_eq!(annual_rate, "0.1");
            }
            XirrResultDto::Unavailable { reason, .. } => {
                panic!("expected available XIRR, got {reason}")
            }
        }
        assert!(annualize_return(two_year, 730)
            .expect("annualize")
            .is_some());
        let one_year = solve_xirr(&[
            XirrCashflow {
                date: date("2020-01-01"),
                amount: dec("-100"),
            },
            XirrCashflow {
                date: date("2020-12-31"),
                amount: dec("110"),
            },
        ])
        .expect("one-year");
        match available_xirr(one_year).expect("one") {
            XirrResultDto::Available { annual_rate, .. } => assert_eq!(annual_rate, "0.1"),
            XirrResultDto::Unavailable { reason, .. } => {
                panic!("expected available XIRR, got {reason}")
            }
        }
    }
}
