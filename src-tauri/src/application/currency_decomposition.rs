//! Currency decomposition of realized and unrealized gain.
//!
//! Historical acquisition FX is selected with the existing at-or-before-cutoff
//! rule. Missing `f0` marks only that lot undecomposed. The identity
//! `base gain = instrument movement + currency movement` is exact.

use std::collections::HashMap;

use rust_decimal::Decimal;
use sqlx::{Sqlite, Transaction};

use super::{
    account_service::{self, AccountRecordDto},
    cost_basis_service,
    gain_service::{self, SignedMoneyDto},
    historical_valuation_service::select_fx_quote_at,
    history_repositories::{self, FxPreferenceObservationRecord},
    quote_service::{self, FxQuoteRecordDto},
    reference::{begin_read_tx, finish_read_tx, require_household_tx},
    valuation_service::ValuationSnapshot,
};
use crate::{
    domain::{
        checked_add, checked_div, checked_mul, checked_sub, resolve_local_datetime, ActivityId,
        AmbiguousOffset, AnalyticsScope, BasisStatus, CalendarDate, ConsumptionKind, CurrencyCode,
        FxPair, FxRate, HistoryTimezone, LotLedger, LotOpening, LotRef, QuoteSourceKind, Timestamp,
    },
    error::AppError,
    state::AppState,
};

