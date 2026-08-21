use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};
use std::str::FromStr;

use super::{
    activity_service,
    history_repositories::{
        insert_fx_preference_observation, insert_instrument_preference_observation,
        FxPreferenceObservationRecord, InstrumentPreferenceObservationRecord,
    },
    instrument_service, query_count,
    reference::{
        begin_write_tx, finish_write_tx, map_read_error, map_write_error, require_household_id_tx,
        require_household_tx,
    },
};
use crate::{
    domain::{
        canonical_decimal, CurrencyCode, FxPair, FxQuote, FxQuoteId, FxRate, HouseholdId,
        InstrumentId, InstrumentQuote, InstrumentQuoteId, QuotePreferenceObservationId,
        QuoteSourceKind, Timestamp, UnitPrice,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListInstrumentQuotesInput {
    pub instrument_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppendManualInstrumentQuoteInput {
    pub instrument_id: String,
    pub unit_price: String,
    pub quoted_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetInstrumentQuotePreferenceInput {
    pub instrument_id: String,
    pub quote_preference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentQuoteRecordDto {
    pub id: String,
    pub instrument_id: String,
    pub unit_price: String,
    pub quote_currency: String,
    pub source_kind: String,
    pub source_key: String,
    pub delayed: bool,
    pub quoted_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListFxQuotesInput {
    pub base_currency: String,
    pub quote_currency: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppendManualFxQuoteInput {
    pub base_currency: String,
    pub quote_currency: String,
    pub rate: String,
    pub quoted_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetFxQuotePreferenceInput {
    pub currency_a: String,
    pub currency_b: String,
    pub quote_preference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FxQuoteRecordDto {
    pub id: String,
    pub base_currency: String,
    pub quote_currency: String,
    pub rate: String,
    pub source_kind: String,
    pub source_key: String,
    pub delayed: bool,
    pub quoted_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FxPairStatusDto {
    pub currency_a: String,
    pub currency_b: String,
    pub quote_preference: String,
    pub selected_quote: Option<FxQuoteRecordDto>,
    pub selected_rate: Option<String>,
}

pub async fn list_instrument_quotes(
    state: &AppState,
    input: ListInstrumentQuotesInput,
) -> Result<Vec<InstrumentQuoteRecordDto>, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household_id = require_household_id_tx(&mut tx).await?;
        instrument_service::load_instrument(&mut tx, &household_id, &input.instrument_id).await?;
        list_instrument_quotes_in_tx(&mut tx, &input.instrument_id).await
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn append_manual_instrument_quote(
    state: &AppState,
    input: AppendManualInstrumentQuoteInput,
) -> Result<InstrumentQuoteRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = append_manual_instrument_quote_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn set_instrument_quote_preference(
    state: &AppState,
    input: SetInstrumentQuotePreferenceInput,
) -> Result<instrument_service::InstrumentRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = set_instrument_quote_preference_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn list_fx_quotes(
    state: &AppState,
    input: ListFxQuotesInput,
) -> Result<Vec<FxQuoteRecordDto>, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        list_fx_quotes_in_tx(
            &mut tx,
            &household.id,
            &input.base_currency,
            &input.quote_currency,
        )
        .await
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn append_manual_fx_quote(
    state: &AppState,
    input: AppendManualFxQuoteInput,
) -> Result<FxQuoteRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = append_manual_fx_quote_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn set_fx_quote_preference(
    state: &AppState,
    input: SetFxQuotePreferenceInput,
) -> Result<FxPairStatusDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = set_fx_quote_preference_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

async fn append_manual_instrument_quote_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: AppendManualInstrumentQuoteInput,
) -> Result<InstrumentQuoteRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let mut instrument =
        instrument_service::load_instrument_domain(tx, &household_id, &input.instrument_id).await?;
    let now = Timestamp::now();
    let quoted_at = input
        .quoted_at
        .as_deref()
        .map(Timestamp::parse)
        .transpose()?
        .unwrap_or_else(|| now.clone());
    let quote = InstrumentQuote::new(
        InstrumentId::parse(&input.instrument_id)?,
        UnitPrice::parse(&input.unit_price)?,
        instrument.quote_currency(),
        QuoteSourceKind::Manual,
        "manual",
        false,
        quoted_at,
        now.clone(),
    )?;
    let previous_preference = instrument.quote_preference();
    insert_instrument_quote(tx, &quote).await?;
    instrument.set_quote_preference(QuoteSourceKind::Manual, now.clone());
    sqlx::query(
        "UPDATE instruments SET quote_preference = ?, updated_at = ? WHERE id = ? AND household_id = ?",
    )
    .bind(instrument.quote_preference().as_str())
    .bind(instrument.updated_at().to_rfc3339())
    .bind(instrument.id().to_string())
    .bind(&household_id)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("instrument.preference_failed", error))?;
    if previous_preference != QuoteSourceKind::Manual {
        append_instrument_preference_observation(
            tx,
            &instrument.id().to_string(),
            QuoteSourceKind::Manual.as_str(),
            &now,
        )
        .await?;
    }
    activity_service::mark_dirty_for_household(tx, &household_id, quote.quoted_at()).await?;
    Ok(instrument_quote_dto(&quote))
}

pub(crate) async fn append_imported_manual_instrument_quote_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    instrument_id: &str,
    unit_price: &str,
    quoted_at: Option<&str>,
) -> Result<InstrumentQuoteRecordDto, AppError> {
    let instrument =
        instrument_service::load_instrument_domain(tx, household_id, instrument_id).await?;
    if instrument.archived_at().is_some() {
        return Err(AppError::import_commit_failed(
            "The referenced instrument is archived.",
        ));
    }
    let now = Timestamp::now();
    let quoted_at = quoted_at
        .map(Timestamp::parse)
        .transpose()?
        .unwrap_or_else(|| now.clone());
    let quote = InstrumentQuote::new(
        InstrumentId::parse(instrument_id)?,
        UnitPrice::parse(unit_price)?,
        instrument.quote_currency(),
        QuoteSourceKind::Manual,
        "manual",
        false,
        quoted_at,
        now,
    )?;
    insert_instrument_quote(tx, &quote).await?;
    activity_service::mark_dirty_for_household(tx, household_id, quote.quoted_at()).await?;
    Ok(instrument_quote_dto(&quote))
}

async fn set_instrument_quote_preference_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: SetInstrumentQuotePreferenceInput,
) -> Result<instrument_service::InstrumentRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let mut instrument =
        instrument_service::load_instrument_domain(tx, &household_id, &input.instrument_id).await?;
    let preference = QuoteSourceKind::parse(&input.quote_preference)?;
    if instrument.quote_preference() == preference {
        return instrument_service::load_instrument(
            tx,
            &household_id,
            &instrument.id().to_string(),
        )
        .await;
    }
    let now = Timestamp::now();
    instrument.set_quote_preference(preference, now.clone());
    sqlx::query(
        "UPDATE instruments SET quote_preference = ?, updated_at = ? WHERE id = ? AND household_id = ?",
    )
    .bind(instrument.quote_preference().as_str())
    .bind(instrument.updated_at().to_rfc3339())
    .bind(instrument.id().to_string())
    .bind(&household_id)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("instrument.preference_failed", error))?;
    append_instrument_preference_observation(
        tx,
        &instrument.id().to_string(),
        preference.as_str(),
        &now,
    )
    .await?;
    activity_service::mark_dirty_for_household(tx, &household_id, &now).await?;
    instrument_service::load_instrument(tx, &household_id, &instrument.id().to_string()).await
}

async fn append_manual_fx_quote_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: AppendManualFxQuoteInput,
) -> Result<FxQuoteRecordDto, AppError> {
    let household = require_household_tx(tx).await?;
    let now = Timestamp::now();
    let quoted_at = input
        .quoted_at
        .as_deref()
        .map(Timestamp::parse)
        .transpose()?
        .unwrap_or_else(|| now.clone());
    let quote = FxQuote::new(
        HouseholdId::parse(&household.id).map_err(|_| AppError::Internal)?,
        CurrencyCode::parse_supported(&input.base_currency)?,
        CurrencyCode::parse_supported(&input.quote_currency)?,
        FxRate::parse(&input.rate)?,
        QuoteSourceKind::Manual,
        "manual",
        false,
        quoted_at,
        now.clone(),
    )?;
    insert_fx_quote(tx, &quote).await?;
    let pair = FxPair::new(quote.base_currency(), quote.quote_currency())?;
    let previous = current_fx_preference(tx, &household.id, pair).await?;
    upsert_fx_preference(tx, &household.id, pair, QuoteSourceKind::Manual, &now).await?;
    if previous != Some(QuoteSourceKind::Manual) {
        append_fx_preference_observation(tx, &household.id, pair, QuoteSourceKind::Manual, &now)
            .await?;
    }
    activity_service::mark_dirty_for_household(tx, &household.id, quote.quoted_at()).await?;
    Ok(fx_quote_dto(&quote))
}

pub(crate) async fn append_imported_manual_fx_quote_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    base_currency: &str,
    quote_currency: &str,
    rate: &str,
    quoted_at: Option<&str>,
) -> Result<FxQuoteRecordDto, AppError> {
    let now = Timestamp::now();
    let quoted_at = quoted_at
        .map(Timestamp::parse)
        .transpose()?
        .unwrap_or_else(|| now.clone());
    let quote = FxQuote::new(
        HouseholdId::parse(household_id).map_err(|_| AppError::Internal)?,
        CurrencyCode::parse_supported(base_currency)?,
        CurrencyCode::parse_supported(quote_currency)?,
        FxRate::parse(rate)?,
        QuoteSourceKind::Manual,
        "manual",
        false,
        quoted_at,
        now,
    )?;
    insert_fx_quote(tx, &quote).await?;
    activity_service::mark_dirty_for_household(tx, household_id, quote.quoted_at()).await?;
    Ok(fx_quote_dto(&quote))
}

async fn set_fx_quote_preference_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: SetFxQuotePreferenceInput,
) -> Result<FxPairStatusDto, AppError> {
    let household = require_household_tx(tx).await?;
    let pair = FxPair::new(
        CurrencyCode::parse(&input.currency_a)?,
        CurrencyCode::parse(&input.currency_b)?,
    )?;
    let preference = QuoteSourceKind::parse(&input.quote_preference)?;
    let previous = current_fx_preference(tx, &household.id, pair).await?;
    let now = Timestamp::now();
    upsert_fx_preference(tx, &household.id, pair, preference, &now).await?;
    if previous != Some(preference) {
        append_fx_preference_observation(tx, &household.id, pair, preference, &now).await?;
        activity_service::mark_dirty_for_household(tx, &household.id, &now).await?;
    }
    load_fx_pair_status(tx, &household.id, pair).await
}

