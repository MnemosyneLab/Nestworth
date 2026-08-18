use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;
use serde::Serialize;
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    account_service::{AccountRecordDto, MoneyDto},
    cash_service::{self, AccountCashRecordDto},
    holding_service::{self, HoldingRecordDto},
    instrument_service::{self, InstrumentRecordDto},
    quote_service::{self, FxQuoteRecordDto, InstrumentQuoteRecordDto},
};
use crate::{
    domain::{
        canonical_decimal, checked_add, convert_native_to_base, holding_native_value,
        round_to_money_scale, CurrencyCode, Freshness, FxPair, FxQuoteId, FxRate, InstrumentId,
        Money, PrimaryCategory, Quantity, QuoteSourceKind, Timestamp, TrackingMode,
    },
    error::AppError,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UnvaluedItemDto {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountValuationDto {
    pub native: Option<MoneyDto>,
    pub base: Option<MoneyDto>,
    pub complete: bool,
    pub freshness: String,
    pub unvalued_items: Vec<UnvaluedItemDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HoldingValuationDto {
    pub holding_id: String,
    pub account_id: String,
    pub instrument_id: String,
    pub instrument_name: String,
    pub instrument_symbol: Option<String>,
    pub instrument_type: String,
    pub country_code: Option<String>,
    pub quantity: String,
    pub native: Option<MoneyDto>,
    pub base: Option<MoneyDto>,
    pub complete: bool,
    pub freshness: String,
    pub quoted_at: Option<String>,
    pub source_kind: Option<String>,
    pub missing_reason: Option<String>,
}

pub struct ValuationSnapshot {
    #[allow(dead_code)]
    household_id: String,
    base_currency: CurrencyCode,
    instruments: HashMap<String, InstrumentRecordDto>,
    holdings: Vec<HoldingRecordDto>,
    cash: Vec<AccountCashRecordDto>,
    instrument_quotes: HashMap<(String, String), InstrumentQuoteRecordDto>,
    fx_quotes: HashMap<(String, String, String), FxQuoteRecordDto>,
    fx_preferences: HashMap<FxPair, QuoteSourceKind>,
}

impl ValuationSnapshot {
    pub async fn load(
        tx: &mut Transaction<'_, Sqlite>,
        household_id: &str,
        base_currency: &str,
    ) -> Result<Self, AppError> {
        let instruments = instrument_service::list_instruments_in_tx(tx, household_id, true)
            .await?
            .into_iter()
            .map(|instrument| (instrument.id.clone(), instrument))
            .collect();
        let holdings =
            holding_service::list_active_holdings_for_household(tx, household_id).await?;
        let cash = cash_service::list_latest_cash_for_household(tx, household_id).await?;
        let mut instrument_quotes = HashMap::new();
        for quote in quote_service::list_latest_instrument_quotes(tx, household_id).await? {
            instrument_quotes.insert(
                (quote.instrument_id.clone(), quote.source_kind.clone()),
                quote,
            );
        }
        let mut fx_quotes = HashMap::new();
        for quote in quote_service::list_latest_fx_quotes(tx, household_id).await? {
            fx_quotes.insert(
                (
                    quote.base_currency.clone(),
                    quote.quote_currency.clone(),
                    quote.source_kind.clone(),
                ),
                quote,
            );
        }
        let mut fx_preferences = HashMap::new();
        for (pair, source) in quote_service::list_fx_preferences(tx, household_id).await? {
            fx_preferences.insert(pair, source);
        }
        Ok(Self {
            household_id: household_id.to_owned(),
            base_currency: CurrencyCode::parse(base_currency)?,
            instruments,
            holdings,
            cash,
            instrument_quotes,
            fx_quotes,
            fx_preferences,
        })
    }
}

pub fn empty_account_valuation() -> AccountValuationDto {
    AccountValuationDto {
        native: None,
        base: None,
        complete: false,
        freshness: Freshness::Unavailable.as_str().to_owned(),
        unvalued_items: Vec::new(),
    }
}

pub fn enrich_accounts(
    snapshot: &ValuationSnapshot,
    accounts: &mut [AccountRecordDto],
    now: &Timestamp,
) -> Result<(), AppError> {
    for account in accounts {
        account.valuation = value_account(snapshot, account, now)?;
        if account.tracking_mode == TrackingMode::Holdings.as_str() {
            account.latest_value = account.valuation.base.clone();
        }
    }
    Ok(())
}

pub fn value_account(
    snapshot: &ValuationSnapshot,
    account: &AccountRecordDto,
    now: &Timestamp,
) -> Result<AccountValuationDto, AppError> {
    if account.tracking_mode == TrackingMode::Holdings.as_str() {
        return value_holdings_account(snapshot, account, now);
    }
    value_simple_account(snapshot, account, now)
}

fn value_simple_account(
    snapshot: &ValuationSnapshot,
    account: &AccountRecordDto,
    now: &Timestamp,
) -> Result<AccountValuationDto, AppError> {
    let native = match &account.latest_value {
        Some(value) => Money::parse(&value.amount, CurrencyCode::parse(&value.currency)?)?,
        None => Money::parse("0", CurrencyCode::parse(&account.default_currency)?)?,
    };
    let converted = convert_amount(snapshot, native, now)?;
    let mut unvalued = Vec::new();
    if !converted.complete {
        unvalued.push(UnvaluedItemDto {
            kind: "account".to_owned(),
            id: account.id.clone(),
            name: account.name.clone(),
            reason: converted
                .missing_reason
                .clone()
                .unwrap_or_else(|| "fx_quote".to_owned()),
        });
    }
    Ok(AccountValuationDto {
        native: Some(money_dto(native)),
        base: converted.base.map(money_dto),
        complete: converted.complete,
        freshness: converted.freshness.as_str().to_owned(),
        unvalued_items: unvalued,
    })
}

fn value_holdings_account(
    snapshot: &ValuationSnapshot,
    account: &AccountRecordDto,
    now: &Timestamp,
) -> Result<AccountValuationDto, AppError> {
    let mut total = Decimal::ZERO;
    let mut complete = true;
    let mut freshness = Freshness::Manual;
    let mut unvalued = Vec::new();
    for holding in snapshot
        .holdings
        .iter()
        .filter(|holding| holding.account_id == account.id)
    {
        let valued = value_holding(snapshot, holding, now)?;
        if valued.complete {
            if let Some(base) = &valued.base {
                total = checked_add(
                    total,
                    Money::parse(&base.amount, snapshot.base_currency)?.amount(),
                )?;
            }
            freshness = merge_freshness(freshness, Freshness::parse(&valued.freshness)?);
        } else {
            complete = false;
            unvalued.push(UnvaluedItemDto {
                kind: "holding".to_owned(),
                id: holding.id.clone(),
                name: holding.instrument_name.clone(),
                reason: valued
                    .missing_reason
                    .clone()
                    .unwrap_or_else(|| "quote".to_owned()),
            });
        }
    }
    for cash in snapshot
        .cash
        .iter()
        .filter(|cash| cash.account_id == account.id)
    {
        let native = Money::parse(&cash.amount, CurrencyCode::parse(&cash.currency)?)?;
        let converted = convert_amount(snapshot, native, now)?;
        if converted.complete {
            if let Some(base) = converted.base {
                total = checked_add(total, base.amount())?;
            }
            freshness = merge_freshness(freshness, converted.freshness);
        } else {
            complete = false;
            unvalued.push(UnvaluedItemDto {
                kind: "cash".to_owned(),
                id: cash.id.clone(),
                name: format!("{} cash", cash.currency),
                reason: converted
                    .missing_reason
                    .unwrap_or_else(|| "fx_quote".to_owned()),
            });
        }
    }
    let base = if complete {
        Some(MoneyDto {
            amount: canonical_decimal(round_to_money_scale(total)?),
            currency: snapshot.base_currency.as_str().to_owned(),
        })
    } else if total.is_zero() {
        None
    } else {
        Some(MoneyDto {
            amount: canonical_decimal(round_to_money_scale(total)?),
            currency: snapshot.base_currency.as_str().to_owned(),
        })
    };
    Ok(AccountValuationDto {
        native: None,
        base,
        complete,
        freshness: if complete {
            freshness.as_str().to_owned()
        } else if unvalued.len()
            == snapshot
                .holdings
                .iter()
                .filter(|holding| holding.account_id == account.id)
                .count()
                + snapshot
                    .cash
                    .iter()
                    .filter(|cash| cash.account_id == account.id)
                    .count()
            && !unvalued.is_empty()
        {
            Freshness::Unavailable.as_str().to_owned()
        } else {
            freshness.as_str().to_owned()
        },
        unvalued_items: unvalued,
    })
}

pub fn value_holding(
    snapshot: &ValuationSnapshot,
    holding: &HoldingRecordDto,
    now: &Timestamp,
) -> Result<HoldingValuationDto, AppError> {
    let instrument = snapshot
        .instruments
        .get(&holding.instrument_id)
        .ok_or_else(|| AppError::not_found("instrument", &holding.instrument_id))?;
    let Some(quote_dto) = selected_instrument_quote(snapshot, instrument) else {
        return Ok(holding_dto(
            snapshot,
            holding,
            instrument,
            None,
            None,
            false,
            Freshness::Unavailable,
            None,
            Some("instrument_quote"),
        ));
    };
    let quote = quote_service::parse_instrument_quote(quote_dto)?;
    if quote.quote_currency().as_str() != instrument.quote_currency {
        return Ok(holding_dto(
            snapshot,
            holding,
            instrument,
            None,
            None,
            false,
            Freshness::Unavailable,
            None,
            Some("instrument_quote"),
        ));
    }
    let native = holding_native_value(Quantity::parse(&holding.quantity)?, &quote)?;
    let converted = convert_amount(snapshot, native, now)?;
    let quote_freshness = Freshness::from_selected_quote(
        quote.source_kind(),
        quote.delayed(),
        quote.quoted_at(),
        now,
    );
    let freshness = merge_freshness(quote_freshness, converted.freshness);
    Ok(holding_dto(
        snapshot,
        holding,
        instrument,
        Some(money_dto(native)),
        converted.base.map(money_dto),
        converted.complete,
        if converted.complete {
            freshness
        } else {
            Freshness::Unavailable
        },
        Some(quote_dto.quoted_at.clone()),
        converted.missing_reason.as_deref(),
    ))
}

#[allow(clippy::too_many_arguments)]
fn holding_dto(
    snapshot: &ValuationSnapshot,
    holding: &HoldingRecordDto,
    instrument: &InstrumentRecordDto,
    native: Option<MoneyDto>,
    base: Option<MoneyDto>,
    complete: bool,
    freshness: Freshness,
    quoted_at: Option<String>,
    missing_reason: Option<&str>,
) -> HoldingValuationDto {
    let _ = snapshot;
    HoldingValuationDto {
        holding_id: holding.id.clone(),
        account_id: holding.account_id.clone(),
        instrument_id: holding.instrument_id.clone(),
        instrument_name: instrument.name.clone(),
        instrument_symbol: instrument.symbol.clone(),
        instrument_type: instrument.instrument_type.clone(),
        country_code: instrument.country_code.clone(),
        quantity: holding.quantity.clone(),
        native,
        base,
        complete,
        freshness: freshness.as_str().to_owned(),
        quoted_at,
        source_kind: Some(instrument.quote_preference.clone()),
        missing_reason: missing_reason.map(ToOwned::to_owned),
    }
}

struct Converted {
    base: Option<Money>,
    freshness: Freshness,
    complete: bool,
    missing_reason: Option<String>,
}

fn convert_amount(
    snapshot: &ValuationSnapshot,
    native: Money,
    now: &Timestamp,
) -> Result<Converted, AppError> {
    let converted = convert_native_to_base(
        native,
        snapshot.base_currency,
        selected_fx(snapshot, native.currency(), snapshot.base_currency, now)?,
        selected_fx(snapshot, snapshot.base_currency, native.currency(), now)?,
    )?;
    Ok(Converted {
        base: converted.base,
        freshness: converted.freshness,
        complete: converted.complete,
        missing_reason: converted.missing_reason,
    })
}

fn selected_instrument_quote<'a>(
    snapshot: &'a ValuationSnapshot,
    instrument: &InstrumentRecordDto,
) -> Option<&'a InstrumentQuoteRecordDto> {
    snapshot
        .instrument_quotes
        .get(&(instrument.id.clone(), instrument.quote_preference.clone()))
}

fn selected_fx(
    snapshot: &ValuationSnapshot,
    base: CurrencyCode,
    quote: CurrencyCode,
    now: &Timestamp,
) -> Result<Option<(FxQuoteId, FxRate, Freshness)>, AppError> {
    if base == quote {
        return Ok(None);
    }
    let pair = FxPair::new(base, quote)?;
    let preference = snapshot
        .fx_preferences
        .get(&pair)
        .copied()
        .unwrap_or(QuoteSourceKind::Manual);
    let dto = snapshot.fx_quotes.get(&(
        base.as_str().to_owned(),
        quote.as_str().to_owned(),
        preference.as_str().to_owned(),
    ));
    let Some(dto) = dto else {
        return Ok(None);
    };
    let freshness = Freshness::from_selected_quote(
        QuoteSourceKind::parse(&dto.source_kind)?,
        dto.delayed,
        &Timestamp::parse(&dto.quoted_at)?,
        now,
    );
    Ok(Some((
        FxQuoteId::parse(&dto.id)?,
        FxRate::parse(&dto.rate)?,
        freshness,
    )))
}

fn merge_freshness(left: Freshness, right: Freshness) -> Freshness {
    fn rank(value: Freshness) -> u8 {
        match value {
            Freshness::Unavailable => 4,
            Freshness::Stale => 3,
            Freshness::Delayed => 2,
            Freshness::Manual => 1,
            Freshness::Fresh => 0,
        }
    }
    if rank(right) > rank(left) {
        right
    } else {
        left
    }
}

fn money_dto(money: Money) -> MoneyDto {
    MoneyDto {
        amount: money.canonical_amount(),
        currency: money.currency().as_str().to_owned(),
    }
}

impl Freshness {
    fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "manual" => Ok(Self::Manual),
            "fresh" => Ok(Self::Fresh),
            "delayed" => Ok(Self::Delayed),
            "stale" => Ok(Self::Stale),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err(AppError::Internal),
        }
    }
}

