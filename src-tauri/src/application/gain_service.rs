//! Realized and unrealized gain over the derived lot ledger.
//!
//! Gain is a read-only interpretation. It never writes, never reads current
//! projection tables as an input, and never capitalizes fees into cost.

use std::collections::HashMap;

use rust_decimal::Decimal;
use sqlx::{Sqlite, Transaction};

use super::{
    account_service::{self, AccountRecordDto, MoneyDto},
    cost_basis_service, quote_service,
    reference::{begin_read_tx, finish_read_tx, require_household_tx},
    valuation_service::{self, ValuationSnapshot},
};
use crate::{
    domain::{
        checked_add, checked_sub, endpoint_in_scope, holding_native_value, AnalyticsScope,
        BasisStatus, ComponentKind, ConsumptionKind, CurrencyCode, LotLedger, Money, Quantity,
        SignedMoney, Timestamp,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

pub async fn get_gain_summary(
    state: &AppState,
    scope: AnalyticsScope,
) -> Result<GainSummaryDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_gain_summary_in_tx(&mut tx, scope).await;
    finish_read_tx(tx, result).await
}

pub async fn get_gain_summary_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    scope: AnalyticsScope,
) -> Result<GainSummaryDto, AppError> {
    let household = require_household_tx(tx).await?;
    let ledger = cost_basis_service::load_effective_lot_ledger_in_tx(tx, &household.id).await?;
    let accounts = account_service::list_account_records_in_tx(tx, &household.id, true).await?;
    let snapshot = ValuationSnapshot::load(tx, &household.id, &household.base_currency).await?;
    summarize_gain(&ledger, &snapshot, &accounts, scope, &Timestamp::now())
}

pub(crate) fn summarize_gain(
    ledger: &LotLedger,
    snapshot: &ValuationSnapshot,
    accounts: &[AccountRecordDto],
    scope: AnalyticsScope,
    now: &Timestamp,
) -> Result<GainSummaryDto, AppError> {
    if ledger.has_quantity_shortfall() {
        return Err(AppError::Internal);
    }

    let accounts_by_id: HashMap<&str, &AccountRecordDto> = accounts
        .iter()
        .map(|account| (account.id.as_str(), account))
        .collect();

    let mut basis_complete = true;
    let mut input_complete = true;
    let mut realized_gross = Decimal::ZERO;
    let mut allocated_fees = Decimal::ZERO;
    let mut has_realized = false;
    let mut realized_currency: Option<CurrencyCode> = None;
    let mut unrealized_gross = Decimal::ZERO;
    let mut has_unrealized = false;
    let mut unrealized_currency: Option<CurrencyCode> = None;
    let mut unexplained = Decimal::ZERO;
    let mut has_unexplained = false;
    let mut unexplained_currency: Option<CurrencyCode> = None;
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
                add_amount(
                    &mut unrealized_gross,
                    &mut has_unrealized,
                    &mut unrealized_currency,
                    amount,
                    currency,
                )?;
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
                add_amount(
                    &mut unexplained,
                    &mut has_unexplained,
                    &mut unexplained_currency,
                    cost,
                    currency,
                )?;
            }
            ConsumptionKind::Realized => {
                if unknown {
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
                add_amount(
                    &mut realized_gross,
                    &mut has_realized,
                    &mut realized_currency,
                    gross,
                    currency,
                )?;
                allocated_fees = match realized_currency {
                    Some(existing) if existing == currency => checked_add(allocated_fees, fees)?,
                    Some(_) => {
                        return Err(AppError::validation(
                            "currency",
                            "Gain components must use the same currency.",
                        ))
                    }
                    None => fees,
                };
            }
        }
    }

    let realized = if has_realized {
        let currency = realized_currency.expect("realized currency");
        Some(rounded_gross_net_fees(
            realized_gross,
            allocated_fees,
            currency,
        )?)
    } else {
        None
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
            Some(signed_dto(
                unrealized_gross,
                unrealized_currency.expect("unrealized currency"),
            )?)
        } else {
            None
        },
        unexplained_disposal_value: if has_unexplained {
            Some(signed_dto(
                unexplained,
                unexplained_currency.expect("unexplained currency"),
            )?)
        } else {
            None
        },
        basis_complete,
        input_complete,
        unknown_basis_quantity,
        unknown_basis_value,
    })
}