pub(crate) async fn insert_instrument_quote(
    tx: &mut Transaction<'_, Sqlite>,
    quote: &InstrumentQuote,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO instrument_quotes
         (id, instrument_id, unit_price, quote_currency, source_kind, source_key, delayed, quoted_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(quote.id().to_string())
    .bind(quote.instrument_id().to_string())
    .bind(quote.unit_price().canonical())
    .bind(quote.quote_currency().as_str())
    .bind(quote.source_kind().as_str())
    .bind(quote.source_key())
    .bind(i64::from(quote.delayed()))
    .bind(quote.quoted_at().to_rfc3339())
    .bind(quote.created_at().to_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("instrument_quote.insert_failed", error))?;
    Ok(())
}

pub(crate) async fn insert_fx_quote(
    tx: &mut Transaction<'_, Sqlite>,
    quote: &FxQuote,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO fx_quotes
         (id, household_id, base_currency, quote_currency, rate, source_kind, source_key, delayed, quoted_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(quote.id().to_string())
    .bind(quote.household_id().to_string())
    .bind(quote.base_currency().as_str())
    .bind(quote.quote_currency().as_str())
    .bind(quote.rate().canonical())
    .bind(quote.source_kind().as_str())
    .bind(quote.source_key())
    .bind(i64::from(quote.delayed()))
    .bind(quote.quoted_at().to_rfc3339())
    .bind(quote.created_at().to_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("fx_quote.insert_failed", error))?;
    Ok(())
}

pub(crate) async fn upsert_fx_preference(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    pair: FxPair,
    source_kind: QuoteSourceKind,
    now: &Timestamp,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO fx_quote_preferences (household_id, currency_a, currency_b, source_kind, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(household_id, currency_a, currency_b)
         DO UPDATE SET source_kind = excluded.source_kind, updated_at = excluded.updated_at",
    )
    .bind(household_id)
    .bind(pair.currency_a().as_str())
    .bind(pair.currency_b().as_str())
    .bind(source_kind.as_str())
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("fx.preference_failed", error))?;
    Ok(())
}

pub(crate) async fn current_fx_preference(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    pair: FxPair,
) -> Result<Option<QuoteSourceKind>, AppError> {
    let source: Option<String> = sqlx::query_scalar(
        "SELECT source_kind FROM fx_quote_preferences WHERE household_id = ? AND currency_a = ? AND currency_b = ?",
    )
    .bind(household_id)
    .bind(pair.currency_a().as_str())
    .bind(pair.currency_b().as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("fx.preference_load_failed", error))?;
    source
        .map(|value| QuoteSourceKind::parse(&value))
        .transpose()
}

async fn append_instrument_preference_observation(
    tx: &mut Transaction<'_, Sqlite>,
    instrument_id: &str,
    quote_preference: &str,
    at: &Timestamp,
) -> Result<(), AppError> {
    insert_instrument_preference_observation(
        tx,
        &InstrumentPreferenceObservationRecord {
            id: QuotePreferenceObservationId::new().to_string(),
            instrument_id: instrument_id.to_owned(),
            quote_preference: quote_preference.to_owned(),
            effective_at: at.to_rfc3339(),
            created_at: at.to_rfc3339(),
        },
    )
    .await
}

async fn append_fx_preference_observation(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    pair: FxPair,
    source_kind: QuoteSourceKind,
    at: &Timestamp,
) -> Result<(), AppError> {
    insert_fx_preference_observation(
        tx,
        &FxPreferenceObservationRecord {
            id: QuotePreferenceObservationId::new().to_string(),
            household_id: household_id.to_owned(),
            currency_a: pair.currency_a().as_str().to_owned(),
            currency_b: pair.currency_b().as_str().to_owned(),
            source_kind: source_kind.as_str().to_owned(),
            effective_at: at.to_rfc3339(),
            created_at: at.to_rfc3339(),
        },
    )
    .await
}

async fn list_instrument_quotes_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    instrument_id: &str,
) -> Result<Vec<InstrumentQuoteRecordDto>, AppError> {
    sqlx::query(
        "SELECT id, instrument_id, unit_price, quote_currency, source_kind, source_key, delayed, quoted_at, created_at
         FROM instrument_quotes
         WHERE instrument_id = ?
         ORDER BY quoted_at DESC, created_at DESC, id DESC",
    )
    .bind(instrument_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("instrument_quote.list_failed", error))?
    .into_iter()
    .map(instrument_quote_from_row)
    .collect()
}