pub fn required_fx_pairs(
    snapshot: &ValuationSnapshot,
    accounts: &[AccountRecordDto],
) -> Result<Vec<FxPair>, AppError> {
    let mut pairs = HashSet::new();
    let included: HashSet<&str> = accounts
        .iter()
        .filter(|account| account.include_in_net_worth && account.archived_at.is_none())
        .map(|account| account.id.as_str())
        .collect();
    for account in accounts
        .iter()
        .filter(|account| included.contains(account.id.as_str()))
    {
        if account.tracking_mode != TrackingMode::Holdings.as_str() {
            if let Some(value) = &account.latest_value {
                let native = CurrencyCode::parse(&value.currency)?;
                if native != snapshot.base_currency {
                    pairs.insert(FxPair::new(native, snapshot.base_currency)?);
                }
            }
        }
    }
    for holding in &snapshot.holdings {
        if !included.contains(holding.account_id.as_str()) {
            continue;
        }
        let native = CurrencyCode::parse(&holding.quote_currency)?;
        if native != snapshot.base_currency {
            pairs.insert(FxPair::new(native, snapshot.base_currency)?);
        }
    }
    for cash in &snapshot.cash {
        if !included.contains(cash.account_id.as_str()) {
            continue;
        }
        let native = CurrencyCode::parse(&cash.currency)?;
        if native != snapshot.base_currency {
            pairs.insert(FxPair::new(native, snapshot.base_currency)?);
        }
    }
    let mut ordered: Vec<FxPair> = pairs.into_iter().collect();
    ordered.sort_by_key(|pair| {
        (
            pair.currency_a().as_str().to_owned(),
            pair.currency_b().as_str().to_owned(),
        )
    });
    Ok(ordered)
}

