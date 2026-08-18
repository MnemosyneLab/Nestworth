use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::Serialize;
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    account_service::{self, AccountRecordDto, MoneyDto},
    quote_service::{self, FxPairStatusDto, FxQuoteRecordDto},
    reference::{begin_read_tx, finish_read_tx, require_household_tx},
    valuation_service::{self, HoldingValuationDto, UnvaluedItemDto, ValuationSnapshot},
};
use crate::{
    domain::{
        canonical_decimal, checked_add, round_to_money_scale, CurrencyCode, Money, PrimaryCategory,
        QuoteSourceKind, Timestamp, TOTAL_BPS,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AllocationRowDto {
    pub key: String,
    pub name: Option<String>,
    pub amount: MoneyDto,
    pub share_bps: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioAccountDto {
    pub account_id: String,
    pub name: String,
    pub base_value: Option<MoneyDto>,
    pub complete: bool,
    pub freshness: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioDto {
    pub base_currency: String,
    pub total: MoneyDto,
    pub is_complete: bool,
    pub coverage_bps: i32,
    pub unvalued_items: Vec<UnvaluedItemDto>,
    pub positions: Vec<HoldingValuationDto>,
    pub accounts: Vec<PortfolioAccountDto>,
    pub cash: Vec<MoneyDto>,
    pub by_currency: Vec<AllocationRowDto>,
    pub by_country: Vec<AllocationRowDto>,
    pub by_instrument_type: Vec<AllocationRowDto>,
    pub required_fx: Vec<FxPairStatusDto>,
}

pub async fn get_portfolio(state: &AppState) -> Result<PortfolioDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_portfolio_in_tx(&mut tx).await;
    finish_read_tx(tx, result).await
}

pub async fn list_required_fx(state: &AppState) -> Result<Vec<FxPairStatusDto>, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let accounts = account_service::list_accounts_in_tx(&mut tx, &household.id, false).await?;
        let snapshot =
            ValuationSnapshot::load(&mut tx, &household.id, &household.base_currency).await?;
        required_fx_status(&mut tx, &household.id, &snapshot, &accounts).await
    }
    .await;
    finish_read_tx(tx, result).await
}

async fn get_portfolio_in_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<PortfolioDto, AppError> {
    let household = require_household_tx(tx).await?;
    let accounts = account_service::list_accounts_in_tx(tx, &household.id, false).await?;
    let snapshot = ValuationSnapshot::load(tx, &household.id, &household.base_currency).await?;
    let now = Timestamp::now();
    let base = CurrencyCode::parse(&household.base_currency)?;
    let included: Vec<&AccountRecordDto> = accounts
        .iter()
        .filter(|account| account.include_in_investment && account.archived_at.is_none())
        .filter(|account| {
            PrimaryCategory::parse(&account.primary_category)
                .map(|primary| primary != PrimaryCategory::Liability)
                .unwrap_or(false)
        })
        .collect();
    let included_ids: std::collections::HashSet<&str> =
        included.iter().map(|account| account.id.as_str()).collect();

    let mut complete_total = Decimal::ZERO;
    let mut complete = true;
    let mut valued_components = 0i32;
    let mut total_components = 0i32;
    let mut unvalued = Vec::new();
    let mut positions = Vec::new();
    let mut by_currency: HashMap<String, Decimal> = HashMap::new();
    let mut by_country: HashMap<String, Decimal> = HashMap::new();
    let mut by_type: HashMap<String, Decimal> = HashMap::new();
    let mut cash_totals: HashMap<String, Decimal> = HashMap::new();

    for holding in valuation_service::snapshot_holdings(&snapshot) {
        if !included_ids.contains(holding.account_id.as_str()) {
            continue;
        }
        let valued = valuation_service::value_holding(&snapshot, holding, &now)?;
        total_components += 1;
        if valued.complete {
            valued_components += 1;
            if let Some(base_value) = &valued.base {
                let amount = Money::parse(&base_value.amount, base)?.amount();
                complete_total = checked_add(complete_total, amount)?;
                if let Some(native) = &valued.native {
                    *by_currency
                        .entry(native.currency.clone())
                        .or_insert(Decimal::ZERO) += amount;
                }
                let country = valued
                    .country_code
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned());
                *by_country.entry(country).or_insert(Decimal::ZERO) += amount;
                *by_type
                    .entry(valued.instrument_type.clone())
                    .or_insert(Decimal::ZERO) += amount;
            }
        } else {
            complete = false;
            unvalued.push(UnvaluedItemDto {
                kind: "holding".to_owned(),
                id: valued.holding_id.clone(),
                name: valued.instrument_name.clone(),
                reason: valued
                    .missing_reason
                    .clone()
                    .unwrap_or_else(|| "quote".to_owned()),
            });
        }
        positions.push(valued);
    }

    for cash in valuation_service::snapshot_cash(&snapshot) {
        if !included_ids.contains(cash.account_id.as_str()) {
            continue;
        }
        let native = Money::parse(&cash.amount, CurrencyCode::parse(&cash.currency)?)?;
        let converted = valuation_service::convert_money(&snapshot, native, &now)?;
        total_components += 1;
        if converted.complete {
            valued_components += 1;
            if let Some(base_value) = converted.base {
                complete_total = checked_add(complete_total, base_value.amount())?;
                *by_currency
                    .entry(cash.currency.clone())
                    .or_insert(Decimal::ZERO) += base_value.amount();
                *by_country
                    .entry("unknown".to_owned())
                    .or_insert(Decimal::ZERO) += base_value.amount();
                *by_type.entry("cash".to_owned()).or_insert(Decimal::ZERO) += base_value.amount();
            }
            *cash_totals
                .entry(cash.currency.clone())
                .or_insert(Decimal::ZERO) += native.amount();
        } else {
            complete = false;
            unvalued.push(UnvaluedItemDto {
                kind: "cash".to_owned(),
                id: cash.id.clone(),
                name: format!("{} cash", cash.currency),
                reason: "fx_quote".to_owned(),
            });
        }
    }

    let required_fx = required_fx_status(tx, &household.id, &snapshot, &accounts).await?;
    let coverage_bps = if total_components == 0 {
        TOTAL_BPS
    } else {
        (valued_components * TOTAL_BPS) / total_components
    };

    Ok(PortfolioDto {
        base_currency: household.base_currency.clone(),
        total: MoneyDto {
            amount: canonical_decimal(round_to_money_scale(complete_total)?),
            currency: household.base_currency.clone(),
        },
        is_complete: complete,
        coverage_bps,
        unvalued_items: unvalued,
        positions,
        accounts: included
            .iter()
            .map(|account| PortfolioAccountDto {
                account_id: account.id.clone(),
                name: account.name.clone(),
                base_value: account.valuation.base.clone(),
                complete: account.valuation.complete,
                freshness: account.valuation.freshness.clone(),
            })
            .collect(),
        cash: cash_totals
            .into_iter()
            .map(|(currency, amount)| MoneyDto {
                amount: canonical_decimal(amount),
                currency,
            })
            .collect(),
        by_currency: allocation_rows(by_currency, complete_total, &household.base_currency),
        by_country: allocation_rows(by_country, complete_total, &household.base_currency),
        by_instrument_type: allocation_rows(by_type, complete_total, &household.base_currency),
        required_fx,
    })
}