fn in_scope(
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

fn analytics_scope_facts(
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

fn add_amount(
    total: &mut Decimal,
    present: &mut bool,
    currency: &mut Option<CurrencyCode>,
    amount: Decimal,
    amount_currency: CurrencyCode,
) -> Result<(), AppError> {
    if let Some(existing) = *currency {
        if existing != amount_currency {
            return Err(AppError::validation(
                "currency",
                "Gain components must use the same currency.",
            ));
        }
        *total = checked_add(*total, amount)?;
    } else {
        *currency = Some(amount_currency);
        *total = amount;
    }
    *present = true;
    Ok(())
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

fn native_holding_value(
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

fn quote_currency(
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
    use super::{get_gain_summary, summarize_gain, GainSummaryDto, SignedMoneyDto};
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
            replay, ActivityLedgerEvent, AnalyticsScope, BasisStatus, CurrencyCode, LedgerEvent,
            LotEffect, LotRef, Money, Quantity, Timestamp,
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
    const SELL_ACTIVITY: &str = "01a0188f-861f-7c20-83d1-4abb57f8ddc0";
    const SELL_LEG: &str = "01a0188f-861f-7c20-83d1-4ac8ea0f6396";
    const ZERO_GROSS_LEG: &str = "01a0188f-8621-7a61-a206-bf66455312f8";

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

    fn ts(value: &str) -> Timestamp {
        Timestamp::parse(value).expect("timestamp")
    }

    fn qty(value: &str) -> Quantity {
        Quantity::parse(value).expect("quantity")
    }

    fn usd_money(value: &str) -> Money {
        Money::parse(value, CurrencyCode::USD).expect("money")
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
        if let Some(rate) = fx_rate {
            fx_quotes.insert(
                ("USD".to_owned(), "CNY".to_owned(), "manual".to_owned()),
                crate::application::quote_service::FxQuoteRecordDto {
                    id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb1".to_owned(),
                    base_currency: "USD".to_owned(),
                    quote_currency: "CNY".to_owned(),
                    rate: rate.to_owned(),
                    source_kind: "manual".to_owned(),
                    source_key: "manual".to_owned(),
                    delayed: false,
                    quoted_at: "2026-01-18T02:00:00.000Z".to_owned(),
                    created_at: "2026-01-18T02:00:00.000Z".to_owned(),
                },
            );
            fx_preferences.insert(
                crate::domain::FxPair::new(CurrencyCode::CNY, CurrencyCode::USD).expect("pair"),
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
            let mut instrument_sum = GainSummaryDto {
                realized_gross: None,
                realized_net: None,
                allocated_fees: None,
                unrealized_gross: None,
                unexplained_disposal_value: None,
                basis_complete: true,
                input_complete: true,
                unknown_basis_quantity: "0".to_owned(),
                unknown_basis_value: None,
            };
            for instrument in instruments {
                let part = get_gain_summary(
                    &state,
                    AnalyticsScope::Instrument(
                        crate::domain::InstrumentId::parse(instrument).expect("id"),
                    ),
                )
                .await
                .expect("instrument");
                instrument_sum.realized_gross = signed_add(
                    instrument_sum.realized_gross.as_ref(),
                    part.realized_gross.as_ref(),
                );
                instrument_sum.realized_net = signed_add(
                    instrument_sum.realized_net.as_ref(),
                    part.realized_net.as_ref(),
                );
                instrument_sum.allocated_fees = signed_add(
                    instrument_sum.allocated_fees.as_ref(),
                    part.allocated_fees.as_ref(),
                );
                instrument_sum.unrealized_gross = signed_add(
                    instrument_sum.unrealized_gross.as_ref(),
                    part.unrealized_gross.as_ref(),
                );
                instrument_sum.unexplained_disposal_value = signed_add(
                    instrument_sum.unexplained_disposal_value.as_ref(),
                    part.unexplained_disposal_value.as_ref(),
                );
                instrument_sum.unknown_basis_value = money_add(
                    instrument_sum.unknown_basis_value.as_ref(),
                    part.unknown_basis_value
                        .as_ref()
                        .filter(|value| value.amount != "0"),
                );
                instrument_sum.basis_complete &= part.basis_complete;
                instrument_sum.input_complete &= part.input_complete;
                let qty = Quantity::parse(&instrument_sum.unknown_basis_quantity).expect("qty");
                let add = Quantity::parse(&part.unknown_basis_quantity).expect("add");
                instrument_sum.unknown_basis_quantity = Quantity::from_canonical(
                    crate::domain::checked_add(qty.amount(), add.amount()).expect("sum"),
                )
                .expect("qty")
                .canonical();
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
            assert_eq!(household.realized_gross, instrument_sum.realized_gross);
            assert_eq!(household.unrealized_gross, instrument_sum.unrealized_gross);
            assert_eq!(
                household.unknown_basis_quantity,
                instrument_sum.unknown_basis_quantity
            );
            assert_eq!(
                household.unknown_basis_quantity,
                account_sum_qty.canonical()
            );
            assert_eq!(
                household.unknown_basis_value,
                instrument_sum.unknown_basis_value
            );
            assert_eq!(household.basis_complete, instrument_sum.basis_complete);
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
        )
        .expect("gain");
        assert_eq!(summary.realized_gross.as_ref(), Some(&usd("160")));
        assert_eq!(summary.unrealized_gross.as_ref(), Some(&usd("80")));
        assert_eq!(summary.allocated_fees.as_ref(), Some(&usd("14")));
        assert_eq!(summary.realized_net.as_ref(), Some(&usd("146")));
        assert_gross_minus_net_equals_fees(&summary);
    }
}