pub fn required_instrument_ids(
    snapshot: &ValuationSnapshot,
    accounts: &[AccountRecordDto],
) -> Vec<InstrumentId> {
    let included: HashSet<&str> = accounts
        .iter()
        .filter(|account| {
            account.include_in_investment
                && account.archived_at.is_none()
                && account.tracking_mode == TrackingMode::Holdings.as_str()
        })
        .map(|account| account.id.as_str())
        .collect();
    let mut ids = HashSet::new();
    for holding in &snapshot.holdings {
        if included.contains(holding.account_id.as_str()) {
            if let Ok(id) = InstrumentId::parse(&holding.instrument_id) {
                ids.insert(id);
            }
        }
    }
    ids.into_iter().collect()
}

pub fn convert_money(
    snapshot: &ValuationSnapshot,
    native: Money,
    now: &Timestamp,
) -> Result<crate::domain::ConvertedValue, AppError> {
    convert_native_to_base(
        native,
        snapshot.base_currency,
        selected_fx(snapshot, native.currency(), snapshot.base_currency, now)?,
        selected_fx(snapshot, snapshot.base_currency, native.currency(), now)?,
    )
}

pub fn snapshot_holdings(snapshot: &ValuationSnapshot) -> &[HoldingRecordDto] {
    &snapshot.holdings
}

pub fn snapshot_cash(snapshot: &ValuationSnapshot) -> &[AccountCashRecordDto] {
    &snapshot.cash
}

pub fn snapshot_instruments(snapshot: &ValuationSnapshot) -> &HashMap<String, InstrumentRecordDto> {
    &snapshot.instruments
}

pub fn snapshot_base(snapshot: &ValuationSnapshot) -> CurrencyCode {
    snapshot.base_currency
}

pub fn account_is_liability(account: &AccountRecordDto) -> Result<bool, AppError> {
    Ok(PrimaryCategory::parse(&account.primary_category)? == PrimaryCategory::Liability)
}