async fn required_fx_status(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    snapshot: &ValuationSnapshot,
    accounts: &[AccountRecordDto],
) -> Result<Vec<FxPairStatusDto>, AppError> {
    let pairs = valuation_service::required_fx_pairs(snapshot, accounts)?;
    let quotes = quote_service::list_latest_fx_quotes(tx, household_id).await?;
    let preferences = quote_service::list_fx_preferences(tx, household_id).await?;
    let mut statuses = Vec::new();
    for pair in pairs {
        let preference = preferences
            .iter()
            .find(|(item, _)| *item == pair)
            .map(|(_, source)| *source)
            .unwrap_or(QuoteSourceKind::Manual);
        let selected = quotes.iter().find(|quote| {
            quote.source_kind == preference.as_str()
                && ((quote.base_currency == pair.currency_a().as_str()
                    && quote.quote_currency == pair.currency_b().as_str())
                    || (quote.base_currency == pair.currency_b().as_str()
                        && quote.quote_currency == pair.currency_a().as_str()))
        });
        statuses.push(FxPairStatusDto {
            currency_a: pair.currency_a().as_str().to_owned(),
            currency_b: pair.currency_b().as_str().to_owned(),
            quote_preference: preference.as_str().to_owned(),
            selected_quote: selected.cloned(),
        });
    }
    let _unused: Option<FxQuoteRecordDto> = None;
    let _ = _unused;
    Ok(statuses)
}

fn allocation_rows(
    totals: HashMap<String, Decimal>,
    whole: Decimal,
    currency: &str,
) -> Vec<AllocationRowDto> {
    let mut rows: Vec<(String, Decimal)> =
        totals.into_iter().filter(|row| !row.1.is_zero()).collect();
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    let parts: Vec<Decimal> = rows.iter().map(|row| row.1).collect();
    let shares = allocate(&parts, whole);
    rows.into_iter()
        .zip(shares)
        .map(|(row, share_bps)| AllocationRowDto {
            key: row.0.clone(),
            name: Some(row.0),
            amount: MoneyDto {
                amount: canonical_decimal(row.1),
                currency: currency.to_owned(),
            },
            share_bps,
        })
        .collect()
}

fn allocate(parts: &[Decimal], whole: Decimal) -> Vec<i32> {
    if whole.is_zero() || parts.is_empty() {
        return vec![0; parts.len()];
    }
    let mut floors = vec![0i32; parts.len()];
    let mut remainders = Vec::new();
    let mut allocated = 0i32;
    for (index, part) in parts.iter().enumerate() {
        if !part.is_sign_positive() || part.is_zero() {
            remainders.push((Decimal::ZERO, index));
            continue;
        }
        let raw = *part * Decimal::from(TOTAL_BPS) / whole;
        let floor: i32 = raw.trunc().to_string().parse().unwrap_or(0);
        floors[index] = floor;
        allocated += floor;
        remainders.push((raw - raw.trunc(), index));
    }
    let mut leftover = (TOTAL_BPS - allocated).max(0);
    remainders.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    for (_, index) in remainders {
        if leftover == 0 {
            break;
        }
        if parts[index].is_sign_positive() && !parts[index].is_zero() {
            floors[index] += 1;
            leftover -= 1;
        }
    }
    floors
}