const STATUS_AVAILABLE: &str = "available";
const STATUS_UNAVAILABLE: &str = "unavailable";
const KIND_UNREALIZED: &str = "unrealized";
const KIND_REALIZED: &str = "realized";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrencyDecomposition {
    pub base_cost: Decimal,
    pub base_value: Decimal,
    pub base_gain: Decimal,
    pub instrument_movement: Decimal,
    pub currency_movement: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LotRefDto {
    pub origin_holding_id: Option<String>,
    pub activity_leg_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LotDecompositionDto {
    pub lot_ref: LotRefDto,
    pub kind: String,
    pub status: String,
    pub native_cost: String,
    pub native_value: Option<String>,
    pub native_gain: Option<SignedMoneyDto>,
    pub acquisition_fx: Option<String>,
    pub ending_fx: Option<String>,
    pub base_cost: Option<SignedMoneyDto>,
    pub base_value: Option<SignedMoneyDto>,
    pub base_gain: Option<SignedMoneyDto>,
    pub instrument_movement: Option<SignedMoneyDto>,
    pub currency_movement: Option<SignedMoneyDto>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrencyDecompositionSummaryDto {
    pub lots: Vec<LotDecompositionDto>,
    pub instrument_movement: Option<SignedMoneyDto>,
    pub currency_movement: Option<SignedMoneyDto>,
    pub base_gain: Option<SignedMoneyDto>,
    pub basis_complete: bool,
    pub input_complete: bool,
}

pub(crate) struct DecompositionView<'a> {
    pub ledger: &'a LotLedger,
    pub snapshot: &'a ValuationSnapshot,
    pub accounts: &'a [AccountRecordDto],
    pub quotes: &'a [FxQuoteRecordDto],
    pub preference_observations: &'a [FxPreferenceObservationRecord],
    pub current_preferences: &'a HashMap<FxPair, QuoteSourceKind>,
    pub timezone: HistoryTimezone,
    pub disposal_at: &'a HashMap<ActivityId, Timestamp>,
    pub now: &'a Timestamp,
    pub base: CurrencyCode,
    pub scope: AnalyticsScope,
}

/// Exact decomposition identity. Inputs stay at full checked precision.
pub fn decompose_identity(
    native_cost: Decimal,
    native_value: Decimal,
    f0: Decimal,
    f1: Decimal,
) -> Result<CurrencyDecomposition, AppError> {
    let base_cost = checked_mul(native_cost, f0)?;
    let base_value = checked_mul(native_value, f1)?;
    let base_gain = checked_sub(base_value, base_cost)?;
    let instrument_movement = checked_mul(checked_sub(native_value, native_cost)?, f0)?;
    let currency_movement = checked_mul(native_value, checked_sub(f1, f0)?)?;
    let reconstructed = checked_add(instrument_movement, currency_movement)?;
    if !checked_sub(reconstructed, base_gain)?.is_zero() {
        return Err(AppError::Internal);
    }
    Ok(CurrencyDecomposition {
        base_cost,
        base_value,
        base_gain,
        instrument_movement,
        currency_movement,
    })
}

pub async fn get_currency_decomposition(
    state: &AppState,
    scope: AnalyticsScope,
) -> Result<CurrencyDecompositionSummaryDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_currency_decomposition_in_tx(&mut tx, scope).await;
    finish_read_tx(tx, result).await
}

pub async fn get_currency_decomposition_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    scope: AnalyticsScope,
) -> Result<CurrencyDecompositionSummaryDto, AppError> {
    let household = require_household_tx(tx).await?;
    let origin = history_repositories::get_origin_by_household(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    let timezone = HistoryTimezone::parse(&origin.timezone)?;
    let ledger = cost_basis_service::load_effective_lot_ledger_in_tx(tx, &household.id).await?;
    let accounts = account_service::list_account_records_in_tx(tx, &household.id, true).await?;
    let snapshot = ValuationSnapshot::load(tx, &household.id, &household.base_currency).await?;
    let quotes = quote_service::list_all_fx_quotes(tx, &household.id).await?;
    let preference_observations =
        history_repositories::list_fx_preference_observations(tx, &household.id).await?;
    let current_preferences: HashMap<FxPair, QuoteSourceKind> =
        quote_service::list_fx_preferences(tx, &household.id)
            .await?
            .into_iter()
            .collect();
    let activities = history_repositories::list_all_activities_asc(tx, &household.id).await?;
    let disposal_at = activities
        .iter()
        .map(|activity| (activity.id(), activity.effective_at().clone()))
        .collect();
    summarize_decomposition(DecompositionView {
        ledger: &ledger,
        snapshot: &snapshot,
        accounts: &accounts,
        quotes: &quotes,
        preference_observations: &preference_observations,
        current_preferences: &current_preferences,
        timezone,
        disposal_at: &disposal_at,
        now: &Timestamp::now(),
        base: snapshot.base_currency(),
        scope,
    })
}

pub(crate) fn summarize_decomposition(
    view: DecompositionView<'_>,
) -> Result<CurrencyDecompositionSummaryDto, AppError> {
    if view.ledger.has_quantity_shortfall() {
        return Err(AppError::Internal);
    }

    let mut lots = Vec::new();
    let mut basis_complete = true;
    let mut input_complete = true;
    let mut instrument_total = Decimal::ZERO;
    let mut currency_total = Decimal::ZERO;
    let mut base_gain_total = Decimal::ZERO;
    let mut has_decomposed = false;

    let accounts_by_id = accounts_map(view.accounts);

    for lot in view.ledger.open_lots() {
        if !gain_service::in_scope(
            view.scope,
            lot.account_id(),
            lot.instrument_id(),
            &accounts_by_id,
        )? {
            continue;
        }
        if lot.basis() == BasisStatus::Unknown {
            basis_complete = false;
            continue;
        }
        let Some(opening) = view.ledger.opening(lot.lot_ref()) else {
            continue;
        };
        let Some(native_cost) = lot.cost_remaining() else {
            basis_complete = false;
            continue;
        };
        let native_value = match gain_service::native_holding_value(
            view.snapshot,
            lot.instrument_id(),
            lot.quantity_remaining(),
        )? {
            Some(value) => value.amount(),
            None => {
                input_complete = false;
                continue;
            }
        };
        let native_currency =
            gain_service::quote_currency(view.snapshot, lot.instrument_id()).unwrap_or(view.base);
        let (row, decomposed) = decompose_lot(
            &view,
            opening,
            lot.lot_ref(),
            KIND_UNREALIZED,
            native_cost,
            Some(native_value),
            native_currency,
            None,
        )?;
        accumulate_lot(
            decomposed.as_ref(),
            &mut instrument_total,
            &mut currency_total,
            &mut base_gain_total,
            &mut has_decomposed,
            &mut input_complete,
        )?;
        lots.push(row);
    }

    for consumption in view.ledger.consumptions() {
        if !gain_service::in_scope(
            view.scope,
            consumption.account_id(),
            consumption.instrument_id(),
            &accounts_by_id,
        )? {
            continue;
        }
        if consumption.kind() != ConsumptionKind::Realized {
            continue;
        }
        if consumption.consumed_cost().is_none() {
            basis_complete = false;
            continue;
        }
        let Some(opening) = view.ledger.opening(consumption.lot_ref()) else {
            continue;
        };
        let native_cost = consumption.consumed_cost().unwrap_or(Decimal::ZERO);
        let native_value = consumption.proceeds_share();
        if native_value.is_none() {
            input_complete = false;
            continue;
        }
        let native_currency =
            gain_service::quote_currency(view.snapshot, consumption.instrument_id())
                .unwrap_or(view.base);
        let disposal_at = view.disposal_at.get(&consumption.activity_id()).cloned();
        let (row, decomposed) = decompose_lot(
            &view,
            opening,
            consumption.lot_ref(),
            KIND_REALIZED,
            native_cost,
            native_value,
            native_currency,
            disposal_at.as_ref(),
        )?;
        accumulate_lot(
            decomposed.as_ref(),
            &mut instrument_total,
            &mut currency_total,
            &mut base_gain_total,
            &mut has_decomposed,
            &mut input_complete,
        )?;
        lots.push(row);
    }

    Ok(CurrencyDecompositionSummaryDto {
        lots,
        instrument_movement: if has_decomposed {
            Some(signed_dto(instrument_total, view.base)?)
        } else {
            None
        },
        currency_movement: if has_decomposed {
            Some(signed_dto(currency_total, view.base)?)
        } else {
            None
        },
        base_gain: if has_decomposed {
            Some(signed_dto(base_gain_total, view.base)?)
        } else {
            None
        },
        basis_complete,
        input_complete,
    })
}

fn accounts_map(accounts: &[AccountRecordDto]) -> HashMap<&str, &AccountRecordDto> {
    accounts
        .iter()
        .map(|account| (account.id.as_str(), account))
        .collect()
}

fn accumulate_lot(
    decomposed: Option<&CurrencyDecomposition>,
    instrument_total: &mut Decimal,
    currency_total: &mut Decimal,
    base_gain_total: &mut Decimal,
    has_decomposed: &mut bool,
    input_complete: &mut bool,
) -> Result<(), AppError> {
    let Some(decomposed) = decomposed else {
        *input_complete = false;
        return Ok(());
    };
    *instrument_total = checked_add(*instrument_total, decomposed.instrument_movement)?;
    *currency_total = checked_add(*currency_total, decomposed.currency_movement)?;
    *base_gain_total = checked_add(*base_gain_total, decomposed.base_gain)?;
    *has_decomposed = true;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn decompose_lot(
    view: &DecompositionView<'_>,
    opening: &LotOpening,
    lot_ref: LotRef,
    kind: &str,
    native_cost: Decimal,
    native_value: Option<Decimal>,
    native_currency: CurrencyCode,
    disposal_at: Option<&Timestamp>,
) -> Result<(LotDecompositionDto, Option<CurrencyDecomposition>), AppError> {
    let native_gain = native_value
        .map(|value| checked_sub(value, native_cost))
        .transpose()?;
    let f0 = acquisition_fx(view, opening, native_currency)?;
    let f1 = ending_fx(view, native_currency, kind, disposal_at)?;
    let decomposed = match (f0, f1, native_value) {
        (Some(f0), Some(f1), Some(value)) => Some(decompose_identity(native_cost, value, f0, f1)?),
        _ => None,
    };
    Ok((
        LotDecompositionDto {
            lot_ref: lot_ref_dto(lot_ref),
            kind: kind.to_owned(),
            status: if decomposed.is_some() {
                STATUS_AVAILABLE.to_owned()
            } else {
                STATUS_UNAVAILABLE.to_owned()
            },
            native_cost: crate::domain::canonical_decimal(native_cost),
            native_value: native_value.map(crate::domain::canonical_decimal),
            native_gain: native_gain
                .map(|amount| signed_dto(amount, native_currency))
                .transpose()?,
            acquisition_fx: f0.map(crate::domain::canonical_decimal),
            ending_fx: f1.map(crate::domain::canonical_decimal),
            base_cost: decomposed
                .as_ref()
                .map(|value| signed_dto(value.base_cost, view.base))
                .transpose()?,
            base_value: decomposed
                .as_ref()
                .map(|value| signed_dto(value.base_value, view.base))
                .transpose()?,
            base_gain: decomposed
                .as_ref()
                .map(|value| signed_dto(value.base_gain, view.base))
                .transpose()?,
            instrument_movement: decomposed
                .as_ref()
                .map(|value| signed_dto(value.instrument_movement, view.base))
                .transpose()?,
            currency_movement: decomposed
                .as_ref()
                .map(|value| signed_dto(value.currency_movement, view.base))
                .transpose()?,
        },
        decomposed,
    ))
}

fn acquisition_fx(
    view: &DecompositionView<'_>,
    opening: &LotOpening,
    native: CurrencyCode,
) -> Result<Option<Decimal>, AppError> {
    let Some(cutoff) = acquisition_cutoff(opening, view.timezone)? else {
        return Ok(None);
    };
    native_to_base_rate(
        view.quotes,
        view.preference_observations,
        view.current_preferences,
        native,
        view.base,
        &cutoff,
    )
}

fn ending_fx(
    view: &DecompositionView<'_>,
    native: CurrencyCode,
    kind: &str,
    disposal_at: Option<&Timestamp>,
) -> Result<Option<Decimal>, AppError> {
    let cutoff = if kind == KIND_REALIZED {
        let Some(disposal_at) = disposal_at else {
            return Ok(None);
        };
        disposal_at
    } else {
        view.now
    };
    native_to_base_rate(
        view.quotes,
        view.preference_observations,
        view.current_preferences,
        native,
        view.base,
        cutoff,
    )
}

fn acquisition_cutoff(
    opening: &LotOpening,
    timezone: HistoryTimezone,
) -> Result<Option<Timestamp>, AppError> {
    if opening.is_declared() {
        return match opening.declared_acquired_on() {
            Some(date) => start_of_local_day(timezone, date).map(Some),
            None => Ok(None),
        };
    }
    Ok(Some(opening.acquired_at().clone()))
}

fn start_of_local_day(
    timezone: HistoryTimezone,
    date: CalendarDate,
) -> Result<Timestamp, AppError> {
    resolve_local_datetime(
        timezone,
        &date.to_ymd(),
        "00:00",
        Some(AmbiguousOffset::Earlier),
    )
    .map(|(timestamp, _)| timestamp)
}

pub(crate) fn native_to_base_rate(
    quotes: &[FxQuoteRecordDto],
    observations: &[FxPreferenceObservationRecord],
    current_preferences: &HashMap<FxPair, QuoteSourceKind>,
    native: CurrencyCode,
    household_base: CurrencyCode,
    cutoff: &Timestamp,
) -> Result<Option<Decimal>, AppError> {
    if native == household_base {
        return Ok(Some(Decimal::ONE));
    }
    let pair = FxPair::new(native, household_base)?;
    let source = preference_at(observations, current_preferences, pair, cutoff)?;
    if let Some(direct) = select_fx_quote_at(
        quotes,
        native.as_str(),
        household_base.as_str(),
        source.as_str(),
        cutoff,
    ) {
        return Ok(Some(FxRate::parse(&direct.rate)?.amount()));
    }
    if let Some(inverse) = select_fx_quote_at(
        quotes,
        household_base.as_str(),
        native.as_str(),
        source.as_str(),
        cutoff,
    ) {
        return Ok(Some(checked_div(
            Decimal::ONE,
            FxRate::parse(&inverse.rate)?.amount(),
        )?));
    }
    Ok(None)
}

fn preference_at(
    observations: &[FxPreferenceObservationRecord],
    current_preferences: &HashMap<FxPair, QuoteSourceKind>,
    pair: FxPair,
    cutoff: &Timestamp,
) -> Result<QuoteSourceKind, AppError> {
    let mut selected: Option<&FxPreferenceObservationRecord> = None;
    for observation in observations {
        let observed = FxPair::new(
            CurrencyCode::parse(&observation.currency_a)?,
            CurrencyCode::parse(&observation.currency_b)?,
        )?;
        if observed != pair {
            continue;
        }
        let effective_at = Timestamp::parse(&observation.effective_at)?;
        if effective_at > *cutoff {
            continue;
        }
        let better = match selected {
            None => true,
            Some(current) => {
                let current_at = Timestamp::parse(&current.effective_at)?;
                effective_at
                    .cmp(&current_at)
                    .then(observation.created_at.cmp(&current.created_at))
                    .then(observation.id.cmp(&current.id))
                    .is_gt()
            }
        };
        if better {
            selected = Some(observation);
        }
    }
    if let Some(observation) = selected {
        return QuoteSourceKind::parse(&observation.source_kind);
    }
    Ok(current_preferences
        .get(&pair)
        .copied()
        .unwrap_or(QuoteSourceKind::Manual))
}

fn lot_ref_dto(lot_ref: LotRef) -> LotRefDto {
    match lot_ref {
        LotRef::OriginHolding(id) => LotRefDto {
            origin_holding_id: Some(id.to_string()),
            activity_leg_id: None,
        },
        LotRef::Acquisition(id) => LotRefDto {
            origin_holding_id: None,
            activity_leg_id: Some(id.to_string()),
        },
    }
}

fn signed_dto(amount: Decimal, currency: CurrencyCode) -> Result<SignedMoneyDto, AppError> {
    let value = crate::domain::SignedMoney::from_canonical(amount, currency)?;
    Ok(SignedMoneyDto {
        amount: value.canonical_amount(),
        currency: value.currency().as_str().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        decompose_identity, get_currency_decomposition, native_to_base_rate, start_of_local_day,
        summarize_decomposition, DecompositionView, STATUS_AVAILABLE, STATUS_UNAVAILABLE,
    };
    use crate::{
        application::{
            account_service::AccountRecordDto,
            cost_basis_service::{declare_lot_cost_basis, DeclareLotCostBasisInput},
            gain_service::get_gain_summary,
            history_repositories::FxPreferenceObservationRecord,
            quote_service::FxQuoteRecordDto,
            valuation_service::{self, ValuationSnapshot},
        },
        domain::{
            checked_add, checked_sub, replay, ActivityId, ActivityLedgerEvent, AnalyticsScope,
            CalendarDate, CurrencyCode, FxPair, HistoryTimezone, LedgerEvent, LotEffect, Money,
            Quantity, QuoteSourceKind, Timestamp,
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
    const BUY1_LEG: &str = "01a0188f-861c-7b20-8609-5363bbc99c48";
    const BUY1_ACTIVITY: &str = "01a0188f-861c-7b20-8609-535e345b7c42";
    const BUY2_LEG: &str = "01a0188f-861e-7e70-930b-5f578c9baeea";
    const BUY2_ACTIVITY: &str = "01a0188f-861e-7e70-930b-5f4e2d6cda2d";

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).expect("decimal")
    }

    fn ts(value: &str) -> Timestamp {
        Timestamp::parse(value).expect("timestamp")
    }

    fn voo_scope() -> AnalyticsScope {
        AnalyticsScope::Instrument(crate::domain::InstrumentId::parse(VOO).expect("voo"))
    }

    fn qqq_scope() -> AnalyticsScope {
        AnalyticsScope::Instrument(crate::domain::InstrumentId::parse(QQQ).expect("qqq"))
    }

    fn fx_quote(
        id: &str,
        base: &str,
        quote: &str,
        rate: &str,
        quoted_at: &str,
        source_kind: &str,
    ) -> FxQuoteRecordDto {
        FxQuoteRecordDto {
            id: id.to_owned(),
            base_currency: base.to_owned(),
            quote_currency: quote.to_owned(),
            rate: rate.to_owned(),
            source_kind: source_kind.to_owned(),
            source_key: source_kind.to_owned(),
            delayed: false,
            quoted_at: quoted_at.to_owned(),
            created_at: quoted_at.to_owned(),
        }
    }

    fn empty_preferences() -> (
        Vec<FxPreferenceObservationRecord>,
        HashMap<FxPair, QuoteSourceKind>,
    ) {
        let pair = FxPair::new(CurrencyCode::CNY, CurrencyCode::USD).expect("pair");
        let mut current = HashMap::new();
        current.insert(pair, QuoteSourceKind::Manual);
        (Vec::new(), current)
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
        let path = test_path("v014-p4", name);
        let _ = fs::remove_file(&path);
        apply_migrations(&path, &[1, 2, 3]).await;
        load_sql(&path, include_str!("../../test-fixtures/v0.1.3.sql")).await;
        let state = AppState::initialize(path.clone()).await;
        (state, path)
    }

    fn origin_qqq_declare(cost: &str, acquired_on: Option<&str>) -> DeclareLotCostBasisInput {
        DeclareLotCostBasisInput {
            origin_holding_id: Some(ORIGIN_QQQ_HOLDING.to_owned()),
            activity_leg_id: None,
            instrument_id: QQQ.to_owned(),
            declared_cost: cost.to_owned(),
            declared_currency: "USD".to_owned(),
            acquired_on: acquired_on.map(ToOwned::to_owned),
            note: None,
        }
    }

    #[test]
    fn currency_decomposition_module_does_not_use_binary_floats() {
        let code = include_str!("currency_decomposition.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("code");
        assert!(!code.contains("f32"));
        assert!(!code.contains("f64"));
    }

    #[test]
    fn contract_identity_200_300_6_5_6_9() {
        let result =
            decompose_identity(dec("200"), dec("300"), dec("6.5"), dec("6.9")).expect("identity");
        assert_eq!(result.base_cost, dec("1300"));
        assert_eq!(result.base_value, dec("2070"));
        assert_eq!(result.base_gain, dec("770"));
        assert_eq!(result.instrument_movement, dec("650"));
        assert_eq!(result.currency_movement, dec("120"));
        assert_eq!(
            result.instrument_movement + result.currency_movement,
            result.base_gain
        );
    }

    #[test]
    fn identity_currency_is_one_and_currency_movement_is_zero() {
        let (observations, current) = empty_preferences();
        let rate = native_to_base_rate(
            &[],
            &observations,
            &current,
            CurrencyCode::CNY,
            CurrencyCode::CNY,
            &ts("2026-01-04T02:00:00.000Z"),
        )
        .expect("rate");
        assert_eq!(rate, Some(Decimal::ONE));
        let result =
            decompose_identity(dec("200"), dec("300"), Decimal::ONE, Decimal::ONE).expect("id");
        assert_eq!(result.currency_movement, Decimal::ZERO);
        assert_eq!(result.base_gain, dec("100"));
        assert_eq!(result.instrument_movement, dec("100"));
        assert_eq!(
            result.instrument_movement + result.currency_movement,
            result.base_gain
        );
    }

    #[test]
    fn missing_acquisition_fx_does_not_use_later_quote_one_or_reciprocal() {
        let quotes = vec![
            fx_quote(
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
                "USD",
                "CNY",
                "6.9",
                "2026-01-06T01:00:00.000Z",
                "manual",
            ),
            fx_quote(
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2",
                "CNY",
                "USD",
                "0.144927536232",
                "2026-01-06T01:00:00.000Z",
                "manual",
            ),
        ];
        let (observations, current) = empty_preferences();
        let cutoff = ts("2026-01-04T02:00:00.000Z");
        let f0 = native_to_base_rate(
            &quotes,
            &observations,
            &current,
            CurrencyCode::USD,
            CurrencyCode::CNY,
            &cutoff,
        )
        .expect("f0");
        assert_eq!(f0, None);
        assert_ne!(f0, Some(dec("6.9")));
        assert_ne!(f0, Some(Decimal::ONE));
        let later = native_to_base_rate(
            &quotes,
            &observations,
            &current,
            CurrencyCode::USD,
            CurrencyCode::CNY,
            &ts("2026-01-06T01:00:00.000Z"),
        )
        .expect("later");
        assert_eq!(later, Some(dec("6.9")));
    }

    #[test]
    fn quote_after_cutoff_is_not_selectable() {
        let quotes = vec![
            fx_quote(
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3",
                "USD",
                "CNY",
                "6.5",
                "2026-01-04T02:00:00.000Z",
                "manual",
            ),
            fx_quote(
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa4",
                "USD",
                "CNY",
                "6.9",
                "2026-01-06T01:00:00.000Z",
                "manual",
            ),
        ];
        let (observations, current) = empty_preferences();
        let f0 = native_to_base_rate(
            &quotes,
            &observations,
            &current,
            CurrencyCode::USD,
            CurrencyCode::CNY,
            &ts("2026-01-04T02:00:00.000Z"),
        )
        .expect("f0");
        assert_eq!(f0, Some(dec("6.5")));
        assert_ne!(f0, Some(dec("6.9")));
    }

    #[test]
    fn remaining_open_lot_identity_from_fixture() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("open-lot-identity").await;
            let summary = get_currency_decomposition(&state, voo_scope())
                .await
                .expect("decomp");
            let open = summary
                .lots
                .iter()
                .find(|lot| {
                    lot.kind == "unrealized"
                        && lot.lot_ref.activity_leg_id.as_deref() == Some(BUY2_LEG)
                })
                .expect("open voo");
            assert_eq!(open.status, STATUS_AVAILABLE);
            assert_eq!(open.native_cost, "240");
            assert_eq!(open.native_value.as_deref(), Some("320"));
            assert_eq!(open.acquisition_fx.as_deref(), Some("6.5"));
            let result = decompose_identity(dec("240"), dec("320"), dec("6.5"), dec("6.9"))
                .expect("identity");
            assert_eq!(
                result.instrument_movement + result.currency_movement,
                result.base_gain
            );
            assert_eq!(
                open.instrument_movement.as_ref().map(|v| v.amount.as_str()),
                Some("520")
            );
            assert_eq!(
                open.currency_movement.as_ref().map(|v| v.amount.as_str()),
                Some("128")
            );
            assert_eq!(
                open.base_gain.as_ref().map(|v| v.amount.as_str()),
                Some("648")
            );
            cleanup(&path);
        });
    }

    #[test]
    fn identity_holds_for_every_fixture_lot_with_f0() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("all-lots-identity").await;
            let summary = get_currency_decomposition(&state, AnalyticsScope::Household)
                .await
                .expect("decomp");
            let mut counted = 0;
            for lot in &summary.lots {
                let Some(f0) = lot.acquisition_fx.as_deref() else {
                    continue;
                };
                counted += 1;
                let native_cost = dec(&lot.native_cost);
                let native_value = dec(lot.native_value.as_deref().expect("value"));
                let f1 = dec(lot.ending_fx.as_deref().expect("f1"));
                let result =
                    decompose_identity(native_cost, native_value, dec(f0), f1).expect("identity");
                assert!(
                    checked_sub(
                        checked_add(result.instrument_movement, result.currency_movement)
                            .expect("sum"),
                        result.base_gain
                    )
                    .expect("diff")
                    .is_zero(),
                    "identity failed for {:?}",
                    lot.lot_ref
                );
                assert_eq!(lot.status, STATUS_AVAILABLE);
            }
            assert!(counted >= 3, "expected decomposed lots, got {counted}");
            cleanup(&path);
        });
    }

    #[test]
    fn fifo_buy1_consumption_uses_6_5_not_later_6_9() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("buy1-f0").await;
            let summary = get_currency_decomposition(&state, voo_scope())
                .await
                .expect("decomp");
            let consumed = summary
                .lots
                .iter()
                .find(|lot| {
                    lot.kind == "realized"
                        && lot.lot_ref.activity_leg_id.as_deref() == Some(BUY1_LEG)
                })
                .expect("buy1 consumption");
            assert_eq!(consumed.native_cost, "200");
            assert_eq!(consumed.native_value.as_deref(), Some("300"));
            assert_eq!(consumed.acquisition_fx.as_deref(), Some("6.5"));
            assert_eq!(
                consumed.base_cost.as_ref().map(|v| v.amount.as_str()),
                Some("1300")
            );
            assert_eq!(
                consumed.base_value.as_ref().map(|v| v.amount.as_str()),
                Some("2070")
            );
            assert_eq!(
                consumed.base_gain.as_ref().map(|v| v.amount.as_str()),
                Some("770")
            );
            assert_eq!(
                consumed
                    .instrument_movement
                    .as_ref()
                    .map(|v| v.amount.as_str()),
                Some("650")
            );
            assert_eq!(
                consumed
                    .currency_movement
                    .as_ref()
                    .map(|v| v.amount.as_str()),
                Some("120")
            );
            cleanup(&path);
        });
    }

    #[test]
    fn missing_f0_marks_only_that_lot_undecomposed() {
        let events = vec![
            LedgerEvent::Activity(ActivityLedgerEvent {
                activity_id: ActivityId::parse(BUY1_ACTIVITY).expect("a"),
                created_at: ts("2026-01-04T02:00:00.000Z"),
                effective_at: ts("2026-01-04T02:00:00.000Z"),
                reverses: None,
                reversed_by: None,
                sort_order: 0,
                effect: LotEffect::Buy {
                    holding_leg_id: crate::domain::ActivityLegId::parse(BUY1_LEG).expect("leg"),
                    instrument_id: crate::domain::InstrumentId::parse(VOO).expect("voo"),
                    account_id: crate::domain::AccountId::parse(BROKERAGE).expect("acct"),
                    quantity: Quantity::parse("2").expect("qty"),
                    gross_settlement: Some(Money::parse("200", CurrencyCode::USD).expect("m")),
                    acquisition_fee: None,
                },
            }),
            LedgerEvent::Activity(ActivityLedgerEvent {
                activity_id: ActivityId::parse(BUY2_ACTIVITY).expect("a"),
                created_at: ts("2026-01-05T02:00:00.000Z"),
                effective_at: ts("2026-01-05T02:00:00.000Z"),
                reverses: None,
                reversed_by: None,
                sort_order: 0,
                effect: LotEffect::Buy {
                    holding_leg_id: crate::domain::ActivityLegId::parse(BUY2_LEG).expect("leg"),
                    instrument_id: crate::domain::InstrumentId::parse(VOO).expect("voo"),
                    account_id: crate::domain::AccountId::parse(BROKERAGE).expect("acct"),
                    quantity: Quantity::parse("2").expect("qty"),
                    gross_settlement: Some(Money::parse("240", CurrencyCode::USD).expect("m")),
                    acquisition_fee: None,
                },
            }),
        ];
        let ledger = replay(events).expect("replay");
        let quotes = vec![fx_quote(
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa5",
            "USD",
            "CNY",
            "6.9",
            "2026-01-04T12:00:00.000Z",
            "manual",
        )];
        let (observations, current) = empty_preferences();
        let snapshot = snapshot_with_quote(VOO, "160");
        let accounts = vec![brokerage_account()];
        let mut disposal_at = HashMap::new();
        disposal_at.insert(
            ActivityId::parse(BUY1_ACTIVITY).expect("a"),
            ts("2026-01-04T02:00:00.000Z"),
        );
        let summary = summarize_decomposition(DecompositionView {
            ledger: &ledger,
            snapshot: &snapshot,
            accounts: &accounts,
            quotes: &quotes,
            preference_observations: &observations,
            current_preferences: &current,
            timezone: HistoryTimezone::parse("Asia/Singapore").expect("tz"),
            disposal_at: &disposal_at,
            now: &ts("2026-01-18T02:00:00.000Z"),
            base: CurrencyCode::CNY,
            scope: voo_scope(),
        })
        .expect("summary");
        let early = summary
            .lots
            .iter()
            .find(|lot| lot.lot_ref.activity_leg_id.as_deref() == Some(BUY1_LEG))
            .expect("buy1");
        let later = summary
            .lots
            .iter()
            .find(|lot| lot.lot_ref.activity_leg_id.as_deref() == Some(BUY2_LEG))
            .expect("buy2");
        assert_eq!(early.status, STATUS_UNAVAILABLE);
        assert!(early.acquisition_fx.is_none());
        assert_ne!(early.acquisition_fx.as_deref(), Some("6.9"));
        assert_ne!(early.acquisition_fx.as_deref(), Some("1"));
        assert!(early.instrument_movement.is_none());
        assert_eq!(later.status, STATUS_AVAILABLE);
        assert_eq!(later.acquisition_fx.as_deref(), Some("6.9"));
        assert!(!summary.input_complete);
    }

    #[test]
    fn declared_lot_without_acquired_on_is_undecomposed_and_date_enables_it() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("declared-acquired-on").await;
            declare_lot_cost_basis(&state, origin_qqq_declare("1500", None))
                .await
                .expect("declare");
            let without_date = get_currency_decomposition(&state, qqq_scope())
                .await
                .expect("without");
            let lot = without_date
                .lots
                .iter()
                .find(|lot| lot.lot_ref.origin_holding_id.as_deref() == Some(ORIGIN_QQQ_HOLDING))
                .expect("origin lot");
            assert_eq!(lot.status, STATUS_UNAVAILABLE);
            assert!(lot.acquisition_fx.is_none());
            let native_gain = lot
                .native_gain
                .as_ref()
                .map(|value| value.amount.as_str())
                .expect("native gain");
            assert_eq!(native_gain, "600");
            let gain_without = get_gain_summary(&state, qqq_scope()).await.expect("gain");
            assert_eq!(
                gain_without
                    .unrealized_gross
                    .as_ref()
                    .map(|value| value.amount.as_str()),
                Some("600")
            );

            declare_lot_cost_basis(&state, origin_qqq_declare("1500", Some("2026-01-02")))
                .await
                .expect("declare date");
            let with_date = get_currency_decomposition(&state, qqq_scope())
                .await
                .expect("with");
            let lot = with_date
                .lots
                .iter()
                .find(|lot| lot.lot_ref.origin_holding_id.as_deref() == Some(ORIGIN_QQQ_HOLDING))
                .expect("origin lot");
            assert_eq!(lot.status, STATUS_AVAILABLE);
            assert!(lot.acquisition_fx.is_some());
            assert_eq!(
                lot.native_gain.as_ref().map(|value| value.amount.as_str()),
                Some("600")
            );
            let gain_with = get_gain_summary(&state, qqq_scope()).await.expect("gain");
            assert_eq!(gain_with.unrealized_gross, gain_without.unrealized_gross);
            assert_eq!(lot.native_cost, "1500");
            assert_eq!(lot.native_value.as_deref(), Some("2100"));
            cleanup(&path);
        });
    }

    #[test]
    fn declared_acquired_on_uses_start_of_local_day() {
        let timezone = HistoryTimezone::parse("Asia/Singapore").expect("tz");
        let cutoff = start_of_local_day(timezone, CalendarDate::parse("2026-01-02").expect("date"))
            .expect("start");
        assert_eq!(cutoff.to_rfc3339(), "2026-01-01T16:00:00.000Z");
        let quotes = vec![
            fx_quote(
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa6",
                "USD",
                "CNY",
                "6.8",
                "2026-01-01T00:00:00.000Z",
                "manual",
            ),
            fx_quote(
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa7",
                "USD",
                "CNY",
                "6.9",
                "2026-01-02T00:00:00.000Z",
                "manual",
            ),
        ];
        let (observations, current) = empty_preferences();
        let f0 = native_to_base_rate(
            &quotes,
            &observations,
            &current,
            CurrencyCode::USD,
            CurrencyCode::CNY,
            &cutoff,
        )
        .expect("f0");
        assert_eq!(f0, Some(dec("6.8")));
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

    fn snapshot_with_quote(instrument_id: &str, price: &str) -> ValuationSnapshot {
        let mut instruments = HashMap::new();
        instruments.insert(
            instrument_id.to_owned(),
            crate::application::instrument_service::InstrumentRecordDto {
                id: instrument_id.to_owned(),
                name: "Fixture".to_owned(),
                symbol: None,
                instrument_type: "etf".to_owned(),
                quote_currency: "USD".to_owned(),
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
            },
        );
        let mut quotes = HashMap::new();
        quotes.insert(
            (instrument_id.to_owned(), "manual".to_owned()),
            crate::application::quote_service::InstrumentQuoteRecordDto {
                id: "cccccccc-cccc-4ccc-8ccc-ccccccccccc1".to_owned(),
                instrument_id: instrument_id.to_owned(),
                unit_price: price.to_owned(),
                quote_currency: "USD".to_owned(),
                source_kind: "manual".to_owned(),
                source_key: "manual".to_owned(),
                delayed: false,
                quoted_at: "2026-01-18T02:00:00.000Z".to_owned(),
                created_at: "2026-01-18T02:00:00.000Z".to_owned(),
            },
        );
        ValuationSnapshot::from_parts(
            "11111111-1111-4111-8111-111111111111".to_owned(),
            CurrencyCode::CNY,
            instruments,
            Vec::new(),
            Vec::new(),
            quotes,
            HashMap::new(),
            HashMap::new(),
        )
    }

    #[test]
    fn zero_instrument_is_decomposed_in_fixture() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("zero-decomp").await;
            let summary = get_currency_decomposition(
                &state,
                AnalyticsScope::Instrument(crate::domain::InstrumentId::parse(ZERO).expect("zero")),
            )
            .await
            .expect("decomp");
            assert_eq!(summary.lots.len(), 1);
            assert_eq!(summary.lots[0].status, STATUS_AVAILABLE);
            assert_eq!(summary.lots[0].native_cost, "0");
            cleanup(&path);
        });
    }
}