async fn list_fx_quotes_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    base_currency: &str,
    quote_currency: &str,
) -> Result<Vec<FxQuoteRecordDto>, AppError> {
    sqlx::query(
        "SELECT id, household_id, base_currency, quote_currency, rate, source_kind, source_key, delayed, quoted_at, created_at
         FROM fx_quotes
         WHERE household_id = ? AND base_currency = ? AND quote_currency = ?
         ORDER BY quoted_at DESC, created_at DESC, id DESC",
    )
    .bind(household_id)
    .bind(base_currency)
    .bind(quote_currency)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("fx_quote.list_failed", error))?
    .into_iter()
    .map(fx_quote_from_row)
    .collect()
}

pub(crate) async fn list_all_instrument_quotes(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Vec<InstrumentQuoteRecordDto>, AppError> {
    query_count::record("instrument_quotes");
    sqlx::query(
        "SELECT q.id, q.instrument_id, q.unit_price, q.quote_currency, q.source_kind, q.source_key, q.delayed, q.quoted_at, q.created_at
         FROM instrument_quotes q
         JOIN instruments i ON i.id = q.instrument_id
         WHERE i.household_id = ?
         ORDER BY q.quoted_at DESC, q.created_at DESC, q.id DESC",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("instrument_quote.household_list_failed", error))?
    .into_iter()
    .map(instrument_quote_from_row)
    .collect()
}

pub(crate) async fn list_latest_instrument_quotes(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Vec<InstrumentQuoteRecordDto>, AppError> {
    sqlx::query(
        "SELECT q.id, q.instrument_id, q.unit_price, q.quote_currency, q.source_kind, q.source_key, q.delayed, q.quoted_at, q.created_at
         FROM (
           SELECT id, instrument_id, unit_price, quote_currency, source_kind, source_key, delayed, quoted_at, created_at,
                  ROW_NUMBER() OVER (PARTITION BY instrument_id, source_kind ORDER BY quoted_at DESC, created_at DESC, id DESC) AS rn
           FROM instrument_quotes
         ) q
         JOIN instruments i ON i.id = q.instrument_id
         WHERE i.household_id = ? AND q.rn = 1",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("instrument_quote.latest_failed", error))?
    .into_iter()
    .map(instrument_quote_from_row)
    .collect()
}

pub(crate) async fn list_all_fx_quotes(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Vec<FxQuoteRecordDto>, AppError> {
    query_count::record("fx_quotes");
    sqlx::query(
        "SELECT id, household_id, base_currency, quote_currency, rate, source_kind, source_key, delayed, quoted_at, created_at
         FROM fx_quotes
         WHERE household_id = ?
         ORDER BY quoted_at DESC, created_at DESC, id DESC",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("fx_quote.household_list_failed", error))?
    .into_iter()
    .map(fx_quote_from_row)
    .collect()
}

pub(crate) async fn list_fx_quotes_for_pair_at(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    native: &str,
    household_base: &str,
    cutoff_at: &str,
) -> Result<Vec<FxQuoteRecordDto>, AppError> {
    query_count::record("benchmark.fx_quotes");
    sqlx::query(
        "SELECT id, household_id, base_currency, quote_currency, rate, source_kind, source_key, delayed, quoted_at, created_at
         FROM fx_quotes
         WHERE household_id = ?
           AND quoted_at <= ?
           AND (
                (base_currency = ? AND quote_currency = ?)
                OR (base_currency = ? AND quote_currency = ?)
           )
         ORDER BY quoted_at DESC, created_at DESC, id DESC",
    )
    .bind(household_id)
    .bind(cutoff_at)
    .bind(native)
    .bind(household_base)
    .bind(household_base)
    .bind(native)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("fx_quote.pair_cutoff_list_failed", error))?
    .into_iter()
    .map(fx_quote_from_row)
    .collect()
}

pub(crate) async fn list_latest_fx_quotes(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Vec<FxQuoteRecordDto>, AppError> {
    sqlx::query(
        "SELECT id, household_id, base_currency, quote_currency, rate, source_kind, source_key, delayed, quoted_at, created_at
         FROM (
           SELECT id, household_id, base_currency, quote_currency, rate, source_kind, source_key, delayed, quoted_at, created_at,
                  ROW_NUMBER() OVER (PARTITION BY household_id, base_currency, quote_currency, source_kind ORDER BY quoted_at DESC, created_at DESC, id DESC) AS rn
           FROM fx_quotes
           WHERE household_id = ?
         ) ranked
         WHERE rn = 1",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("fx_quote.latest_failed", error))?
    .into_iter()
    .map(fx_quote_from_row)
    .collect()
}

pub(crate) async fn list_latest_instrument_quotes_at(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    cutoff_at: &str,
) -> Result<Vec<InstrumentQuoteRecordDto>, AppError> {
    sqlx::query(
        "SELECT q.id, q.instrument_id, q.unit_price, q.quote_currency, q.source_kind, q.source_key, q.delayed, q.quoted_at, q.created_at
         FROM (
           SELECT id, instrument_id, unit_price, quote_currency, source_kind, source_key, delayed, quoted_at, created_at,
                  ROW_NUMBER() OVER (PARTITION BY instrument_id, source_kind ORDER BY quoted_at DESC, created_at DESC, id DESC) AS rn
           FROM instrument_quotes
           WHERE quoted_at <= ?
         ) q
         JOIN instruments i ON i.id = q.instrument_id
         WHERE i.household_id = ? AND q.rn = 1",
    )
    .bind(cutoff_at)
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("instrument_quote.historical_failed", error))?
    .into_iter()
    .map(instrument_quote_from_row)
    .collect()
}

pub(crate) async fn list_latest_fx_quotes_at(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    cutoff_at: &str,
) -> Result<Vec<FxQuoteRecordDto>, AppError> {
    sqlx::query(
        "SELECT id, household_id, base_currency, quote_currency, rate, source_kind, source_key, delayed, quoted_at, created_at
         FROM (
           SELECT id, household_id, base_currency, quote_currency, rate, source_kind, source_key, delayed, quoted_at, created_at,
                  ROW_NUMBER() OVER (PARTITION BY household_id, base_currency, quote_currency, source_kind ORDER BY quoted_at DESC, created_at DESC, id DESC) AS rn
           FROM fx_quotes
           WHERE household_id = ? AND quoted_at <= ?
         ) ranked
         WHERE rn = 1",
    )
    .bind(household_id)
    .bind(cutoff_at)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("fx_quote.historical_failed", error))?
    .into_iter()
    .map(fx_quote_from_row)
    .collect()
}

#[cfg(test)]
pub(crate) async fn list_instrument_quotes_at(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    cutoff_at: &str,
) -> Result<Vec<InstrumentQuoteRecordDto>, AppError> {
    sqlx::query(
        "SELECT q.id, q.instrument_id, q.unit_price, q.quote_currency, q.source_kind, q.source_key, q.delayed, q.quoted_at, q.created_at
         FROM instrument_quotes q
         JOIN instruments i ON i.id = q.instrument_id
         WHERE i.household_id = ? AND q.quoted_at <= ?
         ORDER BY q.quoted_at DESC, q.created_at DESC, q.id DESC",
    )
    .bind(household_id)
    .bind(cutoff_at)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("instrument_quote.cutoff_list_failed", error))?
    .into_iter()
    .map(instrument_quote_from_row)
    .collect()
}

#[cfg(test)]
pub(crate) async fn list_fx_quotes_at(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    cutoff_at: &str,
) -> Result<Vec<FxQuoteRecordDto>, AppError> {
    sqlx::query(
        "SELECT id, household_id, base_currency, quote_currency, rate, source_kind, source_key, delayed, quoted_at, created_at
         FROM fx_quotes
         WHERE household_id = ? AND quoted_at <= ?
         ORDER BY quoted_at DESC, created_at DESC, id DESC",
    )
    .bind(household_id)
    .bind(cutoff_at)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("fx_quote.cutoff_list_failed", error))?
    .into_iter()
    .map(fx_quote_from_row)
    .collect()
}

pub(crate) async fn list_fx_preferences(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Vec<(FxPair, QuoteSourceKind)>, AppError> {
    let rows = sqlx::query(
        "SELECT currency_a, currency_b, source_kind FROM fx_quote_preferences WHERE household_id = ?",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("fx.preference_list_failed", error))?;
    let mut preferences = Vec::new();
    for row in rows {
        let left: String = row
            .try_get("currency_a")
            .map_err(|_| AppError::DatabaseUnavailable)?;
        let right: String = row
            .try_get("currency_b")
            .map_err(|_| AppError::DatabaseUnavailable)?;
        let source: String = row
            .try_get("source_kind")
            .map_err(|_| AppError::DatabaseUnavailable)?;
        preferences.push((
            FxPair::new(CurrencyCode::parse(&left)?, CurrencyCode::parse(&right)?)?,
            QuoteSourceKind::parse(&source)?,
        ));
    }
    Ok(preferences)
}

async fn load_fx_pair_status(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    pair: FxPair,
) -> Result<FxPairStatusDto, AppError> {
    let source: Option<String> = sqlx::query_scalar(
        "SELECT source_kind FROM fx_quote_preferences WHERE household_id = ? AND currency_a = ? AND currency_b = ?",
    )
    .bind(household_id)
    .bind(pair.currency_a().as_str())
    .bind(pair.currency_b().as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("fx.preference_load_failed", error))?;
    let quote_preference = source.unwrap_or_else(|| QuoteSourceKind::Manual.as_str().to_owned());
    let quotes = list_latest_fx_quotes(tx, household_id).await?;
    let selected = quotes.into_iter().find(|quote| {
        quote.source_kind == quote_preference
            && ((quote.base_currency == pair.currency_a().as_str()
                && quote.quote_currency == pair.currency_b().as_str())
                || (quote.base_currency == pair.currency_b().as_str()
                    && quote.quote_currency == pair.currency_a().as_str()))
    });
    let selected_rate = selected_rate_for_pair(pair, selected.as_ref())?;
    Ok(FxPairStatusDto {
        currency_a: pair.currency_a().as_str().to_owned(),
        currency_b: pair.currency_b().as_str().to_owned(),
        quote_preference,
        selected_quote: selected,
        selected_rate,
    })
}

pub(crate) fn selected_rate_for_pair(
    pair: FxPair,
    selected: Option<&FxQuoteRecordDto>,
) -> Result<Option<String>, AppError> {
    let Some(selected) = selected else {
        return Ok(None);
    };
    if selected.base_currency == pair.currency_b().as_str()
        && selected.quote_currency == pair.currency_a().as_str()
    {
        return Ok(Some(selected.rate.clone()));
    }
    if selected.base_currency == pair.currency_a().as_str()
        && selected.quote_currency == pair.currency_b().as_str()
    {
        let rate = rust_decimal::Decimal::from_str(&selected.rate)
            .map_err(|_| AppError::DatabaseUnavailable)?;
        let inverted = rust_decimal::Decimal::ONE
            .checked_div(rate)
            .ok_or(AppError::DecimalOverflow)?;
        return Ok(Some(canonical_decimal(inverted)));
    }
    Ok(None)
}

fn instrument_quote_dto(quote: &InstrumentQuote) -> InstrumentQuoteRecordDto {
    InstrumentQuoteRecordDto {
        id: quote.id().to_string(),
        instrument_id: quote.instrument_id().to_string(),
        unit_price: quote.unit_price().canonical(),
        quote_currency: quote.quote_currency().as_str().to_owned(),
        source_kind: quote.source_kind().as_str().to_owned(),
        source_key: quote.source_key().to_owned(),
        delayed: quote.delayed(),
        quoted_at: quote.quoted_at().to_rfc3339(),
        created_at: quote.created_at().to_rfc3339(),
    }
}

fn fx_quote_dto(quote: &FxQuote) -> FxQuoteRecordDto {
    FxQuoteRecordDto {
        id: quote.id().to_string(),
        base_currency: quote.base_currency().as_str().to_owned(),
        quote_currency: quote.quote_currency().as_str().to_owned(),
        rate: quote.rate().canonical(),
        source_kind: quote.source_kind().as_str().to_owned(),
        source_key: quote.source_key().to_owned(),
        delayed: quote.delayed(),
        quoted_at: quote.quoted_at().to_rfc3339(),
        created_at: quote.created_at().to_rfc3339(),
    }
}

fn instrument_quote_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<InstrumentQuoteRecordDto, AppError> {
    Ok(InstrumentQuoteRecordDto {
        id: row
            .try_get("id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        instrument_id: row
            .try_get("instrument_id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        unit_price: row
            .try_get("unit_price")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        quote_currency: row
            .try_get("quote_currency")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        source_kind: row
            .try_get("source_kind")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        source_key: row
            .try_get("source_key")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        delayed: row
            .try_get::<i64, _>("delayed")
            .map_err(|_| AppError::DatabaseUnavailable)?
            != 0,
        quoted_at: row
            .try_get("quoted_at")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        created_at: row
            .try_get("created_at")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    })
}

fn fx_quote_from_row(row: sqlx::sqlite::SqliteRow) -> Result<FxQuoteRecordDto, AppError> {
    Ok(FxQuoteRecordDto {
        id: row
            .try_get("id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        base_currency: row
            .try_get("base_currency")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        quote_currency: row
            .try_get("quote_currency")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        rate: row
            .try_get("rate")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        source_kind: row
            .try_get("source_kind")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        source_key: row
            .try_get("source_key")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        delayed: row
            .try_get::<i64, _>("delayed")
            .map_err(|_| AppError::DatabaseUnavailable)?
            != 0,
        quoted_at: row
            .try_get("quoted_at")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        created_at: row
            .try_get("created_at")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    })
}

pub fn parse_instrument_quote(dto: &InstrumentQuoteRecordDto) -> Result<InstrumentQuote, AppError> {
    Ok(InstrumentQuote::from_persisted(
        InstrumentQuoteId::parse(&dto.id)?,
        InstrumentId::parse(&dto.instrument_id)?,
        UnitPrice::parse(&dto.unit_price)?,
        CurrencyCode::parse(&dto.quote_currency)?,
        QuoteSourceKind::parse(&dto.source_kind)?,
        dto.source_key.clone(),
        dto.delayed,
        Timestamp::parse(&dto.quoted_at)?,
        Timestamp::parse(&dto.created_at)?,
    ))
}

pub fn parse_fx_quote(
    household_id: HouseholdId,
    dto: &FxQuoteRecordDto,
) -> Result<FxQuote, AppError> {
    Ok(FxQuote::from_persisted(
        FxQuoteId::parse(&dto.id)?,
        household_id,
        CurrencyCode::parse(&dto.base_currency)?,
        CurrencyCode::parse(&dto.quote_currency)?,
        FxRate::parse(&dto.rate)?,
        QuoteSourceKind::parse(&dto.source_kind)?,
        dto.source_key.clone(),
        dto.delayed,
        Timestamp::parse(&dto.quoted_at)?,
        Timestamp::parse(&dto.created_at)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::{selected_rate_for_pair, FxQuoteRecordDto};
    use crate::domain::{CurrencyCode, FxPair};

    fn quote(base_currency: &str, quote_currency: &str, rate: &str) -> FxQuoteRecordDto {
        FxQuoteRecordDto {
            id: "fx-test".to_owned(),
            base_currency: base_currency.to_owned(),
            quote_currency: quote_currency.to_owned(),
            rate: rate.to_owned(),
            source_kind: "manual".to_owned(),
            source_key: "manual".to_owned(),
            delayed: false,
            quoted_at: "2026-01-01T00:00:00Z".to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn selected_rate_uses_persisted_pair_direction() {
        let pair = FxPair::new(CurrencyCode::CNY, CurrencyCode::SGD).expect("pair");
        assert_eq!(
            selected_rate_for_pair(pair, Some(&quote("SGD", "CNY", "5.3"))).expect("direct"),
            Some("5.3".to_owned())
        );
        assert_eq!(
            selected_rate_for_pair(pair, Some(&quote("CNY", "SGD", "0.2"))).expect("inverse"),
            Some("5".to_owned())
        );
    }
}
