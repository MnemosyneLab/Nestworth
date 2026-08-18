use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};

use super::reference::{
    begin_write_tx, finish_write_tx, map_read_error, map_unique_or_write, map_write_error,
    next_sort_order, require_household_id, require_household_id_tx, sort_order_i32, SortTable,
};
use crate::{
    domain::{
        CurrencyCode, HouseholdId, Instrument, InstrumentId, InstrumentType, MediaAssetId,
        NewInstrument, PersistedInstrument, QuoteSourceKind, Timestamp,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstrumentInput {
    pub name: String,
    pub symbol: Option<String>,
    pub instrument_type: String,
    pub quote_currency: String,
    pub market_code: Option<String>,
    pub country_code: Option<String>,
    pub isin: Option<String>,
    pub provider_key: Option<String>,
    pub provider_symbol: Option<String>,
    pub quote_preference: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstrumentInput {
    pub id: String,
    pub name: String,
    pub symbol: Option<String>,
    pub instrument_type: String,
    pub market_code: Option<String>,
    pub country_code: Option<String>,
    pub isin: Option<String>,
    pub provider_key: Option<String>,
    pub provider_symbol: Option<String>,
    pub quote_preference: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentRecordDto {
    pub id: String,
    pub name: String,
    pub symbol: Option<String>,
    pub instrument_type: String,
    pub quote_currency: String,
    pub market_code: Option<String>,
    pub country_code: Option<String>,
    pub isin: Option<String>,
    pub provider_key: Option<String>,
    pub provider_symbol: Option<String>,
    pub quote_preference: String,
    pub note: Option<String>,
    pub logo_asset_id: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

pub async fn list_instruments(
    state: &AppState,
    include_archived: bool,
) -> Result<Vec<InstrumentRecordDto>, AppError> {
    let database = state.writable_db()?;
    let household_id = require_household_id(database).await?;
    sqlx::query(list_sql(include_archived))
        .bind(&household_id)
        .fetch_all(database)
        .await
        .map_err(|error| map_read_error("instrument.list_failed", error))?
        .into_iter()
        .map(instrument_from_row)
        .collect()
}

pub(crate) async fn list_instruments_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    include_archived: bool,
) -> Result<Vec<InstrumentRecordDto>, AppError> {
    sqlx::query(list_sql(include_archived))
        .bind(household_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("instrument.list_failed", error))?
        .into_iter()
        .map(instrument_from_row)
        .collect()
}

fn list_sql(include_archived: bool) -> &'static str {
    if include_archived {
        "SELECT id, household_id, name, symbol, instrument_type, quote_currency, market_code, country_code, isin, provider_key, provider_symbol, quote_preference, note, logo_asset_id, sort_order, created_at, updated_at, archived_at
         FROM instruments
         WHERE household_id = ?
         ORDER BY sort_order ASC, name COLLATE NOCASE ASC, id ASC"
    } else {
        "SELECT id, household_id, name, symbol, instrument_type, quote_currency, market_code, country_code, isin, provider_key, provider_symbol, quote_preference, note, logo_asset_id, sort_order, created_at, updated_at, archived_at
         FROM instruments
         WHERE household_id = ? AND archived_at IS NULL
         ORDER BY sort_order ASC, name COLLATE NOCASE ASC, id ASC"
    }
}

pub async fn get_instrument(state: &AppState, id: &str) -> Result<InstrumentRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household_id = require_household_id_tx(&mut tx).await?;
        load_instrument(&mut tx, &household_id, id).await
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn create_instrument(
    state: &AppState,
    input: CreateInstrumentInput,
) -> Result<InstrumentRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = create_instrument_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn update_instrument(
    state: &AppState,
    input: UpdateInstrumentInput,
) -> Result<InstrumentRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = update_instrument_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn archive_instrument(
    state: &AppState,
    id: &str,
) -> Result<InstrumentRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = mutate_archive_in_tx(&mut tx, id, true).await;
    finish_write_tx(tx, result).await
}

pub async fn restore_instrument(
    state: &AppState,
    id: &str,
) -> Result<InstrumentRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = mutate_archive_in_tx(&mut tx, id, false).await;
    finish_write_tx(tx, result).await
}

async fn create_instrument_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: CreateInstrumentInput,
) -> Result<InstrumentRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let sort_order = next_sort_order(tx, SortTable::Instruments, &household_id).await?;
    let instrument = Instrument::new(
        new_instrument_from_create(&household_id, &input, sort_order)?,
        Timestamp::now(),
    )?;
    insert_instrument(tx, &household_id, &instrument).await?;
    tracing::info!(
        event = "instrument.create",
        instrument_id = %instrument.id(),
        "instrument created"
    );
    load_instrument(tx, &household_id, &instrument.id().to_string()).await
}

