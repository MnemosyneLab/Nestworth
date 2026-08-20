//! Realized and unrealized gain over the derived lot ledger.
//!
//! Gain is a read-only interpretation. It never writes, never reads current
//! projection tables as an input, and never capitalizes fees into cost.

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::Serialize;
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    account_service::{self, AccountRecordDto, MoneyDto},
    cost_basis_service, query_count, quote_service,
    reference::{begin_read_tx, finish_read_tx, require_household_tx},
    valuation_service::{self, ValuationSnapshot},
};
use crate::{
    domain::{
        checked_add, checked_sub, endpoint_in_scope, holding_native_value, ActivityId,
        AnalyticsScope, BasisStatus, CalendarDate, ComponentKind, ConsumptionKind, CurrencyCode,
        LotLedger, Money, Quantity, SignedMoney, Timestamp,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SignedMoneyDto {
    pub amount: String,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GainSummaryDto {
    pub realized_gross: Option<SignedMoneyDto>,
    pub realized_net: Option<SignedMoneyDto>,
    pub allocated_fees: Option<SignedMoneyDto>,
    pub unrealized_gross: Option<SignedMoneyDto>,
    pub unexplained_disposal_value: Option<SignedMoneyDto>,
    pub basis_complete: bool,
    pub input_complete: bool,
    pub unknown_basis_quantity: String,
    pub unknown_basis_value: Option<MoneyDto>,
    pub unknown_realized: bool,
    pub reporting_currency: String,
}

#[derive(Debug, Clone)]
pub struct GainPeriod {
    pub start: CalendarDate,
    pub end: CalendarDate,
    pub activity_dates: HashMap<ActivityId, CalendarDate>,
}

pub async fn get_gain_summary(
    state: &AppState,
    scope: AnalyticsScope,
) -> Result<GainSummaryDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_gain_summary_in_tx(&mut tx, scope, None).await;
    finish_read_tx(tx, result).await
}

pub async fn get_gain_summary_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    scope: AnalyticsScope,
    period: Option<&GainPeriod>,
) -> Result<GainSummaryDto, AppError> {
    query_count::record("gain_summary");
    let household = require_household_tx(tx).await?;
    let ledger = cost_basis_service::load_effective_lot_ledger_in_tx(tx, &household.id).await?;
    let accounts = account_service::list_account_records_in_tx(tx, &household.id, true).await?;
    let snapshot = ValuationSnapshot::load(tx, &household.id, &household.base_currency).await?;
    summarize_gain(
        &ledger,
        &snapshot,
        &accounts,
        scope,
        &Timestamp::now(),
        period,
    )
}

pub(crate) fn summarize_gain(
    ledger: &LotLedger,
    snapshot: &ValuationSnapshot,
    accounts: &[AccountRecordDto],
    scope: AnalyticsScope,
    now: &Timestamp,
    period: Option<&GainPeriod>,
) -> Result<GainSummaryDto, AppError> {
    if ledger.has_quantity_shortfall() {
        return Err(AppError::Internal);
    }

    let accounts_by_id: HashMap<&str, &AccountRecordDto> = accounts
        .iter()
        .map(|account| (account.id.as_str(), account))
        .collect();
    let reporting = reporting_currency(scope, snapshot);

    let mut basis_complete = true;
    let mut input_complete = true;
    let mut realized_gross = Decimal::ZERO;
    let mut allocated_fees = Decimal::ZERO;
    let mut has_realized = false;
    let mut unknown_realized = false;
    let mut unrealized_gross = Decimal::ZERO;
    let mut has_unrealized = false;
    let mut unexplained = Decimal::ZERO;
    let mut has_unexplained = false;
    let mut unknown_qty = Decimal::ZERO;
    let mut unknown_value = Decimal::ZERO;
    let mut has_unknown_value = false;

    for lot in ledger.open_lots() {
        if !in_scope(
            scope,
            lot.account_id(),
            lot.instrument_id(),
            &accounts_by_id,
        )? {
            continue;
        }
        if lot.basis() == BasisStatus::Unknown {
            basis_complete = false;
            unknown_qty = checked_add(unknown_qty, lot.quantity_remaining().amount())?;
            match unknown_market_value_base(
                snapshot,
                lot.instrument_id(),
                lot.quantity_remaining(),
                now,
            )? {
                UnknownValue::Converted(value) => {
                    unknown_value = checked_add(unknown_value, value)?;
                    has_unknown_value = true;
                }
                UnknownValue::MissingInput => input_complete = false,
            }
            continue;
        }
        match known_unrealized(snapshot, lot, now)? {
            Some((amount, currency)) => {
                if add_converted(
                    &mut unrealized_gross,
                    snapshot,
                    amount,
                    currency,
                    reporting,
                    now,
                    &mut input_complete,
                )? {
                    has_unrealized = true;
                }
            }
            None => input_complete = false,
        }
    }

    for consumption in ledger.consumptions() {
        if !in_scope(
            scope,
            consumption.account_id(),
            consumption.instrument_id(),
            &accounts_by_id,
        )? {
            continue;
        }
        if !consumption_in_period(consumption.activity_id(), period) {
            continue;
        }
        let unknown = consumption.consumed_cost().is_none();
        if unknown {
            basis_complete = false;
        }
        match consumption.kind() {
            ConsumptionKind::Transfer => {}
            ConsumptionKind::UnexplainedDisposal => {
                if unknown {
                    continue;
                }
                let Some(cost) = consumption.consumed_cost() else {
                    continue;
                };
                let currency = quote_currency(snapshot, consumption.instrument_id())
                    .unwrap_or(snapshot.base_currency());
                if add_converted(
                    &mut unexplained,
                    snapshot,
                    cost,
                    currency,
                    reporting,
                    now,
                    &mut input_complete,
                )? {
                    has_unexplained = true;
                }
            }
            ConsumptionKind::Realized => {
                if unknown {
                    unknown_realized = true;
                    continue;
                }
                let Some(proceeds) = consumption.proceeds_share() else {
                    continue;
                };
                let cost = consumption.consumed_cost().unwrap_or(Decimal::ZERO);
                let acq_fee = consumption
                    .allocated_acquisition_fee()
                    .unwrap_or(Decimal::ZERO);
                let disposal_fee = consumption
                    .allocated_disposal_fee()
                    .unwrap_or(Decimal::ZERO);
                let fees = checked_add(acq_fee, disposal_fee)?;
                let gross = checked_sub(proceeds, cost)?;
                let currency = quote_currency(snapshot, consumption.instrument_id())
                    .unwrap_or(snapshot.base_currency());
                if add_converted(
                    &mut realized_gross,
                    snapshot,
                    gross,
                    currency,
                    reporting,
                    now,
                    &mut input_complete,
                )? {
                    has_realized = true;
                    if add_converted(
                        &mut allocated_fees,
                        snapshot,
                        fees,
                        currency,
                        reporting,
                        now,
                        &mut input_complete,
                    )? {
                        // Fees share the realized reporting currency.
                    }
                }
            }
        }
    }

    let realized = if has_realized {
        Some(rounded_gross_net_fees(
            realized_gross,
            allocated_fees,
            reporting,
        )?)
    } else if unknown_realized {
        None
    } else {
        Some(rounded_gross_net_fees(
            Decimal::ZERO,
            Decimal::ZERO,
            reporting,
        )?)
    };
    let unknown_basis_quantity = Quantity::from_canonical(unknown_qty)?.canonical();
    let unknown_basis_value = if unknown_qty.is_zero() {
        Some(money_dto(Decimal::ZERO, snapshot.base_currency())?)
    } else if has_unknown_value {
        Some(money_dto(unknown_value, snapshot.base_currency())?)
    } else {
        None
    };

    Ok(GainSummaryDto {
        realized_gross: realized.as_ref().map(|value| value.0.clone()),
        realized_net: realized.as_ref().map(|value| value.1.clone()),
        allocated_fees: realized.as_ref().map(|value| value.2.clone()),
        unrealized_gross: if has_unrealized {
            Some(signed_dto(unrealized_gross, reporting)?)
        } else if input_complete && unknown_qty.is_zero() {
            Some(signed_dto(Decimal::ZERO, reporting)?)
        } else {
            None
        },
        unexplained_disposal_value: if has_unexplained {
            Some(signed_dto(unexplained, reporting)?)
        } else {
            Some(signed_dto(Decimal::ZERO, reporting)?)
        },
        basis_complete,
        input_complete,
        unknown_basis_quantity,
        unknown_basis_value,
        unknown_realized,
        reporting_currency: reporting.as_str().to_owned(),
    })
}

pub(crate) fn in_scope(
    scope: AnalyticsScope,
    account_id: crate::domain::AccountId,
    instrument_id: crate::domain::InstrumentId,
    accounts: &HashMap<&str, &AccountRecordDto>,
) -> Result<bool, AppError> {
    let account_key = account_id.to_string();
    let facts = match accounts.get(account_key.as_str()) {
        Some(account) => analytics_scope_facts(account, instrument_id)?,
        None => crate::domain::ScopeEndpointFacts {
            account_id,
            instrument_id: Some(instrument_id),
            component_kind: ComponentKind::HoldingQuantity,
            included_in_net_worth: false,
            included_in_investment: false,
            is_liability: false,
            is_active: false,
        },
    };
    Ok(endpoint_in_scope(scope, &facts))
}

pub(crate) fn analytics_scope_facts(
    account: &AccountRecordDto,
    instrument_id: crate::domain::InstrumentId,
) -> Result<crate::domain::ScopeEndpointFacts, AppError> {
    Ok(crate::domain::ScopeEndpointFacts {
        account_id: crate::domain::AccountId::parse(&account.id)?,
        instrument_id: Some(instrument_id),
        component_kind: ComponentKind::HoldingQuantity,
        included_in_net_worth: account.include_in_net_worth,
        included_in_investment: account.include_in_investment,
        is_liability: valuation_service::account_is_liability(account)?,
        is_active: account.archived_at.is_none(),
    })
}

fn reporting_currency(scope: AnalyticsScope, snapshot: &ValuationSnapshot) -> CurrencyCode {
    match scope {
        AnalyticsScope::Instrument(instrument_id)
        | AnalyticsScope::Holding { instrument_id, .. } => {
            quote_currency(snapshot, instrument_id).unwrap_or(snapshot.base_currency())
        }
        AnalyticsScope::Household | AnalyticsScope::Portfolio | AnalyticsScope::Account(_) => {
            snapshot.base_currency()
        }
    }
}

fn consumption_in_period(activity_id: ActivityId, period: Option<&GainPeriod>) -> bool {
    let Some(period) = period else {
        return true;
    };
    if period.start > period.end {
        return false;
    }
    let Some(date) = period.activity_dates.get(&activity_id) else {
        return false;
    };
    *date >= period.start && *date <= period.end
}

fn add_converted(
    total: &mut Decimal,
    snapshot: &ValuationSnapshot,
    amount: Decimal,
    amount_currency: CurrencyCode,
    reporting: CurrencyCode,
    now: &Timestamp,
    input_complete: &mut bool,
) -> Result<bool, AppError> {
    match convert_to_reporting(snapshot, amount, amount_currency, reporting, now)? {
        Some(converted) => {
            *total = checked_add(*total, converted)?;
            Ok(true)
        }
        None => {
            *input_complete = false;
            Ok(false)
        }
    }
}

fn convert_to_reporting(
    snapshot: &ValuationSnapshot,
    amount: Decimal,
    amount_currency: CurrencyCode,
    reporting: CurrencyCode,
    now: &Timestamp,
) -> Result<Option<Decimal>, AppError> {
    if amount_currency == reporting {
        return Ok(Some(amount));
    }
    let native = Money::from_unrounded(amount, amount_currency);
    let converted = valuation_service::convert_amount(snapshot, native, now)?;
    match converted.base {
        Some(base) if converted.complete && base.currency() == reporting => Ok(Some(base.amount())),
        _ => Ok(None),
    }
}

enum UnknownValue {
    Converted(Decimal),
    MissingInput,
}

fn unknown_market_value_base(
    snapshot: &ValuationSnapshot,
    instrument_id: crate::domain::InstrumentId,
    quantity: Quantity,
    now: &Timestamp,
) -> Result<UnknownValue, AppError> {
    let Some(native) = native_holding_value(snapshot, instrument_id, quantity)? else {
        return Ok(UnknownValue::MissingInput);
    };
    let converted = valuation_service::convert_amount(snapshot, native, now)?;
    match converted.base {
        Some(base) if converted.complete => Ok(UnknownValue::Converted(base.amount())),
        _ => Ok(UnknownValue::MissingInput),
    }
}

fn known_unrealized(
    snapshot: &ValuationSnapshot,
    lot: &crate::domain::OpenLot,
    now: &Timestamp,
) -> Result<Option<(Decimal, CurrencyCode)>, AppError> {
    let _ = now;
    let Some(native) =
        native_holding_value(snapshot, lot.instrument_id(), lot.quantity_remaining())?
    else {
        return Ok(None);
    };
    let Some(remaining_cost) = lot.cost_remaining() else {
        return Ok(None);
    };
    Ok(Some((
        checked_sub(native.amount(), remaining_cost)?,
        native.currency(),
    )))
}

pub(crate) fn native_holding_value(
    snapshot: &ValuationSnapshot,
    instrument_id: crate::domain::InstrumentId,
    quantity: Quantity,
) -> Result<Option<Money>, AppError> {
    let Some(instrument) = snapshot.instrument(&instrument_id.to_string()) else {
        return Ok(None);
    };
    let Some(quote_dto) = snapshot.selected_instrument_quote(instrument) else {
        return Ok(None);
    };
    let quote = quote_service::parse_instrument_quote(quote_dto)?;
    if quote.quote_currency().as_str() != instrument.quote_currency {
        return Ok(None);
    }
    Ok(Some(holding_native_value(quantity, &quote)?))
}

pub(crate) fn quote_currency(
    snapshot: &ValuationSnapshot,
    instrument_id: crate::domain::InstrumentId,
) -> Option<CurrencyCode> {
    snapshot
        .instrument(&instrument_id.to_string())
        .and_then(|instrument| CurrencyCode::parse(&instrument.quote_currency).ok())
}

fn rounded_gross_net_fees(
    gross: Decimal,
    fees: Decimal,
    currency: CurrencyCode,
) -> Result<(SignedMoneyDto, SignedMoneyDto, SignedMoneyDto), AppError> {
    let gross = SignedMoney::from_canonical(gross, currency)?;
    let fees = SignedMoney::from_canonical(fees, currency)?;
    let net = gross.checked_sub(fees)?;
    Ok((signed_from(gross), signed_from(net), signed_from(fees)))
}

fn signed_dto(amount: Decimal, currency: CurrencyCode) -> Result<SignedMoneyDto, AppError> {
    Ok(signed_from(SignedMoney::from_canonical(amount, currency)?))
}

fn signed_from(value: SignedMoney) -> SignedMoneyDto {
    SignedMoneyDto {
        amount: value.canonical_amount(),
        currency: value.currency().as_str().to_owned(),
    }
}

fn money_dto(amount: Decimal, currency: CurrencyCode) -> Result<MoneyDto, AppError> {
    let money = Money::from_canonical(crate::domain::round_to_money_scale(amount)?, currency)?;
    Ok(MoneyDto {
        amount: money.canonical_amount(),
        currency: money.currency().as_str().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::{get_gain_summary, summarize_gain, GainPeriod, GainSummaryDto, SignedMoneyDto};
    use crate::{
        application::{
            account_service::{get_account, AccountRecordDto, MoneyDto},
            cost_basis_service::{
                declare_lot_cost_basis, load_effective_lot_ledger_in_tx, revoke_lot_cost_basis,
                DeclareLotCostBasisInput, RevokeLotCostBasisInput,
            },
            overview_service::get_overview,
            portfolio_service::get_portfolio,
            reference::{begin_read_tx, finish_read_tx, require_household_id_tx},
            valuation_service::{self, ValuationSnapshot},
        },
        domain::{
            replay, ActivityLedgerEvent, AnalyticsScope, BasisStatus, CalendarDate, CurrencyCode,
            LedgerEvent, LotEffect, LotRef, Money, Quantity, Timestamp,
        },
        infrastructure::{database::connect_writable, database_bootstrap::MIGRATOR},
        state::AppState,
        test_support::{cleanup, test_path},
    };
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::str::FromStr;

    const ORIGIN_QQQ_HOLDING: &str = "30303030-3030-4303-8303-303030303030";
    const QQQ: &str = "20202020-2020-4202-8202-202020202020";
    const VOO: &str = "25252525-2525-4252-8252-252525252525";
    const ZERO: &str = "26262626-2626-4262-8262-262626262626";
    const BROKERAGE: &str = "99999999-9999-4999-8999-999999999999";
    const TRANSFER_DEST: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
    const BUY1_ACTIVITY: &str = "01a0188f-861c-7b20-8609-535e345b7c42";
    const BUY1_LEG: &str = "01a0188f-861c-7b20-8609-5363bbc99c48";
    const BUY2_ACTIVITY: &str = "01a0188f-861e-7e70-930b-5f4e2d6cda2d";
    const BUY2_LEG: &str = "01a0188f-861e-7e70-930b-5f578c9baeea";
    const BUY3_ACTIVITY: &str = "01a0188f-86a1-7b20-8609-535e345b7c01";
    const BUY3_LEG: &str = "01a0188f-86a1-7b20-8609-535e345b7c02";
    const SELL_ACTIVITY: &str = "01a0188f-861f-7c20-83d1-4abb57f8ddc0";
    const SELL_LEG: &str = "01a0188f-861f-7c20-83d1-4ac8ea0f6396";
    const SELL2_ACTIVITY: &str = "01a0188f-86a2-7c20-83d1-4abb57f8dd01";
    const SELL2_LEG: &str = "01a0188f-86a2-7c20-83d1-4abb57f8dd02";
    const ZERO_GROSS_LEG: &str = "01a0188f-8621-7a61-a206-bf66455312f8";
    const ES3: &str = "21212121-2121-4212-8212-212121212121";

    fn voo_scope() -> AnalyticsScope {
        AnalyticsScope::Instrument(crate::domain::InstrumentId::parse(VOO).expect("voo"))
    }

    fn qqq_scope() -> AnalyticsScope {
        AnalyticsScope::Instrument(crate::domain::InstrumentId::parse(QQQ).expect("qqq"))
    }

    fn zero_scope() -> AnalyticsScope {
        AnalyticsScope::Instrument(crate::domain::InstrumentId::parse(ZERO).expect("zero"))
    }

    fn brokerage_scope() -> AnalyticsScope {
        AnalyticsScope::Account(crate::domain::AccountId::parse(BROKERAGE).expect("account"))
    }

    fn origin_qqq_declare(cost: &str) -> DeclareLotCostBasisInput {
        DeclareLotCostBasisInput {
            origin_holding_id: Some(ORIGIN_QQQ_HOLDING.to_owned()),
            activity_leg_id: None,
            instrument_id: QQQ.to_owned(),
            declared_cost: cost.to_owned(),
            declared_currency: "USD".to_owned(),
            acquired_on: None,
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
        let path = test_path("v014-p3", name);
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

    async fn effective_ledger(state: &AppState) -> crate::domain::LotLedger {
        let database = state.writable_db().expect("writable");
        let mut tx = begin_read_tx(database).await.expect("tx");
        let household_id = require_household_id_tx(&mut tx).await.expect("household");
        let ledger = load_effective_lot_ledger_in_tx(&mut tx, &household_id)
            .await
            .expect("ledger");
        finish_read_tx(tx, Ok(ledger)).await.expect("rollback")
    }

    fn usd(amount: &str) -> SignedMoneyDto {
        SignedMoneyDto {
            amount: amount.to_owned(),
            currency: "USD".to_owned(),
        }
    }

    fn sgd(amount: &str) -> SignedMoneyDto {
        SignedMoneyDto {
            amount: amount.to_owned(),
            currency: "SGD".to_owned(),
        }
    }

    fn cny(amount: &str) -> SignedMoneyDto {
        SignedMoneyDto {
            amount: amount.to_owned(),
            currency: "CNY".to_owned(),
        }
    }

    fn date(value: &str) -> CalendarDate {
        CalendarDate::parse(value).expect("date")
    }

    fn ts(value: &str) -> Timestamp {
        Timestamp::parse(value).expect("timestamp")
    }

    fn qty(value: &str) -> Quantity {
        Quantity::parse(value).expect("quantity")
    }

    fn usd_money(value: &str) -> Money {
        Money::parse(value, CurrencyCode::USD).expect("money")
    }

    fn sgd_money(value: &str) -> Money {
        Money::parse(value, CurrencyCode::SGD).expect("money")
    }

    fn activity_id(value: &str) -> crate::domain::ActivityId {
        crate::domain::ActivityId::parse(value).expect("activity")
    }

    fn leg_id(value: &str) -> crate::domain::ActivityLegId {
        crate::domain::ActivityLegId::parse(value).expect("leg")
    }

    fn account_id(value: &str) -> crate::domain::AccountId {
        crate::domain::AccountId::parse(value).expect("account")
    }

    fn instrument_id(value: &str) -> crate::domain::InstrumentId {
        crate::domain::InstrumentId::parse(value).expect("instrument")
    }

    fn activity_event(
        activity: &str,
        created_at: &str,
        effective_at: &str,
        effect: LotEffect,
    ) -> LedgerEvent {
        LedgerEvent::Activity(ActivityLedgerEvent {
            activity_id: activity_id(activity),
            created_at: ts(created_at),
            effective_at: ts(effective_at),
            reverses: None,
            reversed_by: None,
            sort_order: 0,
            effect,
        })
    }

    fn brokerage_account() -> AccountRecordDto {
        AccountRecordDto {
            id: BROKERAGE.to_owned(),
            name: "Brokerage".to_owned(),
            primary_category: "investment".to_owned(),
            secondary_category: "brokerage_account".to_owned(),
            tracking_mode: "holdings".to_owned(),
            default_currency: "USD".to_owned(),
            institution_id: None,
            group_id: None,
            note: None,
            logo_asset_id: None,
            include_in_net_worth: true,
            include_in_investment: true,
            include_in_liquid_assets: false,
            opened_on: None,
            closed_on: None,
            sort_order: 0,
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
            updated_at: "2026-01-01T00:00:00.000Z".to_owned(),
            archived_at: None,
            latest_value: None,
            valuation: valuation_service::empty_account_valuation(),
            owners: Vec::new(),
        }
    }

    fn dest_account() -> AccountRecordDto {
        let mut account = brokerage_account();
        account.id = TRANSFER_DEST.to_owned();
        account.name = "Transfer dest".to_owned();
        account
    }

    fn holding_scope(account: &str, instrument: &str) -> AnalyticsScope {
        AnalyticsScope::Holding {
            account_id: account_id(account),
            instrument_id: instrument_id(instrument),
        }
    }

    fn es3_scope() -> AnalyticsScope {
        AnalyticsScope::Instrument(instrument_id(ES3))
    }

    fn instrument_dto(
        id: &str,
        currency: &str,
    ) -> crate::application::instrument_service::InstrumentRecordDto {
        crate::application::instrument_service::InstrumentRecordDto {
            id: id.to_owned(),
            name: "Fixture".to_owned(),
            symbol: None,
            instrument_type: "etf".to_owned(),
            quote_currency: currency.to_owned(),
            market_code: None,
            country_code: None,
            isin: None,
            provider_key: None,
            provider_symbol: None,
            quote_preference: "manual".to_owned(),
            note: None,
            logo_asset_id: None,
            sort_order: 0,
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
            updated_at: "2026-01-01T00:00:00.000Z".to_owned(),
            archived_at: None,
        }
    }

    fn quote_dto(
        instrument_id: &str,
        price: &str,
        currency: &str,
    ) -> crate::application::quote_service::InstrumentQuoteRecordDto {
        crate::application::quote_service::InstrumentQuoteRecordDto {
            id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1".to_owned(),
            instrument_id: instrument_id.to_owned(),
            unit_price: price.to_owned(),
            quote_currency: currency.to_owned(),
            source_kind: "manual".to_owned(),
            source_key: "manual".to_owned(),
            delayed: false,
            quoted_at: "2026-01-18T02:00:00.000Z".to_owned(),
            created_at: "2026-01-18T02:00:00.000Z".to_owned(),
        }
    }

    fn snapshot_with_quotes(
        instruments: Vec<crate::application::instrument_service::InstrumentRecordDto>,
        quotes: Vec<crate::application::quote_service::InstrumentQuoteRecordDto>,
        fx_rate: Option<&str>,
        base: CurrencyCode,
    ) -> ValuationSnapshot {
        let fx = fx_rate
            .map(|rate| vec![("USD", "CNY", rate)])
            .unwrap_or_default();
        snapshot_with_fx(instruments, quotes, &fx, base)
    }

    fn snapshot_with_fx(
        instruments: Vec<crate::application::instrument_service::InstrumentRecordDto>,
        quotes: Vec<crate::application::quote_service::InstrumentQuoteRecordDto>,
        fx: &[(&str, &str, &str)],
        base: CurrencyCode,
    ) -> ValuationSnapshot {
        let mut instrument_map = HashMap::new();
        for instrument in instruments {
            instrument_map.insert(instrument.id.clone(), instrument);
        }
        let mut quote_map = HashMap::new();
        for quote in quotes {
            quote_map.insert(
                (quote.instrument_id.clone(), quote.source_kind.clone()),
                quote,
            );
        }
        let mut fx_quotes = HashMap::new();
        let mut fx_preferences = HashMap::new();
        for (index, (fx_base, fx_quote, rate)) in fx.iter().enumerate() {
            fx_quotes.insert(
                (
                    (*fx_base).to_owned(),
                    (*fx_quote).to_owned(),
                    "manual".to_owned(),
                ),
                crate::application::quote_service::FxQuoteRecordDto {
                    id: format!("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbb{index:02}"),
                    base_currency: (*fx_base).to_owned(),
                    quote_currency: (*fx_quote).to_owned(),
                    rate: (*rate).to_owned(),
                    source_kind: "manual".to_owned(),
                    source_key: "manual".to_owned(),
                    delayed: false,
                    quoted_at: "2026-01-18T02:00:00.000Z".to_owned(),
                    created_at: "2026-01-18T02:00:00.000Z".to_owned(),
                },
            );
            fx_preferences.insert(
                crate::domain::FxPair::new(
                    CurrencyCode::parse(fx_base).expect("base"),
                    CurrencyCode::parse(fx_quote).expect("quote"),
                )
                .expect("pair"),
                crate::domain::QuoteSourceKind::Manual,
            );
        }
        ValuationSnapshot::from_parts(
            "11111111-1111-4111-8111-111111111111".to_owned(),
            base,
            instrument_map,
            Vec::new(),
            Vec::new(),
            quote_map,
            fx_quotes,
            fx_preferences,
        )
    }

    fn assert_gross_minus_net_equals_fees(summary: &GainSummaryDto) {
        let (Some(gross), Some(net), Some(fees)) = (
            summary.realized_gross.as_ref(),
            summary.realized_net.as_ref(),
            summary.allocated_fees.as_ref(),
        ) else {
            return;
        };
        let gross = crate::domain::SignedMoney::parse(
            &gross.amount,
            CurrencyCode::parse(&gross.currency).expect("ccy"),
        )
        .expect("gross");
        let net = crate::domain::SignedMoney::parse(
            &net.amount,
            CurrencyCode::parse(&net.currency).expect("ccy"),
        )
        .expect("net");
        let fees = crate::domain::SignedMoney::parse(
            &fees.amount,
            CurrencyCode::parse(&fees.currency).expect("ccy"),
        )
        .expect("fees");
        assert_eq!(gross.checked_sub(net).expect("diff"), fees);
    }

    fn signed_add(
        left: Option<&SignedMoneyDto>,
        right: Option<&SignedMoneyDto>,
    ) -> Option<SignedMoneyDto> {
        match (left, right) {
            (None, None) => None,
            (Some(value), None) | (None, Some(value)) => Some(value.clone()),
            (Some(left), Some(right)) => {
                let left = crate::domain::SignedMoney::parse(
                    &left.amount,
                    CurrencyCode::parse(&left.currency).expect("ccy"),
                )
                .expect("left");
                let right = crate::domain::SignedMoney::parse(
                    &right.amount,
                    CurrencyCode::parse(&right.currency).expect("ccy"),
                )
                .expect("right");
                let sum = left.checked_add(right).expect("sum");
                Some(SignedMoneyDto {
                    amount: sum.canonical_amount(),
                    currency: sum.currency().as_str().to_owned(),
                })
            }
        }
    }

    fn money_add(left: Option<&MoneyDto>, right: Option<&MoneyDto>) -> Option<MoneyDto> {
        match (left, right) {
            (None, None) => None,
            (Some(value), None) | (None, Some(value)) => Some(value.clone()),
            (Some(left), Some(right)) => {
                let left = Money::parse(
                    &left.amount,
                    CurrencyCode::parse(&left.currency).expect("ccy"),
                )
                .expect("left");
                let right = Money::parse(
                    &right.amount,
                    CurrencyCode::parse(&right.currency).expect("ccy"),
                )
                .expect("right");
                assert_eq!(left.currency(), right.currency());
                Some(MoneyDto {
                    amount: Money::from_canonical(
                        crate::domain::checked_add(left.amount(), right.amount()).expect("add"),
                        left.currency(),
                    )
                    .expect("sum")
                    .canonical_amount(),
                    currency: left.currency().as_str().to_owned(),
                })
            }
        }
    }

    #[test]
    fn gain_and_cost_basis_modules_do_not_use_binary_floats() {
        for source in [
            include_str!("gain_service.rs"),
            include_str!("cost_basis_service.rs"),
        ] {
            let code = source.split("#[cfg(test)]").next().expect("code");
            assert!(!code.contains("f32"));
            assert!(!code.contains("f64"));
        }
    }

    #[test]
    fn golden_fifo_realized_and_unrealized_from_fixture() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("fifo-gain").await;
            let ledger = effective_ledger(&state).await;
            let voo = crate::domain::InstrumentId::parse(VOO).expect("voo");
            let open: Vec<_> = ledger
                .open_lots()
                .iter()
                .filter(|lot| lot.instrument_id() == voo)
                .collect();
            assert_eq!(open.len(), 1);
            assert_eq!(open[0].quantity_remaining().canonical(), "2");
            assert_eq!(open[0].cost_remaining_canonical().as_deref(), Some("240"));
            assert_eq!(open[0].basis(), BasisStatus::Known);
            let gain = ledger
                .realized_gain_totals()
                .expect("totals")
                .expect("realized");
            assert_eq!(gain.consumed_cost_canonical(), "440");
            assert_eq!(gain.realized_gain_gross_canonical(), "160");
            assert_eq!(gain.allocated_fees_canonical(), "14");
            assert_eq!(gain.realized_gain_net_canonical(), "146");

            let summary = get_gain_summary(&state, voo_scope()).await.expect("gain");
            assert_eq!(summary.realized_gross.as_ref(), Some(&usd("160")));
            assert_eq!(summary.allocated_fees.as_ref(), Some(&usd("14")));
            assert_eq!(summary.realized_net.as_ref(), Some(&usd("146")));
            assert_eq!(summary.unrealized_gross.as_ref(), Some(&usd("80")));
            assert!(summary.basis_complete);
            assert!(summary.input_complete);
            assert_eq!(summary.unknown_basis_quantity, "0");
            assert_gross_minus_net_equals_fees(&summary);
            cleanup(&path);
        });
    }

    #[test]
    fn unknown_basis_origin_qqq_excluded_and_valued_in_base() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("unknown-qqq").await;
            let summary = get_gain_summary(&state, qqq_scope()).await.expect("gain");
            assert!(summary.unrealized_gross.is_none());
            assert_eq!(summary.realized_gross.as_ref(), Some(&usd("0")));
            assert!(!summary.unknown_realized);
            assert_eq!(summary.unknown_basis_quantity, "3");
            assert_eq!(
                summary
                    .unknown_basis_value
                    .as_ref()
                    .map(|value| (value.amount.as_str(), value.currency.as_str())),
                Some(("14490", "CNY"))
            );
            assert!(!summary.basis_complete);
            assert!(summary.input_complete);
            cleanup(&path);
        });
    }

    #[test]
    fn declare_origin_qqq_produces_unrealized_600_and_revoke_restores() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("declare-qqq-gain").await;
            let before = get_gain_summary(&state, qqq_scope()).await.expect("before");
            let activities_before = scalar_i64(&state, "SELECT COUNT(*) FROM activities").await;
            let ledger_before = effective_ledger(&state).await;
            let qqq_qty_before = ledger_before
                .open_lots()
                .iter()
                .find(|lot| {
                    lot.lot_ref()
                        == LotRef::OriginHolding(
                            crate::domain::HoldingId::parse(ORIGIN_QQQ_HOLDING).expect("holding"),
                        )
                })
                .expect("origin lot")
                .quantity_remaining()
                .canonical();
            assert_eq!(qqq_qty_before, "3");

            declare_lot_cost_basis(&state, origin_qqq_declare("1500"))
                .await
                .expect("declare");
            let declared = get_gain_summary(&state, qqq_scope())
                .await
                .expect("declared");
            assert_eq!(declared.unrealized_gross.as_ref(), Some(&usd("600")));
            assert!(declared.basis_complete);
            assert!(declared.input_complete);
            assert_eq!(declared.unknown_basis_quantity, "0");
            assert_eq!(
                scalar_i64(&state, "SELECT COUNT(*) FROM activities").await,
                activities_before
            );
            let ledger_declared = effective_ledger(&state).await;
            let origin = ledger_declared
                .open_lots()
                .iter()
                .find(|lot| {
                    lot.lot_ref()
                        == LotRef::OriginHolding(
                            crate::domain::HoldingId::parse(ORIGIN_QQQ_HOLDING).expect("holding"),
                        )
                })
                .expect("origin lot");
            assert_eq!(origin.quantity_remaining().canonical(), "3");
            assert_eq!(origin.cost_remaining_canonical().as_deref(), Some("1500"));
            assert_eq!(origin.basis(), BasisStatus::Known);

            revoke_lot_cost_basis(&state, origin_qqq_revoke())
                .await
                .expect("revoke");
            let revoked = get_gain_summary(&state, qqq_scope())
                .await
                .expect("revoked");
            assert_eq!(revoked, before);
            cleanup(&path);
        });
    }

    #[test]
    fn mixed_known_and_unknown_disposal_reports_known_portion_and_is_incomplete() {
        let events = vec![
            activity_event(
                BUY1_ACTIVITY,
                "2026-01-04T02:00:00.000Z",
                "2026-01-04T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY1_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    gross_settlement: Some(usd_money("200")),
                    acquisition_fee: None,
                },
            ),
            activity_event(
                "dddddddd-dddd-4ddd-8ddd-dddddddddda1",
                "2026-01-05T02:00:00.000Z",
                "2026-01-05T02:00:00.000Z",
                LotEffect::OpeningIncrease {
                    holding_leg_id: leg_id("dddddddd-dddd-4ddd-8ddd-ddddddddddd1"),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("3"),
                },
            ),
            activity_event(
                SELL_ACTIVITY,
                "2026-01-06T02:00:00.000Z",
                "2026-01-06T02:00:00.000Z",
                LotEffect::Sell {
                    holding_leg_id: leg_id(SELL_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("4"),
                    proceeds_gross: Some(usd_money("600")),
                    disposal_fee: Some(usd_money("6")),
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        let snapshot = snapshot_with_quotes(
            vec![instrument_dto(VOO, "USD")],
            vec![quote_dto(VOO, "160", "USD")],
            Some("6.9"),
            CurrencyCode::CNY,
        );
        let summary = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            voo_scope(),
            &ts("2026-01-18T02:00:00.000Z"),
            None,
        )
        .expect("gain");
        assert_eq!(summary.realized_gross.as_ref(), Some(&usd("100")));
        assert_eq!(summary.allocated_fees.as_ref(), Some(&usd("3")));
        assert_eq!(summary.realized_net.as_ref(), Some(&usd("97")));
        assert!(!summary.basis_complete);
        assert_eq!(summary.unknown_basis_quantity, "1");
        assert_gross_minus_net_equals_fees(&summary);
    }

    #[test]
    fn missing_current_quote_excludes_unrealized_and_never_reports_zero_value() {
        let events = vec![
            activity_event(
                BUY1_ACTIVITY,
                "2026-01-04T02:00:00.000Z",
                "2026-01-04T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY1_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    gross_settlement: Some(usd_money("200")),
                    acquisition_fee: Some(usd_money("5")),
                },
            ),
            activity_event(
                BUY2_ACTIVITY,
                "2026-01-05T02:00:00.000Z",
                "2026-01-05T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY2_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("4"),
                    gross_settlement: Some(usd_money("480")),
                    acquisition_fee: Some(usd_money("6")),
                },
            ),
            activity_event(
                SELL_ACTIVITY,
                "2026-01-06T02:00:00.000Z",
                "2026-01-06T02:00:00.000Z",
                LotEffect::Sell {
                    holding_leg_id: leg_id(SELL_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("4"),
                    proceeds_gross: Some(usd_money("600")),
                    disposal_fee: Some(usd_money("6")),
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        let snapshot = snapshot_with_quotes(
            vec![instrument_dto(VOO, "USD")],
            Vec::new(),
            None,
            CurrencyCode::USD,
        );
        let summary = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            voo_scope(),
            &ts("2026-01-18T02:00:00.000Z"),
            None,
        )
        .expect("gain");
        assert_eq!(summary.realized_gross.as_ref(), Some(&usd("160")));
        assert!(summary.unrealized_gross.is_none());
        assert_ne!(
            summary
                .unrealized_gross
                .as_ref()
                .map(|value| value.amount.as_str()),
            Some("0")
        );
        assert!(!summary.input_complete);
        assert!(summary.basis_complete);
        assert_eq!(summary.unknown_basis_quantity, "0");
    }

    #[test]
    fn fees_never_enter_cost_and_gross_minus_net_is_allocated_fees() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("fees-not-cost").await;
            let ledger = effective_ledger(&state).await;
            let voo = crate::domain::InstrumentId::parse(VOO).expect("voo");
            let open = ledger
                .open_lots()
                .iter()
                .find(|lot| lot.instrument_id() == voo)
                .expect("open voo");
            assert_eq!(open.cost_remaining_canonical().as_deref(), Some("240"));
            assert_ne!(open.cost_remaining_canonical().as_deref(), Some("226"));
            let summary = get_gain_summary(&state, voo_scope()).await.expect("gain");
            assert_eq!(summary.allocated_fees.as_ref(), Some(&usd("14")));
            assert_gross_minus_net_equals_fees(&summary);
            cleanup(&path);
        });
    }

    #[test]
    fn four_scopes_are_internally_consistent_for_the_same_lot_set() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("four-scope").await;
            let household = get_gain_summary(&state, AnalyticsScope::Household)
                .await
                .expect("household");
            let portfolio = get_gain_summary(&state, AnalyticsScope::Portfolio)
                .await
                .expect("portfolio");
            let brokerage = get_gain_summary(&state, brokerage_scope())
                .await
                .expect("brokerage");
            let dest = get_gain_summary(
                &state,
                AnalyticsScope::Account(
                    crate::domain::AccountId::parse(TRANSFER_DEST).expect("dest"),
                ),
            )
            .await
            .expect("dest");
            let instruments = [
                QQQ,
                VOO,
                ZERO,
                "21212121-2121-4212-8212-212121212121",
                "23232323-2323-4323-8323-232323232323",
                "27272727-2727-4272-8272-272727272727",
            ];
            let mut unknown_qty = Quantity::parse("0").expect("qty");
            let mut unknown_value = None;
            let mut basis_complete = true;
            let mut input_complete = true;
            for instrument in instruments {
                let part = get_gain_summary(
                    &state,
                    AnalyticsScope::Instrument(
                        crate::domain::InstrumentId::parse(instrument).expect("id"),
                    ),
                )
                .await
                .expect("instrument");
                unknown_value = money_add(
                    unknown_value.as_ref(),
                    part.unknown_basis_value
                        .as_ref()
                        .filter(|value| value.amount != "0"),
                );
                basis_complete &= part.basis_complete;
                input_complete &= part.input_complete;
                let add = Quantity::parse(&part.unknown_basis_quantity).expect("add");
                unknown_qty = Quantity::from_canonical(
                    crate::domain::checked_add(unknown_qty.amount(), add.amount()).expect("sum"),
                )
                .expect("qty");
                match part.reporting_currency.as_str() {
                    "USD" | "SGD" | "CNY" => {}
                    other => panic!("unexpected instrument reporting currency {other}"),
                }
            }
            let mut account_sum_qty =
                Quantity::parse(&brokerage.unknown_basis_quantity).expect("b");
            account_sum_qty = Quantity::from_canonical(
                crate::domain::checked_add(
                    account_sum_qty.amount(),
                    Quantity::parse(&dest.unknown_basis_quantity)
                        .expect("d")
                        .amount(),
                )
                .expect("sum"),
            )
            .expect("qty");
            assert_eq!(household.realized_gross, portfolio.realized_gross);
            assert_eq!(household.realized_net, portfolio.realized_net);
            assert_eq!(household.allocated_fees, portfolio.allocated_fees);
            assert_eq!(household.unrealized_gross, portfolio.unrealized_gross);
            assert_eq!(household.reporting_currency, "CNY");
            assert_eq!(portfolio.reporting_currency, "CNY");
            assert_eq!(brokerage.reporting_currency, "CNY");
            assert_eq!(household.unknown_basis_quantity, unknown_qty.canonical());
            assert_eq!(
                household.unknown_basis_quantity,
                account_sum_qty.canonical()
            );
            assert_eq!(household.unknown_basis_value, unknown_value);
            assert_eq!(household.basis_complete, basis_complete);
            assert_eq!(household.input_complete, input_complete);
            assert_eq!(
                signed_add(
                    brokerage.realized_gross.as_ref(),
                    dest.realized_gross.as_ref()
                ),
                household.realized_gross
            );
            cleanup(&path);
        });
    }

    #[test]
    fn allocation_keeps_full_precision_and_rounds_once_at_dto() {
        let events = vec![
            activity_event(
                BUY1_ACTIVITY,
                "2026-01-04T02:00:00.000Z",
                "2026-01-04T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY1_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("3"),
                    gross_settlement: Some(usd_money("100")),
                    acquisition_fee: None,
                },
            ),
            activity_event(
                SELL_ACTIVITY,
                "2026-01-06T02:00:00.000Z",
                "2026-01-06T02:00:00.000Z",
                LotEffect::Sell {
                    holding_leg_id: leg_id(SELL_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("1"),
                    proceeds_gross: Some(usd_money("40")),
                    disposal_fee: None,
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        let remaining = ledger.open_lots()[0].cost_remaining().expect("remaining");
        let consumed = ledger.consumptions()[0].consumed_cost().expect("consumed");
        assert_eq!(
            crate::domain::checked_add(consumed, remaining).expect("sum"),
            Decimal::from_str("100").expect("100")
        );
        assert_ne!(
            ledger.open_lots()[0].cost_remaining_canonical().as_deref(),
            Some("66.6667")
        );
        let snapshot = snapshot_with_quotes(
            vec![instrument_dto(VOO, "USD")],
            Vec::new(),
            None,
            CurrencyCode::USD,
        );
        let summary = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            voo_scope(),
            &ts("2026-01-18T02:00:00.000Z"),
            None,
        )
        .expect("gain");
        assert_eq!(summary.realized_gross.as_ref(), Some(&usd("6.6667")));
        assert_eq!(summary.realized_net.as_ref(), Some(&usd("6.6667")));
        assert_eq!(summary.allocated_fees.as_ref(), Some(&usd("0")));
        assert_gross_minus_net_equals_fees(&summary);
        assert!(!summary.input_complete);
    }

    #[test]
    fn zero_gross_known_lot_is_not_unknown_basis() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("zero-gross-gain").await;
            let summary = get_gain_summary(&state, zero_scope()).await.expect("gain");
            assert!(summary.basis_complete);
            assert_eq!(summary.unknown_basis_quantity, "0");
            assert!(summary.unrealized_gross.is_some());
            assert_ne!(
                summary
                    .unrealized_gross
                    .as_ref()
                    .map(|value| value.amount.as_str()),
                Some("unknown")
            );
            let ledger = effective_ledger(&state).await;
            let zero = ledger
                .open_lots()
                .iter()
                .find(|lot| lot.lot_ref() == LotRef::Acquisition(leg_id(ZERO_GROSS_LEG)))
                .expect("zero lot");
            assert_eq!(zero.basis(), BasisStatus::Known);
            assert_eq!(zero.cost_remaining_canonical().as_deref(), Some("0"));
            cleanup(&path);
        });
    }

    #[test]
    fn overview_portfolio_and_account_are_byte_identical_after_gain_and_declaration() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("gain-no-side-effects").await;
            let before_overview = get_overview(&state).await.expect("overview");
            let before_portfolio = get_portfolio(&state).await.expect("portfolio");
            let before_account = get_account(&state, BROKERAGE).await.expect("account");
            get_gain_summary(&state, AnalyticsScope::Household)
                .await
                .expect("gain");
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
            declare_lot_cost_basis(&state, origin_qqq_declare("1500"))
                .await
                .expect("declare");
            get_gain_summary(&state, qqq_scope())
                .await
                .expect("declared gain");
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
            cleanup(&path);
        });
    }

    #[test]
    fn in_memory_fifo_unrealized_is_80_at_quote_160() {
        let events = vec![
            activity_event(
                BUY1_ACTIVITY,
                "2026-01-04T02:00:00.000Z",
                "2026-01-04T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY1_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    gross_settlement: Some(usd_money("200")),
                    acquisition_fee: Some(usd_money("5")),
                },
            ),
            activity_event(
                BUY2_ACTIVITY,
                "2026-01-05T02:00:00.000Z",
                "2026-01-05T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY2_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("4"),
                    gross_settlement: Some(usd_money("480")),
                    acquisition_fee: Some(usd_money("6")),
                },
            ),
            activity_event(
                SELL_ACTIVITY,
                "2026-01-06T02:00:00.000Z",
                "2026-01-06T02:00:00.000Z",
                LotEffect::Sell {
                    holding_leg_id: leg_id(SELL_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("4"),
                    proceeds_gross: Some(usd_money("600")),
                    disposal_fee: Some(usd_money("6")),
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        let snapshot = snapshot_with_quotes(
            vec![instrument_dto(VOO, "USD")],
            vec![quote_dto(VOO, "160", "USD")],
            None,
            CurrencyCode::USD,
        );
        let summary = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            voo_scope(),
            &ts("2026-01-18T02:00:00.000Z"),
            None,
        )
        .expect("gain");
        assert_eq!(summary.realized_gross.as_ref(), Some(&usd("160")));
        assert_eq!(summary.unrealized_gross.as_ref(), Some(&usd("80")));
        assert_eq!(summary.allocated_fees.as_ref(), Some(&usd("14")));
        assert_eq!(summary.realized_net.as_ref(), Some(&usd("146")));
        assert_gross_minus_net_equals_fees(&summary);
    }

    fn mixed_usd_sgd_events() -> Vec<LedgerEvent> {
        vec![
            activity_event(
                BUY1_ACTIVITY,
                "2026-01-04T02:00:00.000Z",
                "2026-01-04T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY1_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    gross_settlement: Some(usd_money("200")),
                    acquisition_fee: None,
                },
            ),
            activity_event(
                BUY3_ACTIVITY,
                "2026-01-04T03:00:00.000Z",
                "2026-01-04T03:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY3_LEG),
                    instrument_id: instrument_id(ES3),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("1"),
                    gross_settlement: Some(sgd_money("10")),
                    acquisition_fee: None,
                },
            ),
            activity_event(
                SELL_ACTIVITY,
                "2026-01-06T02:00:00.000Z",
                "2026-01-06T02:00:00.000Z",
                LotEffect::Sell {
                    holding_leg_id: leg_id(SELL_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("1"),
                    proceeds_gross: Some(usd_money("150")),
                    disposal_fee: Some(usd_money("1")),
                },
            ),
            activity_event(
                SELL2_ACTIVITY,
                "2026-01-10T02:00:00.000Z",
                "2026-01-10T02:00:00.000Z",
                LotEffect::Sell {
                    holding_leg_id: leg_id(SELL2_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("1"),
                    proceeds_gross: Some(usd_money("160")),
                    disposal_fee: None,
                },
            ),
        ]
    }

    fn mixed_snapshot(include_sgd_fx: bool) -> ValuationSnapshot {
        let mut fx = vec![("USD", "CNY", "6.9")];
        if include_sgd_fx {
            fx.push(("SGD", "CNY", "5.3"));
        }
        snapshot_with_fx(
            vec![instrument_dto(VOO, "USD"), instrument_dto(ES3, "SGD")],
            vec![quote_dto(VOO, "160", "USD"), quote_dto(ES3, "12", "SGD")],
            &fx,
            CurrencyCode::CNY,
        )
    }

    fn gain_period(start: &str, end: &str, dates: &[(&str, &str)]) -> GainPeriod {
        GainPeriod {
            start: date(start),
            end: date(end),
            activity_dates: dates
                .iter()
                .map(|(activity, on)| (activity_id(activity), date(on)))
                .collect(),
        }
    }

    #[test]
    fn household_aggregates_known_usd_and_sgd_gains_in_base_currency() {
        let ledger = replay(mixed_usd_sgd_events()).expect("replay");
        let snapshot = mixed_snapshot(true);
        let now = ts("2026-01-18T02:00:00.000Z");
        let household = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            AnalyticsScope::Household,
            &now,
            None,
        )
        .expect("household");
        let voo = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            voo_scope(),
            &now,
            None,
        )
        .expect("voo");
        let es3 = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            es3_scope(),
            &now,
            None,
        )
        .expect("es3");
        let account = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            brokerage_scope(),
            &now,
            None,
        )
        .expect("account");
        let portfolio = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            AnalyticsScope::Portfolio,
            &now,
            None,
        )
        .expect("portfolio");
        assert_eq!(household.reporting_currency, "CNY");
        assert_eq!(account.reporting_currency, "CNY");
        assert_eq!(portfolio.reporting_currency, "CNY");
        assert_eq!(voo.reporting_currency, "USD");
        assert_eq!(es3.reporting_currency, "SGD");
        assert_eq!(voo.realized_gross.as_ref(), Some(&usd("110")));
        assert_eq!(voo.allocated_fees.as_ref(), Some(&usd("1")));
        assert_eq!(es3.realized_gross.as_ref(), Some(&sgd("0")));
        assert_eq!(es3.unrealized_gross.as_ref(), Some(&sgd("2")));
        assert_eq!(household.realized_gross.as_ref(), Some(&cny("759")));
        assert_eq!(household.allocated_fees.as_ref(), Some(&cny("6.9")));
        assert_eq!(household.unrealized_gross.as_ref(), Some(&cny("10.6")));
        assert!(household.basis_complete);
        assert!(household.input_complete);
        assert!(!household.unknown_realized);
        assert_eq!(household.realized_gross, account.realized_gross);
        assert_eq!(household.realized_gross, portfolio.realized_gross);
    }

    #[test]
    fn missing_fx_marks_only_the_affected_component_incomplete() {
        let ledger = replay(mixed_usd_sgd_events()).expect("replay");
        let snapshot = mixed_snapshot(false);
        let now = ts("2026-01-18T02:00:00.000Z");
        let household = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            AnalyticsScope::Household,
            &now,
            None,
        )
        .expect("household");
        let voo = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            voo_scope(),
            &now,
            None,
        )
        .expect("voo");
        assert_eq!(voo.realized_gross.as_ref(), Some(&usd("110")));
        assert_eq!(household.realized_gross.as_ref(), Some(&cny("759")));
        assert!(household.unrealized_gross.is_none());
        assert_ne!(
            household
                .unrealized_gross
                .as_ref()
                .map(|value| value.amount.as_str()),
            Some("0")
        );
        assert!(!household.input_complete);
        assert!(household.basis_complete);
        assert_eq!(household.reporting_currency, "CNY");
    }

    #[test]
    fn selected_period_includes_boundary_sales_and_excludes_outside_sales() {
        let ledger = replay(mixed_usd_sgd_events()).expect("replay");
        let snapshot = mixed_snapshot(true);
        let now = ts("2026-01-18T02:00:00.000Z");
        let dates = [
            (BUY1_ACTIVITY, "2026-01-04"),
            (BUY3_ACTIVITY, "2026-01-04"),
            (SELL_ACTIVITY, "2026-01-06"),
            (SELL2_ACTIVITY, "2026-01-10"),
        ];
        let before = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            voo_scope(),
            &now,
            Some(&gain_period("2026-01-01", "2026-01-05", &dates)),
        )
        .expect("before");
        let on_start = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            voo_scope(),
            &now,
            Some(&gain_period("2026-01-06", "2026-01-06", &dates)),
        )
        .expect("start");
        let inside = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            voo_scope(),
            &now,
            Some(&gain_period("2026-01-06", "2026-01-10", &dates)),
        )
        .expect("inside");
        let after = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            voo_scope(),
            &now,
            Some(&gain_period("2026-01-11", "2026-01-18", &dates)),
        )
        .expect("after");
        assert_eq!(before.realized_gross.as_ref(), Some(&usd("0")));
        assert!(!before.unknown_realized);
        assert_eq!(on_start.realized_gross.as_ref(), Some(&usd("50")));
        assert_eq!(inside.realized_gross.as_ref(), Some(&usd("110")));
        assert_eq!(after.realized_gross.as_ref(), Some(&usd("0")));
        assert_ne!(inside.realized_gross, before.realized_gross);
        assert_eq!(inside.unrealized_gross.as_ref(), Some(&usd("0")));
    }

    #[test]
    fn known_basis_without_a_sale_is_zero_realized_not_unknown() {
        let events = vec![activity_event(
            BUY1_ACTIVITY,
            "2026-01-04T02:00:00.000Z",
            "2026-01-04T02:00:00.000Z",
            LotEffect::Buy {
                holding_leg_id: leg_id(BUY1_LEG),
                instrument_id: instrument_id(VOO),
                account_id: account_id(BROKERAGE),
                quantity: qty("1"),
                gross_settlement: Some(usd_money("100")),
                acquisition_fee: None,
            },
        )];
        let ledger = replay(events).expect("replay");
        let snapshot = snapshot_with_quotes(
            vec![instrument_dto(VOO, "USD")],
            vec![quote_dto(VOO, "160", "USD")],
            None,
            CurrencyCode::USD,
        );
        let summary = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            voo_scope(),
            &ts("2026-01-18T02:00:00.000Z"),
            None,
        )
        .expect("gain");
        assert_eq!(summary.realized_gross.as_ref(), Some(&usd("0")));
        assert_eq!(summary.unrealized_gross.as_ref(), Some(&usd("60")));
        assert!(summary.basis_complete);
        assert!(summary.input_complete);
        assert!(!summary.unknown_realized);
    }

    #[test]
    fn unknown_basis_without_a_sale_keeps_realized_zero_and_unrealized_unknown() {
        let events = vec![LedgerEvent::OriginBaseline {
            origin_id: crate::domain::HistoryOriginId::parse(
                "a0a0a0a0-a0a0-4a0a-8a0a-a0a0a0a0a0a0",
            )
            .expect("origin"),
            holding_id: crate::domain::HoldingId::parse(ORIGIN_QQQ_HOLDING).expect("holding"),
            instrument_id: instrument_id(QQQ),
            account_id: account_id(BROKERAGE),
            quantity: qty("3"),
            origin_at: ts("2026-01-02T00:00:00.000Z"),
        }];
        let ledger = replay(events).expect("replay");
        let snapshot = snapshot_with_quotes(
            vec![instrument_dto(QQQ, "USD")],
            vec![quote_dto(QQQ, "160", "USD")],
            None,
            CurrencyCode::USD,
        );
        let summary = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            qqq_scope(),
            &ts("2026-01-18T02:00:00.000Z"),
            None,
        )
        .expect("gain");
        assert_eq!(summary.realized_gross.as_ref(), Some(&usd("0")));
        assert!(summary.unrealized_gross.is_none());
        assert!(!summary.basis_complete);
        assert!(!summary.unknown_realized);
        assert_eq!(summary.unknown_basis_quantity, "3");
    }

    #[test]
    fn missing_quote_marks_unrealized_incomplete_without_zeroing_realized() {
        let events = vec![
            activity_event(
                BUY1_ACTIVITY,
                "2026-01-04T02:00:00.000Z",
                "2026-01-04T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY1_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    gross_settlement: Some(usd_money("200")),
                    acquisition_fee: None,
                },
            ),
            activity_event(
                SELL_ACTIVITY,
                "2026-01-06T02:00:00.000Z",
                "2026-01-06T02:00:00.000Z",
                LotEffect::Sell {
                    holding_leg_id: leg_id(SELL_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("1"),
                    proceeds_gross: Some(usd_money("150")),
                    disposal_fee: None,
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        let snapshot = snapshot_with_quotes(
            vec![instrument_dto(VOO, "USD")],
            Vec::new(),
            None,
            CurrencyCode::USD,
        );
        let summary = summarize_gain(
            &ledger,
            &snapshot,
            &[brokerage_account()],
            voo_scope(),
            &ts("2026-01-18T02:00:00.000Z"),
            None,
        )
        .expect("gain");
        assert_eq!(summary.realized_gross.as_ref(), Some(&usd("50")));
        assert!(summary.unrealized_gross.is_none());
        assert!(!summary.input_complete);
        assert!(summary.basis_complete);
    }

    #[test]
    fn holding_scope_returns_per_account_gain_for_the_same_instrument() {
        let events = vec![
            activity_event(
                BUY1_ACTIVITY,
                "2026-01-04T02:00:00.000Z",
                "2026-01-04T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY1_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    gross_settlement: Some(usd_money("200")),
                    acquisition_fee: None,
                },
            ),
            activity_event(
                BUY2_ACTIVITY,
                "2026-01-05T02:00:00.000Z",
                "2026-01-05T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY2_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(TRANSFER_DEST),
                    quantity: qty("2"),
                    gross_settlement: Some(usd_money("100")),
                    acquisition_fee: None,
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        let snapshot = snapshot_with_quotes(
            vec![instrument_dto(VOO, "USD")],
            vec![quote_dto(VOO, "160", "USD")],
            None,
            CurrencyCode::USD,
        );
        let now = ts("2026-01-18T02:00:00.000Z");
        let accounts = [brokerage_account(), dest_account()];
        let brokerage = summarize_gain(
            &ledger,
            &snapshot,
            &accounts,
            holding_scope(BROKERAGE, VOO),
            &now,
            None,
        )
        .expect("brokerage holding");
        let dest = summarize_gain(
            &ledger,
            &snapshot,
            &accounts,
            holding_scope(TRANSFER_DEST, VOO),
            &now,
            None,
        )
        .expect("dest holding");
        let instrument = summarize_gain(&ledger, &snapshot, &accounts, voo_scope(), &now, None)
            .expect("instrument");
        assert_eq!(brokerage.unrealized_gross.as_ref(), Some(&usd("120")));
        assert_eq!(dest.unrealized_gross.as_ref(), Some(&usd("220")));
        assert_eq!(instrument.unrealized_gross.as_ref(), Some(&usd("340")));
        assert_ne!(brokerage.unrealized_gross, dest.unrealized_gross);
        assert_eq!(brokerage.reporting_currency, "USD");
        assert_eq!(dest.reporting_currency, "USD");
    }
}
