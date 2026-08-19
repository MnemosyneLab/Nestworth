//! Computed FX conversion-spread overlay for cross-currency cash Transfers.
//!
//! Ledger facts stay on Activity legs. This overlay is recomputed from those
//! amounts plus market FX quotes at the Activity effective time.

use std::collections::HashMap;

use serde::Serialize;
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    history_repositories::list_latest_fx_preferences_at,
    quote_service::{self, FxQuoteRecordDto},
    valuation_service::{self, ValuationSnapshot},
};
use crate::{
    domain::{
        checked_sub, round_to_money_scale, Activity, ActivityKind, CurrencyCode, FxPair, FxRate,
        LegRole, Money, QuoteSourceKind, Timestamp,
    },
    error::AppError,
};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActivityFxConversionDto {
    pub status: String,
    pub base_currency: String,
    pub source_currency: String,
    pub destination_currency: String,
    pub transaction_rate: String,
    pub transaction_rate_inverse: String,
    pub market_quote_id: Option<String>,
    pub market_base_currency: Option<String>,
    pub market_quote_currency: Option<String>,
    pub market_rate: Option<String>,
    pub source_base: Option<String>,
    pub destination_base: Option<String>,
    pub spread_amount: Option<String>,
    pub spread_currency: Option<String>,
    pub spread_effect: Option<String>,
}

pub async fn overlay_for_activity(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    base_currency: &str,
    activity: &Activity,
) -> Result<Option<ActivityFxConversionDto>, AppError> {
    if activity.kind() != ActivityKind::Transfer {
        return Ok(None);
    }
    let Some((source, destination)) = transfer_native_amounts(activity) else {
        return Ok(None);
    };
    if source.currency() == destination.currency() {
        return Ok(None);
    }
    let transaction_rate = match activity.legs().iter().find_map(|leg| leg.fx_rate()) {
        Some(rate) => rate,
        None => FxRate::from_ratio(destination.amount(), source.amount())?,
    };
    let inverse = FxRate::from_ratio(source.amount(), destination.amount())?;
    let (snapshot, quotes) =
        fx_snapshot_at(tx, household_id, base_currency, activity.effective_at()).await?;
    let source_converted =
        valuation_service::convert_amount(&snapshot, source, activity.effective_at())?;
    let destination_converted =
        valuation_service::convert_amount(&snapshot, destination, activity.effective_at())?;
    let pending = ActivityFxConversionDto {
        status: "unavailable".to_owned(),
        base_currency: base_currency.to_owned(),
        source_currency: source.currency().as_str().to_owned(),
        destination_currency: destination.currency().as_str().to_owned(),
        transaction_rate: transaction_rate.canonical(),
        transaction_rate_inverse: inverse.canonical(),
        market_quote_id: None,
        market_base_currency: None,
        market_quote_currency: None,
        market_rate: None,
        source_base: None,
        destination_base: None,
        spread_amount: None,
        spread_currency: None,
        spread_effect: None,
    };
    if !source_converted.complete || !destination_converted.complete {
        return Ok(Some(pending));
    }
    let source_base = round_base(
        source_converted.base.expect("complete source"),
        base_currency,
    )?;
    let destination_base = round_base(
        destination_converted.base.expect("complete dest"),
        base_currency,
    )?;
    let spread = round_to_money_scale(checked_sub(
        destination_base.amount(),
        source_base.amount(),
    )?)?;
    let (spread_amount, spread_effect) = if spread.is_zero() {
        ("0".to_owned(), "none")
    } else {
        (
            Money::from_canonical(spread.abs(), CurrencyCode::parse(base_currency)?)?
                .canonical_amount(),
            if spread.is_sign_negative() {
                "loss"
            } else {
                "gain"
            },
        )
    };
    let market_id =
        valuation_service::fx_quote_id(&snapshot, destination.currency(), activity.effective_at())?
            .or(valuation_service::fx_quote_id(
                &snapshot,
                source.currency(),
                activity.effective_at(),
            )?);
    let market = market_id
        .as_ref()
        .and_then(|id| quotes.iter().find(|quote| quote.id == *id));
    Ok(Some(ActivityFxConversionDto {
        status: "computed".to_owned(),
        source_base: Some(source_base.canonical_amount()),
        destination_base: Some(destination_base.canonical_amount()),
        spread_amount: Some(spread_amount),
        spread_currency: Some(base_currency.to_owned()),
        spread_effect: Some(spread_effect.to_owned()),
        market_quote_id: market.map(|quote| quote.id.clone()),
        market_base_currency: market.map(|quote| quote.base_currency.clone()),
        market_quote_currency: market.map(|quote| quote.quote_currency.clone()),
        market_rate: market.map(|quote| quote.rate.clone()),
        ..pending
    }))
}

fn transfer_native_amounts(activity: &Activity) -> Option<(Money, Money)> {
    let source = activity
        .legs()
        .iter()
        .find(|leg| leg.role() == LegRole::Source)?;
    let destination = activity
        .legs()
        .iter()
        .find(|leg| leg.role() == LegRole::Destination)?;
    Some((
        source.component().money().ok()?,
        destination.component().money().ok()?,
    ))
}

async fn fx_snapshot_at(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    base_currency: &str,
    cutoff: &Timestamp,
) -> Result<(ValuationSnapshot, Vec<FxQuoteRecordDto>), AppError> {
    let cutoff_text = cutoff.to_rfc3339();
    let quotes = quote_service::list_latest_fx_quotes_at(tx, household_id, &cutoff_text).await?;
    let mut fx_quotes = HashMap::new();
    for quote in &quotes {
        fx_quotes.insert(
            (
                quote.base_currency.clone(),
                quote.quote_currency.clone(),
                quote.source_kind.clone(),
            ),
            quote.clone(),
        );
    }
    let mut fx_preferences = HashMap::new();
    for preference in list_latest_fx_preferences_at(tx, household_id, &cutoff_text).await? {
        fx_preferences.insert(
            FxPair::new(
                CurrencyCode::parse(&preference.currency_a)?,
                CurrencyCode::parse(&preference.currency_b)?,
            )?,
            QuoteSourceKind::parse(&preference.source_kind)?,
        );
    }
    Ok((
        ValuationSnapshot::from_parts(
            household_id.to_owned(),
            CurrencyCode::parse(base_currency)?,
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            HashMap::new(),
            fx_quotes,
            fx_preferences,
        ),
        quotes,
    ))
}

fn round_base(amount: Money, base_currency: &str) -> Result<Money, AppError> {
    Money::from_canonical(amount.amount(), CurrencyCode::parse(base_currency)?)
}