async fn update_instrument_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: UpdateInstrumentInput,
) -> Result<InstrumentRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let instrument_id = InstrumentId::parse(&input.id)?;
    let mut instrument =
        load_instrument_domain(tx, &household_id, &instrument_id.to_string()).await?;
    let update = NewInstrument {
        household_id: instrument.household_id(),
        name: input.name,
        symbol: input.symbol,
        instrument_type: InstrumentType::parse(&input.instrument_type)?,
        quote_currency: instrument.quote_currency(),
        market_code: input.market_code,
        country_code: input.country_code,
        isin: input.isin,
        provider_key: input.provider_key,
        provider_symbol: input.provider_symbol,
        quote_preference: input
            .quote_preference
            .as_deref()
            .map(QuoteSourceKind::parse)
            .transpose()?
            .unwrap_or_else(|| instrument.quote_preference()),
        note: input.note,
        logo_asset_id: instrument.logo_asset_id(),
        sort_order: instrument.sort_order(),
    };
    instrument.update(update, Timestamp::now())?;
    persist_instrument(tx, &household_id, &instrument).await?;
    load_instrument(tx, &household_id, &instrument.id().to_string()).await
}

async fn mutate_archive_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    archive: bool,
) -> Result<InstrumentRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let current = load_instrument(tx, &household_id, id).await?;
    if archive && current.archived_at.is_some() {
        return Ok(current);
    }
    if !archive && current.archived_at.is_none() {
        return Ok(current);
    }
    let mut instrument = instrument_from_dto(&household_id, current)?;
    if archive {
        instrument.archive(Timestamp::now());
        sqlx::query(
            "UPDATE instruments SET archived_at = ?, updated_at = ? WHERE id = ? AND household_id = ? AND archived_at IS NULL",
        )
        .bind(instrument.archived_at().map(Timestamp::to_rfc3339))
        .bind(instrument.updated_at().to_rfc3339())
        .bind(instrument.id().to_string())
        .bind(&household_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("instrument.archive_failed", error))?;
    } else {
        instrument.restore(Timestamp::now());
        sqlx::query(
            "UPDATE instruments SET archived_at = NULL, updated_at = ? WHERE id = ? AND household_id = ? AND archived_at IS NOT NULL",
        )
        .bind(instrument.updated_at().to_rfc3339())
        .bind(instrument.id().to_string())
        .bind(&household_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("instrument.restore_failed", error))?;
    }
    load_instrument(tx, &household_id, &instrument.id().to_string()).await
}

fn new_instrument_from_create(
    household_id: &str,
    input: &CreateInstrumentInput,
    sort_order: i64,
) -> Result<NewInstrument, AppError> {
    Ok(NewInstrument {
        household_id: HouseholdId::parse(household_id).map_err(|_| AppError::Internal)?,
        name: input.name.clone(),
        symbol: input.symbol.clone(),
        instrument_type: InstrumentType::parse(&input.instrument_type)?,
        quote_currency: CurrencyCode::parse(&input.quote_currency)?,
        market_code: input.market_code.clone(),
        country_code: input.country_code.clone(),
        isin: input.isin.clone(),
        provider_key: input.provider_key.clone(),
        provider_symbol: input.provider_symbol.clone(),
        quote_preference: input
            .quote_preference
            .as_deref()
            .map(QuoteSourceKind::parse)
            .transpose()?
            .unwrap_or(QuoteSourceKind::Manual),
        note: input.note.clone(),
        logo_asset_id: None,
        sort_order,
    })
}

