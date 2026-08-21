//! Cost-basis declaration writes for unknown-basis lots.
//!
//! Declarations supply cost metadata only. They never post an Activity, change
//! current state, mark snapshots dirty, or rewrite projection rows.

use std::collections::HashMap;

use sqlx::{Sqlite, Transaction};

use super::{
    analytics_repositories::{self, CostBasisDeclarationRecord},
    history_repositories::{self, HistoryOriginRecord, OriginHoldingRecord},
    instrument_service,
    reference::{begin_write_tx, finish_write_tx, require_household_tx},
};
use crate::{
    domain::{
        parse_optional_note, replay, AccountId, Activity, ActivityId, ActivityKind,
        ActivityLedgerEvent, ActivityLegId, BasisStatus, CalendarDate, CostBasisDeclarationId,
        CurrencyCode, Direction, HistoryOriginId, HistoryTimezone, HoldingId, InstrumentId,
        LedgerEvent, LegComponent, LegRole, LotEffect, LotLedger, LotOpening, LotRef, Money,
        Quantity, Timestamp,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclareLotCostBasisInput {
    pub origin_holding_id: Option<String>,
    pub activity_leg_id: Option<String>,
    pub instrument_id: String,
    pub declared_cost: String,
    pub declared_currency: String,
    pub acquired_on: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevokeLotCostBasisInput {
    pub origin_holding_id: Option<String>,
    pub activity_leg_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostBasisDeclarationDto {
    pub id: String,
    pub household_id: String,
    pub origin_holding_id: Option<String>,
    pub activity_leg_id: Option<String>,
    pub instrument_id: String,
    pub declared_cost: Option<String>,
    pub declared_currency: Option<String>,
    pub acquired_on: Option<String>,
    pub revokes: Option<String>,
    pub is_revocation: bool,
    pub note: Option<String>,
    pub created_at: String,
}

pub async fn declare_lot_cost_basis(
    state: &AppState,
    input: DeclareLotCostBasisInput,
) -> Result<CostBasisDeclarationDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = declare_lot_cost_basis_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn revoke_lot_cost_basis(
    state: &AppState,
    input: RevokeLotCostBasisInput,
) -> Result<CostBasisDeclarationDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = revoke_lot_cost_basis_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn declare_lot_cost_basis_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: DeclareLotCostBasisInput,
) -> Result<CostBasisDeclarationDto, AppError> {
    let household = require_household_tx(tx).await?;
    let lot_ref = parse_lot_ref(
        input.origin_holding_id.as_deref(),
        input.activity_leg_id.as_deref(),
    )?;
    let instrument_id = InstrumentId::parse(input.instrument_id.trim())?;
    let note = parse_optional_note(input.note.as_deref())?;
    let acquired_on = parse_optional_acquired_on(input.acquired_on.as_deref())?;

    let opening = resolve_unknown_opening(tx, &household.id, lot_ref).await?;
    if opening.instrument_id() != instrument_id {
        return Err(AppError::invalid_cost_basis_declaration(
            "The declared instrument does not match this lot.",
        ));
    }

    let instrument =
        instrument_service::load_instrument(tx, &household.id, &instrument_id.to_string()).await?;
    let quote_currency = CurrencyCode::parse(&instrument.quote_currency)?;
    let declared_currency = CurrencyCode::parse(input.declared_currency.trim())?;
    if declared_currency != quote_currency {
        return Err(AppError::invalid_cost_basis_declaration(
            "Declared cost currency must equal the instrument quote currency.",
        ));
    }
    let cost = Money::parse(input.declared_cost.trim(), declared_currency)?;

    if let Some(acquired_on) = acquired_on {
        let origin = required_origin(tx, &household.id).await?;
        let timezone = HistoryTimezone::parse(&origin.timezone)?;
        let lot_local_date = timezone.local_date(opening.acquired_at());
        if acquired_on > lot_local_date {
            return Err(AppError::invalid_cost_basis_declaration(
                "The acquisition date cannot be after the lot's ledger acquisition date.",
            ));
        }
    }

    let keys = lot_ref_keys(lot_ref);
    let record = CostBasisDeclarationRecord {
        id: CostBasisDeclarationId::new().to_string(),
        household_id: household.id,
        origin_holding_id: keys.0,
        activity_leg_id: keys.1,
        instrument_id: instrument_id.to_string(),
        declared_cost: Some(cost.canonical_amount()),
        declared_currency: Some(cost.currency().as_str().to_owned()),
        acquired_on: acquired_on.map(CalendarDate::to_ymd),
        revokes: None,
        is_revocation: false,
        note,
        created_at: Timestamp::now().to_rfc3339(),
    };
    analytics_repositories::insert_declaration(tx, &record).await?;
    tracing::info!(
        event = "cost_basis.declared",
        "cost-basis declaration recorded"
    );
    Ok(dto_from_record(record))
}

pub async fn revoke_lot_cost_basis_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: RevokeLotCostBasisInput,
) -> Result<CostBasisDeclarationDto, AppError> {
    let household = require_household_tx(tx).await?;
    let lot_ref = parse_lot_ref(
        input.origin_holding_id.as_deref(),
        input.activity_leg_id.as_deref(),
    )?;
    let opening = resolve_unknown_opening(tx, &household.id, lot_ref).await?;
    let keys = lot_ref_keys(lot_ref);
    let latest = analytics_repositories::latest_declaration_for_lot(
        tx,
        &household.id,
        keys.0.as_deref(),
        keys.1.as_deref(),
    )
    .await?;
    let Some(latest) = latest else {
        return Err(AppError::invalid_cost_basis_declaration(
            "This lot has no cost-basis declaration to revoke.",
        ));
    };
    if latest.is_revocation {
        return Err(AppError::invalid_cost_basis_declaration(
            "The latest declaration for this lot is already a revocation.",
        ));
    }

    let record = CostBasisDeclarationRecord {
        id: CostBasisDeclarationId::new().to_string(),
        household_id: household.id,
        origin_holding_id: keys.0,
        activity_leg_id: keys.1,
        instrument_id: opening.instrument_id().to_string(),
        declared_cost: None,
        declared_currency: None,
        acquired_on: None,
        revokes: Some(latest.id),
        is_revocation: true,
        note: None,
        created_at: Timestamp::now().to_rfc3339(),
    };
    analytics_repositories::insert_declaration(tx, &record).await?;
    tracing::info!(
        event = "cost_basis.revoked",
        "cost-basis declaration revoked"
    );
    Ok(dto_from_record(record))
}

async fn resolve_unknown_opening(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    lot_ref: LotRef,
) -> Result<LotOpening, AppError> {
    match lot_ref {
        LotRef::OriginHolding(holding_id) => {
            if analytics_repositories::get_origin_holding_for_household(
                tx,
                household_id,
                &holding_id.to_string(),
            )
            .await?
            .is_none()
            {
                return Err(AppError::CostBasisLotNotFound);
            }
        }
        LotRef::Acquisition(leg_id) => {
            if analytics_repositories::get_activity_leg_for_household(
                tx,
                household_id,
                &leg_id.to_string(),
            )
            .await?
            .is_none()
            {
                return Err(AppError::CostBasisLotNotFound);
            }
        }
    }

    let origin = required_origin(tx, household_id).await?;
    let holdings = history_repositories::list_origin_holdings(tx, &origin.id).await?;
    let activities = history_repositories::list_all_activities_asc(tx, household_id).await?;
    let events = ledger_events_from_origin_and_activities(&origin, &holdings, &activities)?;
    let ledger = replay(events)?;
    let Some(opening) = ledger.opening(lot_ref).cloned() else {
        return Err(AppError::invalid_cost_basis_declaration(
            "This reference does not open a lot.",
        ));
    };
    if opening.basis() != BasisStatus::Unknown {
        return Err(AppError::invalid_cost_basis_declaration(
            "A known-basis lot cannot receive a cost-basis declaration.",
        ));
    }
    Ok(opening)
}

async fn required_origin(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<HistoryOriginRecord, AppError> {
    history_repositories::get_origin_by_household(tx, household_id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)
}

/// Replay the posted ledger and overlay the latest effective declaration per lot.
///
/// A supplying declaration makes an unknown-basis opening known. A latest
/// revocation leaves the lot unknown. Quantity and money balances are unchanged.
pub(crate) async fn load_effective_lot_ledger_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<LotLedger, AppError> {
    let origin = required_origin(tx, household_id).await?;
    let holdings = history_repositories::list_origin_holdings(tx, &origin.id).await?;
    let activities = history_repositories::list_all_activities_asc(tx, household_id).await?;
    let events = ledger_events_from_origin_and_activities(&origin, &holdings, &activities)?;
    let mut ledger = replay(events)?;
    let declarations =
        analytics_repositories::list_declarations_for_household(tx, household_id).await?;
    overlay_effective_declarations(&mut ledger, &declarations)?;
    Ok(ledger)
}

pub(crate) fn overlay_effective_declarations(
    ledger: &mut LotLedger,
    declarations: &[CostBasisDeclarationRecord],
) -> Result<(), AppError> {
    let mut latest = HashMap::new();
    for record in declarations {
        let lot_ref = parse_lot_ref(
            record.origin_holding_id.as_deref(),
            record.activity_leg_id.as_deref(),
        )?;
        latest.entry(lot_ref).or_insert(record);
    }
    for (lot_ref, record) in latest {
        if record.is_revocation {
            continue;
        }
        let (Some(amount), Some(currency)) = (
            record.declared_cost.as_deref(),
            record.declared_currency.as_deref(),
        ) else {
            return Err(AppError::Internal);
        };
        let cost = Money::parse(amount, CurrencyCode::parse(currency)?)?;
        let acquired_on = record
            .acquired_on
            .as_deref()
            .map(CalendarDate::parse)
            .transpose()?;
        ledger.apply_declaration(lot_ref, cost, acquired_on)?;
    }
    Ok(())
}

pub(crate) fn ledger_events_from_origin_and_activities(
    origin: &HistoryOriginRecord,
    holdings: &[OriginHoldingRecord],
    activities: &[Activity],
) -> Result<Vec<LedgerEvent>, AppError> {
    let origin_id = HistoryOriginId::parse(&origin.id)?;
    let origin_at = Timestamp::parse(&origin.origin_at)?;
    let mut events = Vec::new();
    for holding in holdings {
        events.push(LedgerEvent::OriginBaseline {
            origin_id,
            holding_id: HoldingId::parse(&holding.holding_id)?,
            instrument_id: InstrumentId::parse(&holding.instrument_id)?,
            account_id: AccountId::parse(&holding.account_id)?,
            quantity: Quantity::parse(&holding.quantity)?,
            origin_at: origin_at.clone(),
        });
    }

    let mut reversed_by = HashMap::new();
    for activity in activities {
        if let Some(original) = activity.reverses() {
            reversed_by.insert(original, activity.id());
        }
    }
    for activity in activities {
        events.push(map_activity(
            activity,
            reversed_by.get(&activity.id()).copied(),
        ));
    }
    Ok(events)
}

fn map_activity(activity: &Activity, reversed_by: Option<ActivityId>) -> LedgerEvent {
    let (sort_order, effect) = map_effect(activity);
    LedgerEvent::Activity(ActivityLedgerEvent {
        activity_id: activity.id(),
        created_at: activity.created_at().clone(),
        effective_at: activity.effective_at().clone(),
        reverses: activity.reverses(),
        reversed_by,
        sort_order,
        effect,
    })
}

fn map_effect(activity: &Activity) -> (i64, LotEffect) {
    match activity.kind() {
        ActivityKind::Buy => map_trade(activity, true),
        ActivityKind::Sell => map_trade(activity, false),
        ActivityKind::OpeningAdjustment => map_quantity_adjustment(activity, true),
        ActivityKind::PositionAdjustment => map_quantity_adjustment(activity, false),
        ActivityKind::Transfer => map_position_transfer(activity),
        _ => (0, LotEffect::None),
    }
}

fn map_trade(activity: &Activity, is_buy: bool) -> (i64, LotEffect) {
    let Some(holding) = activity
        .legs()
        .iter()
        .find(|leg| leg.role() == LegRole::Holding)
    else {
        return (0, LotEffect::None);
    };
    let LegComponent::HoldingQuantity {
        instrument_id,
        quantity,
        ..
    } = holding.component()
    else {
        return (0, LotEffect::None);
    };
    let gross = monetary_leg(activity, LegRole::Settlement);
    let fee = monetary_leg(activity, LegRole::Fee);
    let effect = if is_buy {
        LotEffect::Buy {
            holding_leg_id: holding.id(),
            instrument_id: *instrument_id,
            account_id: holding.account_id(),
            quantity: *quantity,
            gross_settlement: gross,
            acquisition_fee: fee,
        }
    } else {
        LotEffect::Sell {
            holding_leg_id: holding.id(),
            instrument_id: *instrument_id,
            account_id: holding.account_id(),
            quantity: *quantity,
            proceeds_gross: gross,
            disposal_fee: fee,
        }
    };
    (holding.sort_order(), effect)
}

fn map_quantity_adjustment(activity: &Activity, opening: bool) -> (i64, LotEffect) {
    let Some(leg) = activity.legs().iter().find(|leg| {
        matches!(leg.component(), LegComponent::HoldingQuantity { .. })
            && leg.role() == LegRole::Adjustment
    }) else {
        return (0, LotEffect::None);
    };
    let LegComponent::HoldingQuantity {
        instrument_id,
        quantity,
        ..
    } = leg.component()
    else {
        return (0, LotEffect::None);
    };
    let effect = match (opening, leg.direction()) {
        (true, Direction::Increase) => LotEffect::OpeningIncrease {
            holding_leg_id: leg.id(),
            instrument_id: *instrument_id,
            account_id: leg.account_id(),
            quantity: *quantity,
        },
        (false, Direction::Increase) => LotEffect::PositionIncrease {
            holding_leg_id: leg.id(),
            instrument_id: *instrument_id,
            account_id: leg.account_id(),
            quantity: *quantity,
        },
        (false, Direction::Decrease) => LotEffect::PositionDecrease {
            holding_leg_id: leg.id(),
            instrument_id: *instrument_id,
            account_id: leg.account_id(),
            quantity: *quantity,
        },
        (true, Direction::Decrease) => LotEffect::None,
    };
    (leg.sort_order(), effect)
}

fn map_position_transfer(activity: &Activity) -> (i64, LotEffect) {
    let source = activity.legs().iter().find(|leg| {
        leg.role() == LegRole::Source
            && matches!(leg.component(), LegComponent::HoldingQuantity { .. })
    });
    let destination = activity.legs().iter().find(|leg| {
        leg.role() == LegRole::Destination
            && matches!(leg.component(), LegComponent::HoldingQuantity { .. })
    });
    let (Some(source), Some(destination)) = (source, destination) else {
        return (0, LotEffect::None);
    };
    let LegComponent::HoldingQuantity {
        instrument_id,
        quantity,
        ..
    } = source.component()
    else {
        return (0, LotEffect::None);
    };
    (
        source.sort_order(),
        LotEffect::PositionTransfer {
            source_leg_id: source.id(),
            destination_leg_id: destination.id(),
            instrument_id: *instrument_id,
            source_account_id: source.account_id(),
            destination_account_id: destination.account_id(),
            quantity: *quantity,
        },
    )
}

fn monetary_leg(activity: &Activity, role: LegRole) -> Option<Money> {
    activity
        .legs()
        .iter()
        .find(|leg| leg.role() == role)
        .and_then(|leg| leg.component().money().ok())
}

fn parse_lot_ref(
    origin_holding_id: Option<&str>,
    activity_leg_id: Option<&str>,
) -> Result<LotRef, AppError> {
    let origin = nonempty(origin_holding_id);
    let leg = nonempty(activity_leg_id);
    match (origin, leg) {
        (Some(origin), None) => Ok(LotRef::OriginHolding(HoldingId::parse(origin)?)),
        (None, Some(leg)) => Ok(LotRef::Acquisition(ActivityLegId::parse(leg)?)),
        (Some(_), Some(_)) | (None, None) => Err(AppError::invalid_cost_basis_declaration(
            "Declare exactly one of an origin holding or an activity leg.",
        )),
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn parse_optional_acquired_on(value: Option<&str>) -> Result<Option<CalendarDate>, AppError> {
    let Some(value) = nonempty(value) else {
        return Ok(None);
    };
    Ok(Some(CalendarDate::parse(value)?))
}

fn lot_ref_keys(lot_ref: LotRef) -> (Option<String>, Option<String>) {
    match lot_ref {
        LotRef::OriginHolding(holding_id) => (Some(holding_id.to_string()), None),
        LotRef::Acquisition(leg_id) => (None, Some(leg_id.to_string())),
    }
}

fn dto_from_record(record: CostBasisDeclarationRecord) -> CostBasisDeclarationDto {
    CostBasisDeclarationDto {
        id: record.id,
        household_id: record.household_id,
        origin_holding_id: record.origin_holding_id,
        activity_leg_id: record.activity_leg_id,
        instrument_id: record.instrument_id,
        declared_cost: record.declared_cost,
        declared_currency: record.declared_currency,
        acquired_on: record.acquired_on,
        revokes: record.revokes,
        is_revocation: record.is_revocation,
        note: record.note,
        created_at: record.created_at,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        declare_lot_cost_basis, revoke_lot_cost_basis, DeclareLotCostBasisInput,
        RevokeLotCostBasisInput,
    };
    use crate::{
        application::{
            account_service::get_account,
            analytics_repositories::{self, CostBasisDeclarationRecord},
            overview_service::get_overview,
            portfolio_service::get_portfolio,
            reference::{begin_write_tx, finish_write_tx, require_household_id_tx},
        },
        error::AppError,
        infrastructure::{database::connect_writable, database_bootstrap::MIGRATOR},
        state::AppState,
        test_support::{
            blocked_future_state, cleanup, stable_sqlite_hash, test_path, UNKNOWN_UUID,
        },
    };
    use std::fs;
    use std::path::PathBuf;

    const ORIGIN_QQQ_HOLDING: &str = "30303030-3030-4303-8303-303030303030";
    const QQQ: &str = "20202020-2020-4202-8202-202020202020";
    const ES3: &str = "21212121-2121-4212-8212-212121212121";
    const VOO: &str = "25252525-2525-4252-8252-252525252525";
    const XLF: &str = "27272727-2727-4272-8272-272727272727";
    const BROKERAGE: &str = "99999999-9999-4999-8999-999999999999";
    const FIFO_BUY1_LEG: &str = "01a0188f-861c-7b20-8609-5363bbc99c48";
    const ZERO_GROSS_LEG: &str = "01a0188f-8621-7a61-a206-bf66455312f8";
    const REVERSED_ES3_ACTIVITY: &str = "01a0188f-8622-7d80-a656-dc56157ac0e8";
    const REVERSED_ES3_LEG: &str = "01a0188f-8622-7d80-a656-dc6057cd6dc2";
    const SELL_LEG: &str = "01a0188f-861f-7c20-83d1-4ac8ea0f6396";
    const TRANSFER_SOURCE_LEG: &str = "01a0188f-862f-7b90-9d5a-a95a884f68f0";
    const OPENING_XLF_LEG: &str = "01a0188f-862c-7c93-9999-26697ff52022";
    const FIFO_VOO_HOLDING: &str = "35353535-3535-4353-8353-353535353535";

    fn origin_qqq_declare(
        cost: &str,
        currency: &str,
        acquired_on: Option<&str>,
    ) -> DeclareLotCostBasisInput {
        DeclareLotCostBasisInput {
            origin_holding_id: Some(ORIGIN_QQQ_HOLDING.to_owned()),
            activity_leg_id: None,
            instrument_id: QQQ.to_owned(),
            declared_cost: cost.to_owned(),
            declared_currency: currency.to_owned(),
            acquired_on: acquired_on.map(ToOwned::to_owned),
            note: None,
        }
    }

    fn origin_qqq_revoke() -> RevokeLotCostBasisInput {
        RevokeLotCostBasisInput {
            origin_holding_id: Some(ORIGIN_QQQ_HOLDING.to_owned()),
            activity_leg_id: None,
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
        let path = test_path("v014-p2", name);
        let _ = fs::remove_file(&path);
        apply_migrations(&path, &[1, 2, 3]).await;
        load_sql(&path, include_str!("../../test-fixtures/v0.1.3.sql")).await;
        let state = AppState::initialize(path.clone()).await;
        (state, path)
    }

    async fn load_v012_to_current(name: &str) -> (AppState, PathBuf) {
        let path = test_path("v014-p2", name);
        let _ = fs::remove_file(&path);
        apply_migrations(&path, &[1, 2]).await;
        load_sql(&path, include_str!("../../test-fixtures/v0.1.2.sql")).await;
        let state = AppState::initialize(path.clone()).await;
        (state, path)
    }

    async fn scalar_i64(state: &AppState, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(state.writable_db().expect("writable"))
            .await
            .expect("scalar")
    }

    async fn ledger_fingerprint(state: &AppState) -> String {
        let db = state.writable_db().expect("writable");
        let mut parts = Vec::new();
        for (label, sql) in [
            (
                "activities",
                "SELECT COALESCE(GROUP_CONCAT(id, ','), '') FROM (SELECT id FROM activities ORDER BY id)",
            ),
            (
                "legs",
                "SELECT COALESCE(GROUP_CONCAT(id, ','), '') FROM (SELECT id FROM activity_legs ORDER BY id)",
            ),
            (
                "account_values",
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || amount || ':' || IFNULL(activity_id, ''), ','), '') FROM (SELECT id, amount, activity_id FROM account_values ORDER BY id)",
            ),
            (
                "cash",
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || amount, ','), '') FROM (SELECT id, amount FROM account_cash_values ORDER BY id)",
            ),
            (
                "holdings",
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || quantity, ','), '') FROM (SELECT id, quantity FROM holdings ORDER BY id)",
            ),
            (
                "qty",
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || quantity, ','), '') FROM (SELECT id, quantity FROM holding_quantity_values ORDER BY id)",
            ),
            (
                "snapshots",
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || CAST(revision AS TEXT), ','), '') FROM (SELECT id, revision FROM daily_valuation_snapshots ORDER BY id)",
            ),
            (
                "snapshot_items",
                "SELECT COALESCE(GROUP_CONCAT(id, ','), '') FROM (SELECT id FROM daily_valuation_snapshot_items ORDER BY id)",
            ),
            (
                "corrections",
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || IFNULL(reverses, '') || ':' || IFNULL(corrects, '') || ':' || IFNULL(correction_group, ''), ','), '') FROM (SELECT id, reverses, corrects, correction_group FROM activities ORDER BY id)",
            ),
            (
                "archives",
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || IFNULL(archived_at, ''), ','), '') FROM (SELECT id, archived_at FROM accounts ORDER BY id)",
            ),
            (
                "dirty",
                "SELECT COALESCE(GROUP_CONCAT(household_id || ':' || IFNULL(dirty_from, '') || ':' || rebuild_status, ','), '') FROM history_snapshot_state",
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

    async fn integrity_ok(state: &AppState) {
        let db = state.writable_db().expect("writable");
        let foreign_keys = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(db)
            .await
            .expect("foreign key check");
        assert!(foreign_keys.is_empty());
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(db)
            .await
            .expect("integrity check");
        assert_eq!(integrity, "ok");
    }

    #[test]
    fn v013_declaration_writes_no_ledger_projection_snapshot_or_totals() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("declare-write-nothing").await;
            let before = ledger_fingerprint(&state).await;
            let before_overview = get_overview(&state).await.expect("overview");
            let before_portfolio = get_portfolio(&state).await.expect("portfolio");
            let before_account = get_account(&state, BROKERAGE).await.expect("account");
            let activities_before = scalar_i64(&state, "SELECT COUNT(*) FROM activities").await;
            let legs_before = scalar_i64(&state, "SELECT COUNT(*) FROM activity_legs").await;
            let declarations_before =
                scalar_i64(&state, "SELECT COUNT(*) FROM cost_basis_declarations").await;
            assert_eq!(declarations_before, 0);

            let declared = declare_lot_cost_basis(&state, origin_qqq_declare("1500", "USD", None))
                .await
                .expect("declare origin QQQ");
            assert_eq!(
                declared.origin_holding_id.as_deref(),
                Some(ORIGIN_QQQ_HOLDING)
            );
            assert_eq!(declared.declared_cost.as_deref(), Some("1500"));
            assert_eq!(declared.declared_currency.as_deref(), Some("USD"));
            assert!(!declared.is_revocation);
            assert_eq!(
                scalar_i64(&state, "SELECT COUNT(*) FROM cost_basis_declarations").await,
                1
            );
            assert_eq!(
                scalar_i64(&state, "SELECT COUNT(*) FROM activities").await,
                activities_before
            );
            assert_eq!(
                scalar_i64(&state, "SELECT COUNT(*) FROM activity_legs").await,
                legs_before
            );
            assert_eq!(ledger_fingerprint(&state).await, before);
            assert_eq!(
                get_overview(&state).await.expect("overview"),
                before_overview
            );
            assert_eq!(
                get_portfolio(&state).await.expect("portfolio"),
                before_portfolio
            );
            assert_eq!(
                get_account(&state, BROKERAGE).await.expect("account"),
                before_account
            );
            integrity_ok(&state).await;
            cleanup(&path);
        });
    }

    #[test]
    fn v012_goldens_unchanged_after_migrate_to_6() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v012_to_current("v012-goldens").await;
            assert_eq!(
                scalar_i64(&state, "SELECT MAX(version) FROM _sqlx_migrations").await,
                6
            );
            assert_eq!(
                scalar_i64(&state, "SELECT COUNT(*) FROM cost_basis_declarations").await,
                0
            );
            assert_eq!(
                scalar_i64(&state, "SELECT COUNT(*) FROM activities").await,
                0
            );
            assert_eq!(
                scalar_i64(&state, "SELECT schema_version FROM history_origins").await,
                3
            );
            let overview = get_overview(&state).await.expect("overview");
            assert_eq!(overview.assets.amount, "63190");
            assert_eq!(overview.net_worth.amount, "63190");
            let account = get_account(&state, BROKERAGE).await.expect("holdings");
            assert_eq!(
                account
                    .valuation
                    .base
                    .as_ref()
                    .map(|value| value.amount.as_str()),
                Some("62190")
            );
            let portfolio = get_portfolio(&state).await.expect("portfolio");
            assert_eq!(portfolio.total.amount, "63190");
            integrity_ok(&state).await;
            cleanup(&path);
        });
    }

    #[test]
    fn reject_known_basis_nonexistent_mismatch_and_reversed_write_nothing() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("declare-rejects").await;
            let before = ledger_fingerprint(&state).await;
            let cases: Vec<(&str, DeclareLotCostBasisInput)> = vec![
                (
                    "known-basis buy",
                    DeclareLotCostBasisInput {
                        origin_holding_id: None,
                        activity_leg_id: Some(FIFO_BUY1_LEG.to_owned()),
                        instrument_id: VOO.to_owned(),
                        declared_cost: "200".to_owned(),
                        declared_currency: "USD".to_owned(),
                        acquired_on: None,
                        note: None,
                    },
                ),
                (
                    "zero-gross known buy",
                    DeclareLotCostBasisInput {
                        origin_holding_id: None,
                        activity_leg_id: Some(ZERO_GROSS_LEG.to_owned()),
                        instrument_id: "26262626-2626-4262-8262-262626262626".to_owned(),
                        declared_cost: "0".to_owned(),
                        declared_currency: "USD".to_owned(),
                        acquired_on: None,
                        note: None,
                    },
                ),
                (
                    "nonexistent",
                    origin_qqq_declare("1500", "USD", None).pipe_holding(UNKNOWN_UUID),
                ),
                (
                    "other-household-style holding not in origin",
                    origin_qqq_declare("1500", "USD", None).pipe_holding(FIFO_VOO_HOLDING),
                ),
                (
                    "mismatched instrument",
                    DeclareLotCostBasisInput {
                        origin_holding_id: Some(ORIGIN_QQQ_HOLDING.to_owned()),
                        activity_leg_id: None,
                        instrument_id: ES3.to_owned(),
                        declared_cost: "1500".to_owned(),
                        declared_currency: "USD".to_owned(),
                        acquired_on: None,
                        note: None,
                    },
                ),
                (
                    "reversed ES3 activity id used as leg",
                    DeclareLotCostBasisInput {
                        origin_holding_id: None,
                        activity_leg_id: Some(REVERSED_ES3_ACTIVITY.to_owned()),
                        instrument_id: ES3.to_owned(),
                        declared_cost: "4".to_owned(),
                        declared_currency: "SGD".to_owned(),
                        acquired_on: None,
                        note: None,
                    },
                ),
                (
                    "reversed ES3 buy holding leg",
                    DeclareLotCostBasisInput {
                        origin_holding_id: None,
                        activity_leg_id: Some(REVERSED_ES3_LEG.to_owned()),
                        instrument_id: ES3.to_owned(),
                        declared_cost: "4".to_owned(),
                        declared_currency: "SGD".to_owned(),
                        acquired_on: None,
                        note: None,
                    },
                ),
                (
                    "sell leg did not open",
                    DeclareLotCostBasisInput {
                        origin_holding_id: None,
                        activity_leg_id: Some(SELL_LEG.to_owned()),
                        instrument_id: VOO.to_owned(),
                        declared_cost: "600".to_owned(),
                        declared_currency: "USD".to_owned(),
                        acquired_on: None,
                        note: None,
                    },
                ),
                (
                    "transfer source did not open",
                    DeclareLotCostBasisInput {
                        origin_holding_id: None,
                        activity_leg_id: Some(TRANSFER_SOURCE_LEG.to_owned()),
                        instrument_id: XLF.to_owned(),
                        declared_cost: "10".to_owned(),
                        declared_currency: "USD".to_owned(),
                        acquired_on: None,
                        note: None,
                    },
                ),
            ];
            for (label, input) in cases {
                let error = declare_lot_cost_basis(&state, input)
                    .await
                    .expect_err(label);
                assert!(
                    matches!(
                        error,
                        AppError::InvalidCostBasisDeclaration { .. }
                            | AppError::CostBasisLotNotFound
                    ),
                    "{label}: {error:?}"
                );
                assert_eq!(ledger_fingerprint(&state).await, before, "{label}");
                assert_eq!(
                    scalar_i64(&state, "SELECT COUNT(*) FROM cost_basis_declarations").await,
                    0,
                    "{label}"
                );
            }
            cleanup(&path);
        });
    }

    #[test]
    fn reject_currency_mismatch_and_acquisition_after_lot_date() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("declare-dates").await;
            let before = ledger_fingerprint(&state).await;
            let currency = declare_lot_cost_basis(&state, origin_qqq_declare("1500", "CNY", None))
                .await
                .expect_err("currency");
            assert!(matches!(
                currency,
                AppError::InvalidCostBasisDeclaration { .. }
            ));
            let after = declare_lot_cost_basis(
                &state,
                origin_qqq_declare("1500", "USD", Some("2026-01-03")),
            )
            .await
            .expect_err("after origin");
            assert!(matches!(
                after,
                AppError::InvalidCostBasisDeclaration { .. }
            ));
            assert_eq!(ledger_fingerprint(&state).await, before);
            let accepted = declare_lot_cost_basis(
                &state,
                origin_qqq_declare("1500", "USD", Some("2020-01-01")),
            )
            .await
            .expect("date before origin is accepted");
            assert_eq!(accepted.acquired_on.as_deref(), Some("2020-01-01"));
            assert_eq!(ledger_fingerprint(&state).await, before);
            cleanup(&path);
        });
    }

    #[test]
    fn revocation_returns_unknown_basis_and_preserves_earlier_row() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("revoke").await;
            let before = ledger_fingerprint(&state).await;
            let declared = declare_lot_cost_basis(&state, origin_qqq_declare("1500", "USD", None))
                .await
                .expect("declare");
            let revoked = revoke_lot_cost_basis(&state, origin_qqq_revoke())
                .await
                .expect("revoke");
            assert!(revoked.is_revocation);
            assert_eq!(revoked.revokes.as_deref(), Some(declared.id.as_str()));
            assert!(revoked.declared_cost.is_none());
            assert!(revoked.declared_currency.is_none());
            assert_eq!(
                scalar_i64(&state, "SELECT COUNT(*) FROM cost_basis_declarations").await,
                2
            );
            let supplying: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM cost_basis_declarations WHERE id = ? AND is_revocation = 0 AND declared_cost = '1500'",
            )
            .bind(&declared.id)
            .fetch_one(state.writable_db().expect("writable"))
            .await
            .expect("preserved supplying row");
            assert_eq!(supplying, 1);
            let database = state.writable_db().expect("writable");
            let mut tx = begin_write_tx(database).await.expect("tx");
            let household_id = require_household_id_tx(&mut tx).await.expect("household");
            let latest = analytics_repositories::latest_declaration_for_lot(
                &mut tx,
                &household_id,
                Some(ORIGIN_QQQ_HOLDING),
                None,
            )
            .await
            .expect("latest");
            let _ = tx.rollback().await;
            let latest = latest.expect("revocation is latest");
            assert!(latest.is_revocation);
            assert_eq!(latest.id, revoked.id);
            assert_eq!(ledger_fingerprint(&state).await, before);
            cleanup(&path);
        });
    }

    #[test]
    fn latest_declaration_is_deterministic_under_equal_timestamps() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("latest-tie").await;
            let database = state.writable_db().expect("writable");
            let mut tx = begin_write_tx(database).await.expect("tx");
            let household_id = require_household_id_tx(&mut tx).await.expect("household");
            let early_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1";
            let late_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2";
            let created_at = "2026-08-19T00:00:00.000Z";
            for id in [early_id, late_id] {
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
            let latest = analytics_repositories::latest_declaration_for_lot(
                &mut tx,
                &household_id,
                Some(ORIGIN_QQQ_HOLDING),
                None,
            )
            .await
            .expect("latest")
            .expect("row");
            finish_write_tx(tx, Ok(())).await.expect("commit");
            assert_eq!(latest.id, late_id);
            assert_eq!(latest.created_at, created_at);
            cleanup(&path);
        });
    }

    #[test]
    fn concurrent_declarations_serialize_and_both_rows_persist() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("concurrent").await;
            let first = declare_lot_cost_basis(&state, origin_qqq_declare("1500", "USD", None));
            let second = declare_lot_cost_basis(&state, origin_qqq_declare("1600", "USD", None));
            let (left, right) = tokio::join!(first, second);
            left.expect("first concurrent declare");
            right.expect("second concurrent declare");
            assert_eq!(
                scalar_i64(&state, "SELECT COUNT(*) FROM cost_basis_declarations").await,
                2
            );
            cleanup(&path);
        });
    }

    #[test]
    fn opening_adjustment_unknown_lot_can_be_declared() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("opening-xlf").await;
            let declared = declare_lot_cost_basis(
                &state,
                DeclareLotCostBasisInput {
                    origin_holding_id: None,
                    activity_leg_id: Some(OPENING_XLF_LEG.to_owned()),
                    instrument_id: XLF.to_owned(),
                    declared_cost: "0".to_owned(),
                    declared_currency: "USD".to_owned(),
                    acquired_on: None,
                    note: None,
                },
            )
            .await
            .expect("opening adjustment is unknown-basis");
            assert_eq!(declared.declared_cost.as_deref(), Some("0"));
            cleanup(&path);
        });
    }

    #[test]
    fn future_version_6_rejects_declaration_writes() {
        tauri::async_runtime::block_on(async {
            let (state, path, before_hash) = blocked_future_state("cost-basis").await;
            let error = declare_lot_cost_basis(&state, origin_qqq_declare("1500", "USD", None))
                .await
                .expect_err("blocked declare");
            assert!(matches!(
                error,
                AppError::UnsupportedNewerDatabase {
                    found: 999,
                    supported: 6
                }
            ));
            let error = revoke_lot_cost_basis(&state, origin_qqq_revoke())
                .await
                .expect_err("blocked revoke");
            assert!(matches!(
                error,
                AppError::UnsupportedNewerDatabase {
                    found: 999,
                    supported: 6
                }
            ));
            assert_eq!(stable_sqlite_hash(&path).await, before_hash);
            cleanup(&path);
        });
    }

    #[test]
    fn error_codes_map_to_dedicated_analytics_codes() {
        use crate::error::ErrorCode;
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
    }

    impl DeclareLotCostBasisInput {
        fn pipe_holding(mut self, holding_id: &str) -> Self {
            self.origin_holding_id = Some(holding_id.to_owned());
            self
        }
    }
}
