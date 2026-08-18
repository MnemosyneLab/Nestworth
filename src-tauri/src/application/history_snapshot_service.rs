//! Bounded closed-day snapshot rebuild, history status, and net-worth trend.

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    account_service::{self, MoneyDto},
    historical_valuation_service::{self, HistoricalValuation},
    history_origin::ensure_activity_writes_allowed,
    history_repositories::{
        self, DailyValuationSnapshotItemRecord, DailyValuationSnapshotRecord, SnapshotStateRecord,
    },
    query_count,
    reference::{
        begin_read_tx, begin_write_tx, finish_read_tx, finish_write_tx, require_household_tx,
    },
    valuation_service::{self, ValuationSnapshot},
};
use crate::{
    domain::{
        canonical_decimal, closed_day_cutoff, round_to_money_scale, CalendarDate, HistoryTimezone,
        Money, Timestamp, TrackingMode, ValuationSnapshotId, ValuationSnapshotItemId, TOTAL_BPS,
    },
    error::AppError,
    state::AppState,
};

pub const MAX_REBUILD_DAYS: i64 = 366;

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RebuildHistorySnapshotsInput {
    #[serde(default)]
    pub cancel: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RebuildHistorySnapshotsResultDto {
    pub processed_days: i32,
    pub remaining: bool,
    pub cancelled: bool,
    pub dirty_from: Option<String>,
    pub last_completed_on: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStatusDto {
    pub timezone: String,
    pub timezone_confirmed: bool,
    pub origin_at: String,
    pub origin_local_date: String,
    pub dirty_from: Option<String>,
    pub last_completed_on: Option<String>,
    pub last_closed_on: Option<String>,
    pub rebuild_status: String,
    pub rebuild_cursor_on: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GetNetWorthTrendInput {
    pub range: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetWorthTrendDto {
    pub base_currency: String,
    pub range: String,
    pub origin_local_date: String,
    pub dirty_from: Option<String>,
    pub points: Vec<NetWorthTrendPointDto>,
    pub current: NetWorthTrendPointDto,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetWorthTrendPointDto {
    pub date: Option<String>,
    pub as_of: String,
    pub assets: MoneyDto,
    pub liabilities: MoneyDto,
    pub net_worth: MoneyDto,
    pub is_complete: bool,
    pub is_live: bool,
    pub coverage_bps: i32,
    pub missing_count: i32,
    pub valued_component_count: i32,
    pub total_component_count: i32,
}

struct PlannedSnapshot {
    snapshot_on: String,
    cutoff_at: String,
    historical: HistoricalValuation,
    snapshot: ValuationSnapshot,
}

pub async fn get_history_status(state: &AppState) -> Result<HistoryStatusDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_history_status_in_tx(&mut tx).await;
    finish_read_tx(tx, result).await
}

pub async fn get_net_worth_trend(
    state: &AppState,
    input: GetNetWorthTrendInput,
) -> Result<NetWorthTrendDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_net_worth_trend_in_tx(&mut tx, &input.range).await;
    finish_read_tx(tx, result).await
}

pub async fn rebuild_history_snapshots(
    state: &AppState,
    input: RebuildHistorySnapshotsInput,
) -> Result<RebuildHistorySnapshotsResultDto, AppError> {
    rebuild_history_snapshots_bounded(state, input.cancel, MAX_REBUILD_DAYS).await
}

pub async fn rebuild_history_snapshots_bounded(
    state: &AppState,
    cancel: bool,
    max_days: i64,
) -> Result<RebuildHistorySnapshotsResultDto, AppError> {
    let max_days = max_days.clamp(0, MAX_REBUILD_DAYS);
    let database = state.writable_db()?;
    if cancel {
        let mut tx = begin_write_tx(database).await?;
        let result = cancel_rebuild_in_tx(&mut tx).await;
        return finish_write_tx(tx, result).await;
    }
    ensure_activity_writes_allowed_state(state).await?;
    let started = std::time::Instant::now();
    let mut read_tx = begin_read_tx(database).await?;
    let planned = match load_planned_snapshots(&mut read_tx, max_days).await {
        Ok(planned) => {
            let _ = finish_read_tx(read_tx, Ok(())).await;
            planned
        }
        Err(error) => {
            let _ = finish_read_tx::<()>(read_tx, Err(AppError::Internal)).await;
            mark_rebuild_failed(state).await?;
            return Err(map_rebuild_error(error));
        }
    };

    let mut processed = 0_i32;
    if !planned.is_empty() {
        let mut tx = begin_write_tx(database).await?;
        let marked = set_rebuilding_in_tx(&mut tx).await;
        if let Err(error) = finish_write_tx(tx, marked).await {
            mark_rebuild_failed(state).await?;
            return Err(map_rebuild_error(error));
        }
    }
    for day in &planned {
        if rebuild_was_cancelled(state).await? {
            return cancelled_progress(state, processed).await;
        }
        let mut write_tx = begin_write_tx(database).await?;
        let persist = persist_snapshot_revision(&mut write_tx, day).await;
        match finish_write_tx(write_tx, persist).await {
            Ok(()) => processed += 1,
            Err(error) => {
                mark_rebuild_failed(state).await?;
                return Err(map_rebuild_error(error));
            }
        }
    }

    let mut tx = begin_write_tx(database).await?;
    let result = finalize_rebuild_in_tx(&mut tx, processed, planned.len(), max_days).await;
    let result = finish_write_tx(tx, result).await?;
    tracing::info!(
        event = "history.rebuild",
        days = processed,
        duration_ms = started.elapsed().as_millis() as u64,
        "history snapshots rebuilt"
    );
    Ok(result)
}

async fn ensure_activity_writes_allowed_state(state: &AppState) -> Result<(), AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = ensure_activity_writes_allowed(&mut tx).await.map(|_| ());
    finish_read_tx(tx, result).await
}

async fn load_planned_snapshots(
    tx: &mut Transaction<'_, Sqlite>,
    max_days: i64,
) -> Result<Vec<PlannedSnapshot>, AppError> {
    query_count::record("rebuild_plan");
    let household = require_household_tx(tx).await?;
    let origin = history_repositories::get_origin_by_household(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    if !origin.timezone_confirmed {
        return Err(AppError::HistoryTimezoneConfirmationRequired);
    }
    let timezone = HistoryTimezone::parse(&origin.timezone)?;
    let state = history_repositories::get_snapshot_state(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    let Some(start) = state.dirty_from.as_deref() else {
        return Ok(Vec::new());
    };
    let start = CalendarDate::parse(start)?;
    let origin_date = CalendarDate::parse(&origin.origin_local_date)?;
    let start = if start < origin_date {
        origin_date
    } else {
        start
    };
    let now = Timestamp::now();
    let today = timezone.local_date(&now);
    let Some(last_closed) = today.pred() else {
        return Ok(Vec::new());
    };
    if start > last_closed {
        return Ok(Vec::new());
    }
    let mut days = Vec::new();
    let mut cursor = start;
    while days.len() < usize::try_from(max_days).unwrap_or(0) && cursor <= last_closed {
        days.push(cursor);
        let Some(next) = cursor.succ() else {
            break;
        };
        cursor = next;
    }
    let mut planned = Vec::with_capacity(days.len());
    for date in days {
        let cutoff_at = closed_day_cutoff(timezone, date)?;
        let (historical, snapshot) =
            historical_valuation_service::reconstruct_closed_day(tx, timezone, date).await?;
        planned.push(PlannedSnapshot {
            snapshot_on: date.to_ymd(),
            cutoff_at: cutoff_at.to_rfc3339(),
            historical,
            snapshot,
        });
    }
    Ok(planned)
}

async fn persist_snapshot_revision(
    tx: &mut Transaction<'_, Sqlite>,
    day: &PlannedSnapshot,
) -> Result<(), AppError> {
    query_count::record("snapshot_write");
    let household = require_household_tx(tx).await?;
    let previous =
        history_repositories::latest_snapshot_for_date(tx, &household.id, &day.snapshot_on).await?;
    let revision = previous.as_ref().map(|row| row.revision + 1).unwrap_or(1);
    let items = snapshot_items(&day.historical, &day.snapshot)?;
    let valued = items.iter().filter(|item| item.is_complete).count();
    let total = items.len();
    let coverage_bps = if total == 0 {
        i64::from(TOTAL_BPS)
    } else {
        i64::try_from(valued).unwrap_or(0) * i64::from(TOTAL_BPS)
            / i64::try_from(total).unwrap_or(1)
    };
    let currency = day.historical.base_currency;
    let assets = day.historical.totals.rounded_assets(currency)?;
    let liabilities = day.historical.totals.rounded_liabilities(currency)?;
    let net_worth = day.historical.totals.rounded_net_worth(currency)?;
    let created_at = Timestamp::now().to_rfc3339();
    let header = DailyValuationSnapshotRecord {
        id: ValuationSnapshotId::new().to_string(),
        household_id: household.id.clone(),
        snapshot_on: day.snapshot_on.clone(),
        cutoff_at: day.cutoff_at.clone(),
        revision,
        supersedes_snapshot_id: previous.map(|row| row.id),
        assets_amount: assets.amount,
        liabilities_amount: liabilities.amount,
        net_worth_amount: net_worth.amount,
        currency: currency.as_str().to_owned(),
        is_complete: day.historical.totals.complete,
        valued_component_count: i64::try_from(valued).unwrap_or(0),
        total_component_count: i64::try_from(total).unwrap_or(0),
        coverage_bps,
        generation_reason: "rebuild".to_owned(),
        created_at: created_at.clone(),
    };
    history_repositories::insert_daily_valuation_snapshot(tx, &header).await?;
    for (sort_order, item) in items.into_iter().enumerate() {
        let mut row = item;
        row.snapshot_id = header.id.clone();
        row.sort_order = i64::try_from(sort_order).unwrap_or(0);
        history_repositories::insert_daily_valuation_snapshot_item(tx, &row).await?;
    }
    let next_dirty = CalendarDate::parse(&day.snapshot_on)?
        .succ()
        .map(|date| date.to_ymd());
    history_repositories::upsert_snapshot_state(
        tx,
        &SnapshotStateRecord {
            household_id: household.id,
            dirty_from: next_dirty,
            last_completed_on: Some(day.snapshot_on.clone()),
            rebuild_status: "running".to_owned(),
            rebuild_cursor_on: Some(day.snapshot_on.clone()),
            updated_at: created_at,
        },
    )
    .await?;
    Ok(())
}

async fn finalize_rebuild_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    processed: i32,
    planned: usize,
    max_days: i64,
) -> Result<RebuildHistorySnapshotsResultDto, AppError> {
    let household = require_household_tx(tx).await?;
    let mut state = history_repositories::get_snapshot_state(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    let _ = (planned, max_days);
    state.rebuild_status = "idle".to_owned();
    state.rebuild_cursor_on = None;
    state.updated_at = Timestamp::now().to_rfc3339();
    history_repositories::upsert_snapshot_state(tx, &state).await?;
    Ok(RebuildHistorySnapshotsResultDto {
        processed_days: processed,
        remaining: state.dirty_from.is_some(),
        cancelled: false,
        dirty_from: state.dirty_from,
        last_completed_on: state.last_completed_on,
        status: "idle".to_owned(),
    })
}

async fn cancel_rebuild_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<RebuildHistorySnapshotsResultDto, AppError> {
    let household = require_household_tx(tx).await?;
    let mut state = history_repositories::get_snapshot_state(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    state.rebuild_status = "idle".to_owned();
    state.updated_at = Timestamp::now().to_rfc3339();
    history_repositories::upsert_snapshot_state(tx, &state).await?;
    tracing::info!(
        event = "history.rebuild_cancelled",
        "snapshot rebuild cancelled"
    );
    Ok(RebuildHistorySnapshotsResultDto {
        processed_days: 0,
        remaining: state.dirty_from.is_some(),
        cancelled: true,
        dirty_from: state.dirty_from,
        last_completed_on: state.last_completed_on,
        status: "cancelled".to_owned(),
    })
}

async fn mark_rebuild_failed(state: &AppState) -> Result<(), AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        if let Some(mut snapshot_state) =
            history_repositories::get_snapshot_state(&mut tx, &household.id).await?
        {
            snapshot_state.rebuild_status = "failed".to_owned();
            snapshot_state.updated_at = Timestamp::now().to_rfc3339();
            history_repositories::upsert_snapshot_state(&mut tx, &snapshot_state).await?;
        }
        Ok(())
    }
    .await;
    finish_write_tx(tx, result).await
}

async fn set_rebuilding_in_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<(), AppError> {
    let household = require_household_tx(tx).await?;
    let mut state = history_repositories::get_snapshot_state(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    state.rebuild_status = "running".to_owned();
    state.updated_at = Timestamp::now().to_rfc3339();
    history_repositories::upsert_snapshot_state(tx, &state).await
}

async fn rebuild_was_cancelled(state: &AppState) -> Result<bool, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let snapshot_state = history_repositories::get_snapshot_state(&mut tx, &household.id)
            .await?
            .ok_or(AppError::HistoryInitializationFailed)?;
        Ok(snapshot_state.rebuild_status != "running")
    }
    .await;
    finish_read_tx(tx, result).await
}

async fn cancelled_progress(
    state: &AppState,
    processed: i32,
) -> Result<RebuildHistorySnapshotsResultDto, AppError> {
    let status = get_history_status(state).await?;
    Ok(RebuildHistorySnapshotsResultDto {
        processed_days: processed,
        remaining: status.dirty_from.is_some(),
        cancelled: true,
        dirty_from: status.dirty_from,
        last_completed_on: status.last_completed_on,
        status: "cancelled".to_owned(),
    })
}

fn map_rebuild_error(error: AppError) -> AppError {
    match error {
        AppError::HistoryTimezoneConfirmationRequired
        | AppError::HistoryInitializationFailed
        | AppError::SnapshotRebuildRequired => error,
        _ => AppError::SnapshotRebuildFailed,
    }
}

async fn get_history_status_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<HistoryStatusDto, AppError> {
    let household = require_household_tx(tx).await?;
    let origin = history_repositories::get_origin_by_household(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    let timezone = HistoryTimezone::parse(&origin.timezone)?;
    let state = history_repositories::get_snapshot_state(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    let last_closed_on = timezone
        .local_date(&Timestamp::now())
        .pred()
        .map(|date| date.to_ymd());
    Ok(HistoryStatusDto {
        timezone: origin.timezone,
        timezone_confirmed: origin.timezone_confirmed,
        origin_at: origin.origin_at,
        origin_local_date: origin.origin_local_date,
        dirty_from: state.dirty_from,
        last_completed_on: state.last_completed_on,
        last_closed_on,
        rebuild_status: state.rebuild_status,
        rebuild_cursor_on: state.rebuild_cursor_on,
    })
}

async fn get_net_worth_trend_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    range: &str,
) -> Result<NetWorthTrendDto, AppError> {
    query_count::record("trend");
    let household = require_household_tx(tx).await?;
    let origin = history_repositories::get_origin_by_household(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    let timezone = HistoryTimezone::parse(&origin.timezone)?;
    let now = Timestamp::now();
    let today = timezone.local_date(&now);
    let origin_date = CalendarDate::parse(&origin.origin_local_date)?;
    let last_closed = today.pred();
    let start = trend_start(range, origin_date, today)?;
    let end = last_closed.unwrap_or(origin_date);
    let start_on = if start < origin_date {
        origin_date.to_ymd()
    } else {
        start.to_ymd()
    };
    let end_on = if end < origin_date {
        origin_date.to_ymd()
    } else {
        end.to_ymd()
    };
    let snapshots = if start_on <= end_on {
        history_repositories::list_latest_snapshots_in_range(tx, &household.id, &start_on, &end_on)
            .await?
    } else {
        Vec::new()
    };
    let state = history_repositories::get_snapshot_state(tx, &household.id).await?;
    let points = snapshots
        .into_iter()
        .map(trend_point_from_snapshot)
        .collect();
    let current = live_current_point(tx, &household.base_currency, &now).await?;
    Ok(NetWorthTrendDto {
        base_currency: household.base_currency,
        range: range.to_owned(),
        origin_local_date: origin.origin_local_date,
        dirty_from: state.and_then(|row| row.dirty_from),
        points,
        current,
    })
}

fn trend_start(
    range: &str,
    origin_date: CalendarDate,
    today: CalendarDate,
) -> Result<CalendarDate, AppError> {
    let days = match range {
        "1m" => 30,
        "3m" => 90,
        "1y" => 365,
        "all" => return Ok(origin_date),
        _ => {
            return Err(AppError::validation(
                "range",
                "Trend range must be 1m, 3m, 1y, or all.",
            ))
        }
    };
    Ok(today
        .checked_add_days(-days)
        .unwrap_or(origin_date)
        .max(origin_date))
}

fn trend_point_from_snapshot(row: DailyValuationSnapshotRecord) -> NetWorthTrendPointDto {
    let missing = (row.total_component_count - row.valued_component_count).max(0);
    NetWorthTrendPointDto {
        date: Some(row.snapshot_on),
        as_of: row.cutoff_at,
        assets: MoneyDto {
            amount: row.assets_amount,
            currency: row.currency.clone(),
        },
        liabilities: MoneyDto {
            amount: row.liabilities_amount,
            currency: row.currency.clone(),
        },
        net_worth: MoneyDto {
            amount: row.net_worth_amount,
            currency: row.currency,
        },
        is_complete: row.is_complete,
        is_live: false,
        coverage_bps: i32::try_from(row.coverage_bps).unwrap_or(0),
        missing_count: i32::try_from(missing).unwrap_or(0),
        valued_component_count: i32::try_from(row.valued_component_count).unwrap_or(0),
        total_component_count: i32::try_from(row.total_component_count).unwrap_or(0),
    }
}

async fn live_current_point(
    tx: &mut Transaction<'_, Sqlite>,
    base_currency: &str,
    now: &Timestamp,
) -> Result<NetWorthTrendPointDto, AppError> {
    query_count::record("trend_live");
    let household = require_household_tx(tx).await?;
    let accounts = account_service::list_account_records_in_tx(tx, &household.id, false).await?;
    let snapshot = ValuationSnapshot::load(tx, &household.id, base_currency).await?;
    let totals = valuation_service::household_totals(&snapshot, &accounts, now)?;
    let currency = crate::domain::CurrencyCode::parse(base_currency)?;
    let (valued, total) = component_coverage(&snapshot, &accounts, now)?;
    let missing = total.saturating_sub(valued);
    let coverage_bps = if total == 0 {
        TOTAL_BPS
    } else {
        i32::try_from(i64::from(valued) * i64::from(TOTAL_BPS) / i64::from(total)).unwrap_or(0)
    };
    Ok(NetWorthTrendPointDto {
        date: None,
        as_of: now.to_rfc3339(),
        assets: totals.rounded_assets(currency)?,
        liabilities: totals.rounded_liabilities(currency)?,
        net_worth: totals.rounded_net_worth(currency)?,
        is_complete: totals.complete,
        is_live: true,
        coverage_bps,
        missing_count: missing,
        valued_component_count: valued,
        total_component_count: total,
    })
}

fn component_coverage(
    snapshot: &ValuationSnapshot,
    accounts: &[account_service::AccountRecordDto],
    now: &Timestamp,
) -> Result<(i32, i32), AppError> {
    let mut valued = 0_i32;
    let mut total = 0_i32;
    for account in accounts {
        if account.archived_at.is_some() || !account.include_in_net_worth {
            continue;
        }
        if account.tracking_mode == TrackingMode::Holdings.as_str() {
            for holding in valuation_service::snapshot_holdings(snapshot)
                .iter()
                .filter(|holding| holding.account_id == account.id)
            {
                total += 1;
                if valuation_service::calculate_holding(snapshot, holding, now)?.complete {
                    valued += 1;
                }
            }
            for cash in valuation_service::snapshot_cash(snapshot)
                .iter()
                .filter(|cash| cash.account_id == account.id)
            {
                total += 1;
                let native = Money::parse(
                    &cash.amount,
                    crate::domain::CurrencyCode::parse(&cash.currency)?,
                )?;
                if valuation_service::convert_amount(snapshot, native, now)?.complete {
                    valued += 1;
                }
            }
        } else {
            total += 1;
            if valuation_service::value_account_calculation(snapshot, account, now)?.complete {
                valued += 1;
            }
        }
    }
    Ok((valued, total))
}

fn snapshot_items(
    historical: &HistoricalValuation,
    snapshot: &ValuationSnapshot,
) -> Result<Vec<DailyValuationSnapshotItemRecord>, AppError> {
    let mut items = Vec::new();
    let now = &historical.cutoff;
    for account in &historical.accounts {
        if account.archived_at.is_some() || !account.include_in_net_worth {
            continue;
        }
        let state_id = historical.account_state_ids.get(&account.id).cloned();
        if account.tracking_mode == TrackingMode::Holdings.as_str() {
            for holding in valuation_service::snapshot_holdings(snapshot)
                .iter()
                .filter(|holding| holding.account_id == account.id)
            {
                let valued = valuation_service::calculate_holding(snapshot, holding, now)?;
                let instrument = valuation_service::snapshot_instruments(snapshot)
                    .get(&holding.instrument_id)
                    .ok_or_else(|| AppError::not_found("instrument", &holding.instrument_id))?;
                let missing = if valued.complete {
                    None
                } else {
                    valued.dto.missing_reason.clone()
                };
                let base_amount = if valued.complete {
                    valued
                        .base
                        .map(|money| round_to_money_scale(money.amount()).map(canonical_decimal))
                        .transpose()?
                } else {
                    None
                };
                let fx_id = match valued.native {
                    Some(money) => valuation_service::fx_quote_id(snapshot, money.currency(), now)?,
                    None => None,
                };
                items.push(component_item(
                    &account.id,
                    Some(holding.id.clone()),
                    Some(holding.instrument_id.clone()),
                    "holding_quantity",
                    valued.native.map(|money| money.canonical_amount()),
                    valued
                        .native
                        .map(|money| money.currency().as_str().to_owned()),
                    base_amount,
                    valuation_service::instrument_quote_id(snapshot, instrument),
                    fx_id,
                    state_id.clone(),
                    Some(historical.origin_id.clone()),
                    historical.last_quantity_activity.get(&holding.id).cloned(),
                    valued.complete,
                    missing,
                ));
            }
            for cash in valuation_service::snapshot_cash(snapshot)
                .iter()
                .filter(|cash| cash.account_id == account.id)
            {
                let native = Money::parse(
                    &cash.amount,
                    crate::domain::CurrencyCode::parse(&cash.currency)?,
                )?;
                let converted = convert_component(snapshot, native, now)?;
                items.push(component_item(
                    &account.id,
                    None,
                    None,
                    "holdings_cash",
                    Some(native.canonical_amount()),
                    Some(native.currency().as_str().to_owned()),
                    converted.base_amount,
                    None,
                    converted.fx_quote_id,
                    state_id.clone(),
                    Some(historical.origin_id.clone()),
                    historical
                        .last_cash_activity
                        .get(&(account.id.clone(), cash.currency.clone()))
                        .cloned(),
                    converted.complete,
                    converted.missing_reason,
                ));
            }
        } else {
            let Some(value) = &account.latest_value else {
                items.push(component_item(
                    &account.id,
                    None,
                    None,
                    "account_value",
                    None,
                    None,
                    None,
                    None,
                    None,
                    state_id,
                    Some(historical.origin_id.clone()),
                    historical.last_account_activity.get(&account.id).cloned(),
                    false,
                    Some("account_value".to_owned()),
                ));
                continue;
            };
            let native = Money::parse(
                &value.amount,
                crate::domain::CurrencyCode::parse(&value.currency)?,
            )?;
            let converted = convert_component(snapshot, native, now)?;
            items.push(component_item(
                &account.id,
                None,
                None,
                "account_value",
                Some(native.canonical_amount()),
                Some(native.currency().as_str().to_owned()),
                converted.base_amount,
                None,
                converted.fx_quote_id,
                state_id,
                Some(historical.origin_id.clone()),
                historical.last_account_activity.get(&account.id).cloned(),
                converted.complete,
                converted.missing_reason,
            ));
        }
    }
    Ok(items)
}

struct ConvertedComponent {
    base_amount: Option<String>,
    fx_quote_id: Option<String>,
    complete: bool,
    missing_reason: Option<String>,
}

fn convert_component(
    snapshot: &ValuationSnapshot,
    native: Money,
    now: &Timestamp,
) -> Result<ConvertedComponent, AppError> {
    let converted = valuation_service::convert_amount(snapshot, native, now)?;
    let fx_quote_id = valuation_service::fx_quote_id(snapshot, native.currency(), now)?;
    if converted.complete {
        Ok(ConvertedComponent {
            base_amount: converted
                .base
                .map(|money| round_to_money_scale(money.amount()).map(canonical_decimal))
                .transpose()?,
            fx_quote_id,
            complete: true,
            missing_reason: None,
        })
    } else {
        Ok(ConvertedComponent {
            base_amount: None,
            fx_quote_id,
            complete: false,
            missing_reason: converted.missing_reason,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn component_item(
    account_id: &str,
    holding_id: Option<String>,
    instrument_id: Option<String>,
    component_kind: &str,
    native_amount: Option<String>,
    native_currency: Option<String>,
    base_amount: Option<String>,
    instrument_quote_id: Option<String>,
    fx_quote_id: Option<String>,
    account_state_observation_id: Option<String>,
    origin_id: Option<String>,
    activity_id: Option<String>,
    is_complete: bool,
    missing_reason: Option<String>,
) -> DailyValuationSnapshotItemRecord {
    DailyValuationSnapshotItemRecord {
        id: ValuationSnapshotItemId::new().to_string(),
        snapshot_id: String::new(),
        account_id: account_id.to_owned(),
        holding_id,
        instrument_id,
        component_kind: component_kind.to_owned(),
        native_amount,
        native_currency,
        base_amount,
        instrument_quote_id,
        fx_quote_id,
        account_state_observation_id,
        origin_id,
        activity_id,
        is_complete,
        missing_reason,
        sort_order: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            account_service::{self, CreateAccountInput, OwnershipShareInput},
            historical_valuation_service,
            history_query_service::{self, ConfirmHistoryTimezoneInput, HistoryOriginDto},
            holding_service::{self, CreateHoldingInput},
            instrument_service::{self, CreateInstrumentInput},
            member_service, overview_service, query_count,
            quote_service::{
                self, AppendManualInstrumentQuoteInput, SetInstrumentQuotePreferenceInput,
            },
            reference::{begin_read_tx, finish_read_tx, require_household_tx},
        },
        commands::bootstrap::bootstrap_impl,
        domain::{closed_day_cutoff, CalendarDate, HistoryTimezone, Timestamp},
        error::{AppError, CommandError, ErrorCode},
        test_support::{cleanup, onboarded_state},
    };

    fn owner(member_id: &str) -> OwnershipShareInput {
        OwnershipShareInput {
            member_id: member_id.to_owned(),
            percent: Some("100".to_owned()),
            share_bps: None,
        }
    }

    fn bank_input(name: &str, member_id: &str, amount: &str) -> CreateAccountInput {
        CreateAccountInput {
            name: name.to_owned(),
            primary_category: "cash_equivalent".to_owned(),
            secondary_category: "bank_account".to_owned(),
            default_currency: "CNY".to_owned(),
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

    async fn confirm_tz(state: &crate::state::AppState) -> HistoryOriginDto {
        let origin = history_query_service::get_history_origin(state)
            .await
            .expect("origin");
        if origin.timezone_confirmed {
            return origin;
        }
        history_query_service::confirm_history_timezone(
            state,
            ConfirmHistoryTimezoneInput {
                timezone: origin.timezone.clone(),
            },
        )
        .await
        .expect("confirm")
    }

    async fn member_id(state: &crate::state::AppState) -> String {
        member_service::list_members(state, false)
            .await
            .expect("members")[0]
            .id
            .clone()
    }

    async fn count(state: &crate::state::AppState, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(state.writable_db().expect("writable"))
            .await
            .expect("count")
    }

    async fn text(state: &crate::state::AppState, sql: &str) -> String {
        sqlx::query_scalar(sql)
            .fetch_one(state.writable_db().expect("writable"))
            .await
            .expect("text")
    }

    async fn optional_text(state: &crate::state::AppState, sql: &str) -> Option<String> {
        sqlx::query_scalar(sql)
            .fetch_one(state.writable_db().expect("writable"))
            .await
            .expect("optional")
    }

    async fn set_origin_on(state: &crate::state::AppState, local_date: &str) {
        let origin = history_query_service::get_history_origin(state)
            .await
            .expect("origin");
        let timezone = HistoryTimezone::parse(&origin.timezone).expect("tz");
        let date = CalendarDate::parse(local_date).expect("date");
        let origin_at = date
            .pred()
            .map(|previous| closed_day_cutoff(timezone, previous).expect("cutoff"))
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
    }

    async fn backdate_facts_to(state: &crate::state::AppState, at: &Timestamp, local_date: &str) {
        let timestamp = at.to_rfc3339();
        let database = state.writable_db().expect("db");
        sqlx::query(
            "UPDATE activities SET effective_at = ?, effective_local_date = ?, created_at = ?",
        )
        .bind(&timestamp)
        .bind(local_date)
        .bind(&timestamp)
        .execute(database)
        .await
        .expect("activities");
        for sql in [
            "UPDATE account_values SET effective_at = ?, created_at = ?",
            "UPDATE account_cash_values SET effective_at = ?, created_at = ?",
            "UPDATE holding_quantity_values SET effective_at = ?, created_at = ?",
            "UPDATE account_state_observations SET effective_at = ?, created_at = ?",
            "UPDATE holding_state_observations SET effective_at = ?, created_at = ?",
        ] {
            sqlx::query(sql)
                .bind(&timestamp)
                .bind(&timestamp)
                .execute(database)
                .await
                .expect("fact timestamps");
        }
    }

    fn family_count(families: &[&str], name: &str) -> usize {
        families.iter().filter(|family| **family == name).count()
    }

    #[test]
    fn snapshot_totals_match_historical_valuation_and_incomplete_rows_are_explicit() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("phase6-snap-match").await;
            confirm_tz(&state).await;
            let walt = member_id(&state).await;
            account_service::create_account(&state, bank_input("Bank", &walt, "1000"))
                .await
                .expect("bank");
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
            holding_service::create_holding(
                &state,
                CreateHoldingInput {
                    account_id: brokerage.id,
                    instrument_id: instrument.id,
                    quantity: "2".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("holding");

            let origin = history_query_service::get_history_origin(&state)
                .await
                .expect("origin");
            let timezone = HistoryTimezone::parse(&origin.timezone).expect("tz");
            let closed = timezone
                .local_date(&Timestamp::now())
                .pred()
                .expect("yesterday");
            set_origin_on(&state, &closed.to_ymd()).await;
            backdate_facts_to(
                &state,
                &crate::domain::inclusive_closed_day_instant(timezone, closed).expect("inclusive"),
                &closed.to_ymd(),
            )
            .await;

            let rebuilt = rebuild_history_snapshots_bounded(&state, false, 1)
                .await
                .expect("rebuild");
            assert_eq!(rebuilt.processed_days, 1);
            assert!(!rebuilt.cancelled);

            let database = state.writable_db().expect("db");
            let mut tx = begin_read_tx(database).await.expect("tx");
            let (historical, _) =
                historical_valuation_service::reconstruct_closed_day(&mut tx, timezone, closed)
                    .await
                    .expect("historical");
            let household = require_household_tx(&mut tx).await.expect("household");
            let snapshot = history_repositories::latest_snapshot_for_date(
                &mut tx,
                &household.id,
                &closed.to_ymd(),
            )
            .await
            .expect("latest")
            .expect("row");
            let currency = historical.base_currency;
            assert_eq!(
                snapshot.assets_amount,
                historical
                    .totals
                    .rounded_assets(currency)
                    .expect("assets")
                    .amount
            );
            assert_eq!(
                snapshot.liabilities_amount,
                historical
                    .totals
                    .rounded_liabilities(currency)
                    .expect("liab")
                    .amount
            );
            assert_eq!(
                snapshot.net_worth_amount,
                historical
                    .totals
                    .rounded_net_worth(currency)
                    .expect("nw")
                    .amount
            );
            assert_eq!(snapshot.is_complete, historical.totals.complete);
            let items = history_repositories::list_snapshot_items(&mut tx, &snapshot.id)
                .await
                .expect("items");
            assert!(items.iter().any(|item| !item.is_complete));
            assert!(items.iter().all(|item| {
                !(item.is_complete
                    && item.base_amount.as_deref() == Some("0")
                    && item.missing_reason.is_some())
            }));
            assert!(items
                .iter()
                .all(|item| { item.is_complete || item.missing_reason.is_some() }));
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");
            cleanup(&path);
        });
    }

    #[test]
    fn rebuild_appends_revision_and_never_mutates_previous() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("phase6-revision").await;
            confirm_tz(&state).await;
            let walt = member_id(&state).await;
            account_service::create_account(&state, bank_input("Bank", &walt, "50"))
                .await
                .expect("bank");
            let origin = history_query_service::get_history_origin(&state)
                .await
                .expect("origin");
            let timezone = HistoryTimezone::parse(&origin.timezone).expect("tz");
            let closed = timezone
                .local_date(&Timestamp::now())
                .pred()
                .expect("yesterday");
            set_origin_on(&state, &closed.to_ymd()).await;
            rebuild_history_snapshots_bounded(&state, false, 1)
                .await
                .expect("first");
            let first_id = text(
                &state,
                "SELECT id FROM daily_valuation_snapshots ORDER BY revision ASC, created_at ASC LIMIT 1",
            )
            .await;
            let first_assets = text(
                &state,
                "SELECT assets_amount FROM daily_valuation_snapshots WHERE revision = 1",
            )
            .await;
            sqlx::query("UPDATE history_snapshot_state SET dirty_from = ?")
                .bind(closed.to_ymd())
                .execute(state.writable_db().expect("db"))
                .await
                .expect("redirty");
            rebuild_history_snapshots_bounded(&state, false, 1)
                .await
                .expect("second");
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM daily_valuation_snapshots").await,
                2
            );
            assert_eq!(
                text(
                    &state,
                    "SELECT assets_amount FROM daily_valuation_snapshots WHERE revision = 1"
                )
                .await,
                first_assets
            );
            assert_eq!(
                text(
                    &state,
                    "SELECT id FROM daily_valuation_snapshots WHERE revision = 1"
                )
                .await,
                first_id
            );
            let latest_revision: i64 =
                sqlx::query_scalar("SELECT MAX(revision) FROM daily_valuation_snapshots")
                    .fetch_one(state.writable_db().expect("db"))
                    .await
                    .expect("max revision");
            assert_eq!(latest_revision, 2);
            cleanup(&path);
        });
    }

    #[test]
    fn rebuild_is_bounded_and_keeps_earliest_unprocessed_day_dirty() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("phase6-366").await;
            confirm_tz(&state).await;
            let origin = history_query_service::get_history_origin(&state)
                .await
                .expect("origin");
            let timezone = HistoryTimezone::parse(&origin.timezone).expect("tz");
            let today = timezone.local_date(&Timestamp::now());
            let last_closed = today.pred().expect("yesterday");
            let start = last_closed.checked_add_days(-399).expect("start");
            set_origin_on(&state, &start.to_ymd()).await;
            let rebuilt = rebuild_history_snapshots_bounded(&state, false, MAX_REBUILD_DAYS)
                .await
                .expect("bounded rebuild");
            assert_eq!(rebuilt.processed_days, 366);
            assert!(rebuilt.remaining);
            let expected_dirty = start.checked_add_days(366).expect("dirty").to_ymd();
            assert_eq!(rebuilt.dirty_from.as_deref(), Some(expected_dirty.as_str()));
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM daily_valuation_snapshots").await,
                366
            );

            sqlx::query("UPDATE history_snapshot_state SET dirty_from = ?")
                .bind(start.to_ymd())
                .execute(state.writable_db().expect("db"))
                .await
                .expect("reset dirty");
            let cancelled =
                rebuild_history_snapshots(&state, RebuildHistorySnapshotsInput { cancel: true })
                    .await
                    .expect("cancel");
            assert!(cancelled.cancelled);
            assert_eq!(cancelled.processed_days, 0);
            assert_eq!(
                cancelled.dirty_from.as_deref(),
                Some(start.to_ymd().as_str())
            );

            sqlx::query("UPDATE history_snapshot_state SET dirty_from = 'bogus'")
                .execute(state.writable_db().expect("db"))
                .await
                .expect("bogus dirty");
            let failed =
                rebuild_history_snapshots(&state, RebuildHistorySnapshotsInput { cancel: false })
                    .await
                    .expect_err("failed rebuild");
            assert!(matches!(failed, AppError::SnapshotRebuildFailed));
            assert_eq!(
                text(&state, "SELECT dirty_from FROM history_snapshot_state").await,
                "bogus"
            );
            let shielded = CommandError::from(failed);
            assert_eq!(shielded.code, ErrorCode::SnapshotRebuildFailed);
            assert!(!shielded.message.contains("bogus"));
            cleanup(&path);
        });
    }

    #[test]
    fn quote_preference_and_current_reads_do_not_rebuild() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("phase6-norebuild").await;
            confirm_tz(&state).await;
            let instrument = instrument_service::create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "Cash ETF".to_owned(),
                    symbol: Some("CASH".to_owned()),
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
            set_origin_on(&state, "2026-01-01").await;
            sqlx::query("UPDATE history_snapshot_state SET dirty_from = NULL")
                .execute(state.writable_db().expect("db"))
                .await
                .expect("clear");
            quote_service::append_manual_instrument_quote(
                &state,
                AppendManualInstrumentQuoteInput {
                    instrument_id: instrument.id.clone(),
                    unit_price: "10".to_owned(),
                    quoted_at: Some("2026-03-01T12:00:00.000Z".to_owned()),
                },
            )
            .await
            .expect("quote");
            let quoted = Timestamp::parse("2026-03-01T12:00:00.000Z").expect("quoted");
            let origin = history_query_service::get_history_origin(&state)
                .await
                .expect("origin");
            let timezone = HistoryTimezone::parse(&origin.timezone).expect("tz");
            assert_eq!(
                text(&state, "SELECT dirty_from FROM history_snapshot_state").await,
                timezone.local_date(&quoted).to_ymd()
            );
            sqlx::query("UPDATE history_snapshot_state SET dirty_from = NULL")
                .execute(state.writable_db().expect("db"))
                .await
                .expect("clear");
            quote_service::set_instrument_quote_preference(
                &state,
                SetInstrumentQuotePreferenceInput {
                    instrument_id: instrument.id,
                    quote_preference: "provider".to_owned(),
                },
            )
            .await
            .expect("preference");
            assert_eq!(
                text(&state, "SELECT dirty_from FROM history_snapshot_state").await,
                timezone.local_date(&Timestamp::now()).to_ymd()
            );

            let snapshots_before =
                count(&state, "SELECT COUNT(*) FROM daily_valuation_snapshots").await;
            overview_service::get_overview(&state)
                .await
                .expect("overview");
            bootstrap_impl(&state).await.expect("bootstrap");
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM daily_valuation_snapshots").await,
                snapshots_before
            );
            assert!(
                optional_text(&state, "SELECT dirty_from FROM history_snapshot_state")
                    .await
                    .is_some()
            );
            cleanup(&path);
        });
    }

    #[test]
    fn trend_and_rebuild_query_counts_are_bounded() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("phase6-trend-q").await;
            confirm_tz(&state).await;
            let walt = member_id(&state).await;
            account_service::create_account(&state, bank_input("Bank", &walt, "80"))
                .await
                .expect("bank");
            let origin = history_query_service::get_history_origin(&state)
                .await
                .expect("origin");
            let timezone = HistoryTimezone::parse(&origin.timezone).expect("tz");
            let closed = timezone
                .local_date(&Timestamp::now())
                .pred()
                .expect("yesterday");
            set_origin_on(&state, &closed.to_ymd()).await;
            let (rebuilt, rebuild_families) =
                query_count::capture_async(|| rebuild_history_snapshots_bounded(&state, false, 1))
                    .await;
            let rebuilt = rebuilt.expect("rebuild");
            assert_eq!(rebuilt.processed_days, 1);
            let reconstructs = family_count(&rebuild_families, "historical_reconstruct");
            let legs = family_count(&rebuild_families, "activity_legs");
            assert_eq!(
                reconstructs, 1,
                "one reconstruct per closed day {rebuild_families:?}"
            );
            assert!(
                legs <= reconstructs,
                "legs must be batched per reconstruct, not per activity {rebuild_families:?}"
            );
            assert_eq!(family_count(&rebuild_families, "rebuild_plan"), 1);
            assert_eq!(family_count(&rebuild_families, "snapshot_write"), 1);

            let (trend, trend_families) = query_count::capture_async(|| {
                get_net_worth_trend(
                    &state,
                    GetNetWorthTrendInput {
                        range: "1m".to_owned(),
                    },
                )
            })
            .await;
            let trend = trend.expect("trend");
            assert!(trend.current.is_live);
            assert_eq!(trend.current.net_worth.currency, "CNY");
            assert_eq!(family_count(&trend_families, "trend"), 1);
            assert_eq!(family_count(&trend_families, "snapshots_range"), 1);
            assert_eq!(family_count(&trend_families, "trend_live"), 1);
            assert!(
                family_count(&trend_families, "activity_legs") == 0,
                "trend must not walk activities {trend_families:?}"
            );
            cleanup(&path);
        });
    }

    #[test]
    fn timezone_confirmation_is_required_before_rebuild_when_unconfirmed() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("phase6-tz-rebuild").await;
            sqlx::query("UPDATE history_origins SET timezone_confirmed = 0, timezone = 'UTC'")
                .execute(state.writable_db().expect("db"))
                .await
                .expect("unconfirm");
            let error =
                rebuild_history_snapshots(&state, RebuildHistorySnapshotsInput { cancel: false })
                    .await
                    .expect_err("unconfirmed");
            assert!(matches!(
                error,
                AppError::HistoryTimezoneConfirmationRequired
            ));
            cleanup(&path);
        });
    }

    #[test]
    fn history_origin_confirm_round_trip() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("phase6-origin").await;
            let origin = history_query_service::get_history_origin(&state)
                .await
                .expect("origin");
            assert!(!origin.id.is_empty());
            if !origin.timezone_confirmed {
                let confirmed = history_query_service::confirm_history_timezone(
                    &state,
                    ConfirmHistoryTimezoneInput {
                        timezone: "UTC".to_owned(),
                    },
                )
                .await
                .expect("confirm");
                assert!(confirmed.timezone_confirmed);
            }
            cleanup(&path);
        });
    }
}