async fn insert_instrument(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    instrument: &Instrument,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO instruments
         (id, household_id, name, symbol, instrument_type, quote_currency, market_code, country_code, isin, provider_key, provider_symbol, quote_preference, note, logo_asset_id, sort_order, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(instrument.id().to_string())
    .bind(household_id)
    .bind(instrument.name())
    .bind(instrument.symbol())
    .bind(instrument.instrument_type().as_str())
    .bind(instrument.quote_currency().as_str())
    .bind(instrument.market_code())
    .bind(instrument.country_code())
    .bind(instrument.isin())
    .bind(instrument.provider_key())
    .bind(instrument.provider_symbol())
    .bind(instrument.quote_preference().as_str())
    .bind(instrument.note())
    .bind(instrument.logo_asset_id().map(|id| id.to_string()))
    .bind(instrument.sort_order())
    .bind(instrument.created_at().to_rfc3339())
    .bind(instrument.updated_at().to_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(|error| {
        map_unique_or_write(
            "instrument.create_failed",
            error,
            AppError::conflict("This provider identity is already used in the household."),
        )
    })?;
    Ok(())
}

async fn persist_instrument(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    instrument: &Instrument,
) -> Result<(), AppError> {
    let updated = sqlx::query(
        "UPDATE instruments
         SET name = ?, symbol = ?, instrument_type = ?, market_code = ?, country_code = ?, isin = ?, provider_key = ?, provider_symbol = ?, quote_preference = ?, note = ?, updated_at = ?
         WHERE id = ? AND household_id = ?",
    )
    .bind(instrument.name())
    .bind(instrument.symbol())
    .bind(instrument.instrument_type().as_str())
    .bind(instrument.market_code())
    .bind(instrument.country_code())
    .bind(instrument.isin())
    .bind(instrument.provider_key())
    .bind(instrument.provider_symbol())
    .bind(instrument.quote_preference().as_str())
    .bind(instrument.note())
    .bind(instrument.updated_at().to_rfc3339())
    .bind(instrument.id().to_string())
    .bind(household_id)
    .execute(&mut **tx)
    .await
    .map_err(|error| {
        map_unique_or_write(
            "instrument.update_failed",
            error,
            AppError::conflict("This provider identity is already used in the household."),
        )
    })?;
    if updated.rows_affected() != 1 {
        return Err(AppError::not_found(
            "instrument",
            &instrument.id().to_string(),
        ));
    }
    Ok(())
}

pub(crate) async fn load_instrument(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    id: &str,
) -> Result<InstrumentRecordDto, AppError> {
    let row = sqlx::query(
        "SELECT id, household_id, name, symbol, instrument_type, quote_currency, market_code, country_code, isin, provider_key, provider_symbol, quote_preference, note, logo_asset_id, sort_order, created_at, updated_at, archived_at
         FROM instruments WHERE id = ? AND household_id = ?",
    )
    .bind(id)
    .bind(household_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("instrument.load_failed", error))?
    .ok_or_else(|| AppError::not_found("instrument", id))?;
    instrument_from_row(row)
}

pub(crate) async fn load_instrument_domain(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    id: &str,
) -> Result<Instrument, AppError> {
    instrument_from_dto(household_id, load_instrument(tx, household_id, id).await?)
}

pub(crate) fn instrument_from_dto(
    household_id: &str,
    dto: InstrumentRecordDto,
) -> Result<Instrument, AppError> {
    Ok(Instrument::from_persisted(PersistedInstrument {
        id: InstrumentId::parse(&dto.id)?,
        household_id: HouseholdId::parse(household_id).map_err(|_| AppError::Internal)?,
        name: dto.name,
        symbol: dto.symbol,
        instrument_type: InstrumentType::parse(&dto.instrument_type)?,
        quote_currency: CurrencyCode::parse(&dto.quote_currency)?,
        market_code: dto.market_code,
        country_code: dto.country_code,
        isin: dto.isin,
        provider_key: dto.provider_key,
        provider_symbol: dto.provider_symbol,
        quote_preference: QuoteSourceKind::parse(&dto.quote_preference)?,
        note: dto.note,
        logo_asset_id: dto
            .logo_asset_id
            .as_deref()
            .map(MediaAssetId::parse)
            .transpose()?,
        sort_order: i64::from(dto.sort_order),
        created_at: Timestamp::parse(&dto.created_at)?,
        updated_at: Timestamp::parse(&dto.updated_at)?,
        archived_at: dto
            .archived_at
            .as_deref()
            .map(Timestamp::parse)
            .transpose()?,
    }))
}

fn instrument_from_row(row: sqlx::sqlite::SqliteRow) -> Result<InstrumentRecordDto, AppError> {
    Ok(InstrumentRecordDto {
        id: row
            .try_get("id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        name: row
            .try_get("name")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        symbol: row
            .try_get("symbol")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        instrument_type: row
            .try_get("instrument_type")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        quote_currency: row
            .try_get("quote_currency")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        market_code: row
            .try_get("market_code")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        country_code: row
            .try_get("country_code")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        isin: row
            .try_get("isin")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        provider_key: row
            .try_get("provider_key")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        provider_symbol: row
            .try_get("provider_symbol")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        quote_preference: row
            .try_get("quote_preference")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        note: row
            .try_get("note")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        logo_asset_id: row
            .try_get("logo_asset_id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        sort_order: sort_order_i32(
            row.try_get("sort_order")
                .map_err(|_| AppError::DatabaseUnavailable)?,
        )?,
        created_at: row
            .try_get("created_at")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        archived_at: row
            .try_get("archived_at")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        archive_instrument, create_instrument, list_instruments, restore_instrument,
        update_instrument, CreateInstrumentInput, UpdateInstrumentInput,
    };
    use crate::{
        error::AppError,
        test_support::{cleanup, onboarded_state, UNKNOWN_UUID},
    };

    fn qqq() -> CreateInstrumentInput {
        CreateInstrumentInput {
            name: "Invesco QQQ Trust".to_owned(),
            symbol: Some("qqq".to_owned()),
            instrument_type: "etf".to_owned(),
            quote_currency: "USD".to_owned(),
            market_code: Some("NASDAQ".to_owned()),
            country_code: Some("US".to_owned()),
            isin: None,
            provider_key: None,
            provider_symbol: None,
            quote_preference: Some("manual".to_owned()),
            note: None,
        }
    }

    #[test]
    fn creates_lists_archives_and_restores_instruments() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("instruments-crud").await;
            let created = create_instrument(&state, qqq()).await.expect("create");
            assert_eq!(created.symbol.as_deref(), Some("QQQ"));
            assert_eq!(created.quote_currency, "USD");
            assert_eq!(
                list_instruments(&state, false).await.expect("list").len(),
                1
            );
            archive_instrument(&state, &created.id)
                .await
                .expect("archive");
            assert!(list_instruments(&state, false)
                .await
                .expect("active")
                .is_empty());
            restore_instrument(&state, &created.id)
                .await
                .expect("restore");
            assert_eq!(
                list_instruments(&state, false)
                    .await
                    .expect("restored")
                    .len(),
                1
            );
            cleanup(&path);
        });
    }

    #[test]
    fn rejects_unknown_and_invalid_instruments() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("instruments-validate").await;
            let mut invalid = qqq();
            invalid.instrument_type = "option".to_owned();
            assert!(create_instrument(&state, invalid).await.is_err());
            let error = update_instrument(
                &state,
                UpdateInstrumentInput {
                    id: UNKNOWN_UUID.to_owned(),
                    name: "QQQ".to_owned(),
                    symbol: None,
                    instrument_type: "etf".to_owned(),
                    market_code: None,
                    country_code: None,
                    isin: None,
                    provider_key: None,
                    provider_symbol: None,
                    quote_preference: None,
                    note: None,
                },
            )
            .await
            .expect_err("missing");
            assert!(matches!(error, AppError::NotFound { entity, .. } if entity == "instrument"));
            cleanup(&path);
        });
    }
}
