use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};

use super::{
    backup_service::BACKUP_EXTENSION,
    query_count,
    reference::{begin_read_tx, finish_read_tx, map_read_error, require_household_tx},
};
use crate::{
    domain::{
        harden_spreadsheet_text, ActivityKind, CalendarDate, Checksum, HistoryTimezone, Timestamp,
        ACTIVITY_CSV_HEADERS, BENCHMARK_CSV_HEADERS, QUOTE_CSV_HEADERS,
    },
    error::AppError,
    state::AppState,
};

pub const EXPORT_FORMAT_ID: &str = "com.nestworth.export";
pub const EXPORT_FORMAT_VERSION: &str = "1";
pub const EXPORT_PRIVACY_WARNING: &str = "This file can contain household names, account names, notes, identifiers, media, and financial amounts. Save it only in a location the household already trusts.";
const JSON_EXTENSION: &str = "json";
const CSV_EXTENSION: &str = "csv";

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportCanonicalJsonInput {
    pub destination_path: String,
    pub overwrite_confirmed: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportCsvInput {
    pub destination_path: String,
    pub overwrite_confirmed: bool,
    pub dataset: CsvExportDataset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CsvExportDataset {
    Activity,
    InstrumentQuote,
    FxQuote,
    Benchmark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalExportDto {
    pub format_id: String,
    pub format_version: String,
    pub exported_at: String,
    pub privacy_warning: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CsvExportDto {
    pub template: String,
    pub row_count: i32,
    pub privacy_warning: String,
}

#[derive(Debug, Clone, Copy)]
enum JsonCol {
    Text(&'static str, &'static str),
    OptText(&'static str, &'static str),
    Int(&'static str, &'static str),
    OptInt(&'static str, &'static str),
    Bool(&'static str, &'static str),
    Blob(&'static str, &'static str),
}

pub async fn export_canonical_json(
    state: &AppState,
    input: ExportCanonicalJsonInput,
) -> Result<CanonicalExportDto, AppError> {
    let destination = validate_destination(
        &input.destination_path,
        input.overwrite_confirmed,
        JSON_EXTENSION,
    )?;
    let exported_at = Timestamp::now().to_rfc3339();
    let parent = destination_parent(&destination);
    let temp = unique_temp(parent)?;
    let result = async {
        let mut file = File::create(&temp)
            .map_err(|_| AppError::export_failed("Destination is not writable."))?;
        write_canonical_json(state, &mut file, &exported_at).await?;
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|_| AppError::export_failed("The export could not be synchronized."))?;
        Ok(())
    }
    .await;
    finalize_replaced(&temp, &destination, parent, result)?;
    Ok(CanonicalExportDto {
        format_id: EXPORT_FORMAT_ID.to_owned(),
        format_version: EXPORT_FORMAT_VERSION.to_owned(),
        exported_at,
        privacy_warning: EXPORT_PRIVACY_WARNING.to_owned(),
    })
}

pub async fn export_csv(state: &AppState, input: ExportCsvInput) -> Result<CsvExportDto, AppError> {
    let destination = validate_destination(
        &input.destination_path,
        input.overwrite_confirmed,
        CSV_EXTENSION,
    )?;
    let parent = destination_parent(&destination);
    let temp = unique_temp(parent)?;
    let result = async {
        let mut file = File::create(&temp)
            .map_err(|_| AppError::export_failed("Destination is not writable."))?;
        let row_count = write_csv_dataset(state, &mut file, input.dataset).await?;
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|_| AppError::export_failed("The export could not be synchronized."))?;
        Ok(row_count)
    }
    .await;
    let row_count = match result {
        Ok(row_count) => {
            finalize_replaced(&temp, &destination, parent, Ok(()))?;
            row_count
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
    };
    Ok(CsvExportDto {
        template: csv_template_line(input.dataset).to_owned(),
        row_count,
        privacy_warning: EXPORT_PRIVACY_WARNING.to_owned(),
    })
}

fn csv_template_line(dataset: CsvExportDataset) -> &'static str {
    match dataset {
        CsvExportDataset::Activity => "# nestworth-csv:activity:v1",
        CsvExportDataset::InstrumentQuote | CsvExportDataset::FxQuote => "# nestworth-csv:quote:v1",
        CsvExportDataset::Benchmark => "# nestworth-csv:benchmark:v1",
    }
}

async fn write_canonical_json(
    state: &AppState,
    writer: &mut impl Write,
    exported_at: &str,
) -> Result<(), AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        write_json_open(writer, exported_at)?;
        write_named_object(
            writer,
            "household",
            &mut tx,
            "SELECT id, name, base_currency, created_at, updated_at FROM households ORDER BY id",
            &[
                JsonCol::Text("id", "id"),
                JsonCol::Text("name", "name"),
                JsonCol::Text("base_currency", "baseCurrency"),
                JsonCol::Text("created_at", "createdAt"),
                JsonCol::Text("updated_at", "updatedAt"),
            ],
        )
        .await?;
        write_named_object(
            writer,
            "settings",
            &mut tx,
            "SELECT language, appearance, last_household_id, created_at, updated_at FROM app_settings WHERE id = 1",
            &[
                JsonCol::Text("language", "language"),
                JsonCol::Text("appearance", "appearance"),
                JsonCol::OptText("last_household_id", "lastHouseholdId"),
                JsonCol::Text("created_at", "createdAt"),
                JsonCol::Text("updated_at", "updatedAt"),
            ],
        )
        .await?;
        write_all_sections(writer, &mut tx, &household.id).await?;
        writer
            .write_all(b"}")
            .map_err(|_| AppError::export_failed("The export could not be written."))?;
        Ok(())
    }
    .await;
    finish_read_tx(tx, result).await
}

fn write_json_open(writer: &mut impl Write, exported_at: &str) -> Result<(), AppError> {
    write!(
        writer,
        "{{\"formatId\":{},\"formatVersion\":{},\"exportedAt\":{},\"productVersion\":{},\"databaseMigrationVersion\":5",
        json_string(EXPORT_FORMAT_ID),
        json_string(EXPORT_FORMAT_VERSION),
        json_string(exported_at),
        json_string(env!("CARGO_PKG_VERSION")),
    )
    .map_err(|_| AppError::export_failed("The export could not be written."))
}

async fn write_all_sections(
    writer: &mut impl Write,
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<(), AppError> {
    write_named_array(
        writer,
        "members",
        tx,
        "SELECT id, household_id, name, avatar_asset_id, note, sort_order, created_at, updated_at, archived_at FROM members ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("name", "name"),
            JsonCol::OptText("avatar_asset_id", "avatarAssetId"),
            JsonCol::OptText("note", "note"),
            JsonCol::Int("sort_order", "sortOrder"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::Text("updated_at", "updatedAt"),
            JsonCol::OptText("archived_at", "archivedAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "institutions",
        tx,
        "SELECT id, household_id, name, institution_type, country_code, website, note, logo_asset_id, sort_order, created_at, updated_at, archived_at FROM institutions ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("name", "name"),
            JsonCol::OptText("institution_type", "institutionType"),
            JsonCol::OptText("country_code", "countryCode"),
            JsonCol::OptText("website", "website"),
            JsonCol::OptText("note", "note"),
            JsonCol::OptText("logo_asset_id", "logoAssetId"),
            JsonCol::Int("sort_order", "sortOrder"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::Text("updated_at", "updatedAt"),
            JsonCol::OptText("archived_at", "archivedAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "groups",
        tx,
        "SELECT id, household_id, name, icon_key, color, logo_asset_id, description, sort_order, created_at, updated_at, archived_at FROM account_groups ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("name", "name"),
            JsonCol::OptText("icon_key", "iconKey"),
            JsonCol::OptText("color", "color"),
            JsonCol::OptText("logo_asset_id", "logoAssetId"),
            JsonCol::OptText("description", "description"),
            JsonCol::Int("sort_order", "sortOrder"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::Text("updated_at", "updatedAt"),
            JsonCol::OptText("archived_at", "archivedAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "accounts",
        tx,
        "SELECT id, household_id, institution_id, group_id, name, primary_category, secondary_category, tracking_mode, default_currency, note, logo_asset_id, include_in_net_worth, include_in_investment, include_in_liquid_assets, opened_on, closed_on, sort_order, created_at, updated_at, archived_at FROM accounts ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::OptText("institution_id", "institutionId"),
            JsonCol::OptText("group_id", "groupId"),
            JsonCol::Text("name", "name"),
            JsonCol::Text("primary_category", "primaryCategory"),
            JsonCol::Text("secondary_category", "secondaryCategory"),
            JsonCol::Text("tracking_mode", "trackingMode"),
            JsonCol::Text("default_currency", "defaultCurrency"),
            JsonCol::OptText("note", "note"),
            JsonCol::OptText("logo_asset_id", "logoAssetId"),
            JsonCol::Bool("include_in_net_worth", "includeInNetWorth"),
            JsonCol::Bool("include_in_investment", "includeInInvestment"),
            JsonCol::Bool("include_in_liquid_assets", "includeInLiquidAssets"),
            JsonCol::OptText("opened_on", "openedOn"),
            JsonCol::OptText("closed_on", "closedOn"),
            JsonCol::Int("sort_order", "sortOrder"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::Text("updated_at", "updatedAt"),
            JsonCol::OptText("archived_at", "archivedAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "accountOwnership",
        tx,
        "SELECT account_id, member_id, share_bps FROM account_ownership ORDER BY account_id, member_id",
        &[
            JsonCol::Text("account_id", "accountId"),
            JsonCol::Text("member_id", "memberId"),
            JsonCol::Int("share_bps", "shareBps"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "media",
        tx,
        "SELECT id, household_id, mime_type, data, created_at FROM media_assets ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("mime_type", "mimeType"),
            JsonCol::Blob("data", "data"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "instruments",
        tx,
        "SELECT id, household_id, name, symbol, instrument_type, quote_currency, market_code, country_code, isin, provider_key, provider_symbol, quote_preference, note, logo_asset_id, sort_order, created_at, updated_at, archived_at FROM instruments ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("name", "name"),
            JsonCol::OptText("symbol", "symbol"),
            JsonCol::Text("instrument_type", "instrumentType"),
            JsonCol::Text("quote_currency", "quoteCurrency"),
            JsonCol::OptText("market_code", "marketCode"),
            JsonCol::OptText("country_code", "countryCode"),
            JsonCol::OptText("isin", "isin"),
            JsonCol::OptText("provider_key", "providerKey"),
            JsonCol::OptText("provider_symbol", "providerSymbol"),
            JsonCol::Text("quote_preference", "quotePreference"),
            JsonCol::OptText("note", "note"),
            JsonCol::OptText("logo_asset_id", "logoAssetId"),
            JsonCol::Int("sort_order", "sortOrder"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::Text("updated_at", "updatedAt"),
            JsonCol::OptText("archived_at", "archivedAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "holdings",
        tx,
        "SELECT id, account_id, instrument_id, quantity, note, sort_order, created_at, updated_at, archived_at FROM holdings ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("account_id", "accountId"),
            JsonCol::Text("instrument_id", "instrumentId"),
            JsonCol::Text("quantity", "quantity"),
            JsonCol::OptText("note", "note"),
            JsonCol::Int("sort_order", "sortOrder"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::Text("updated_at", "updatedAt"),
            JsonCol::OptText("archived_at", "archivedAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "cashObservations",
        tx,
        "SELECT id, account_id, amount, currency, effective_at, created_at FROM account_cash_values ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("account_id", "accountId"),
            JsonCol::Text("amount", "amount"),
            JsonCol::Text("currency", "currency"),
            JsonCol::Text("effective_at", "effectiveAt"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "accountValueObservations",
        tx,
        "SELECT id, account_id, value_kind, amount, currency, effective_at, created_at FROM account_values ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("account_id", "accountId"),
            JsonCol::Text("value_kind", "valueKind"),
            JsonCol::Text("amount", "amount"),
            JsonCol::Text("currency", "currency"),
            JsonCol::Text("effective_at", "effectiveAt"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "instrumentQuotes",
        tx,
        "SELECT id, instrument_id, unit_price, quote_currency, source_kind, source_key, delayed, quoted_at, created_at FROM instrument_quotes ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("instrument_id", "instrumentId"),
            JsonCol::Text("unit_price", "unitPrice"),
            JsonCol::Text("quote_currency", "quoteCurrency"),
            JsonCol::Text("source_kind", "sourceKind"),
            JsonCol::Text("source_key", "sourceKey"),
            JsonCol::Bool("delayed", "delayed"),
            JsonCol::Text("quoted_at", "quotedAt"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "fxQuotes",
        tx,
        "SELECT id, household_id, base_currency, quote_currency, rate, source_kind, source_key, delayed, quoted_at, created_at FROM fx_quotes ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("base_currency", "baseCurrency"),
            JsonCol::Text("quote_currency", "quoteCurrency"),
            JsonCol::Text("rate", "rate"),
            JsonCol::Text("source_kind", "sourceKind"),
            JsonCol::Text("source_key", "sourceKey"),
            JsonCol::Bool("delayed", "delayed"),
            JsonCol::Text("quoted_at", "quotedAt"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "fxQuotePreferences",
        tx,
        "SELECT household_id, currency_a, currency_b, source_kind, created_at, updated_at FROM fx_quote_preferences ORDER BY household_id, currency_a, currency_b",
        &[
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("currency_a", "currencyA"),
            JsonCol::Text("currency_b", "currencyB"),
            JsonCol::Text("source_kind", "sourceKind"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::Text("updated_at", "updatedAt"),
        ],
    )
    .await?;
    write_named_object(
        writer,
        "historyOrigin",
        tx,
        "SELECT id, household_id, timezone, timezone_confirmed, origin_at, origin_local_date, source, schema_version, created_at FROM history_origins ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("timezone", "timezone"),
            JsonCol::Bool("timezone_confirmed", "timezoneConfirmed"),
            JsonCol::Text("origin_at", "originAt"),
            JsonCol::Text("origin_local_date", "originLocalDate"),
            JsonCol::Text("source", "source"),
            JsonCol::Int("schema_version", "schemaVersion"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "historyOriginAccountValues",
        tx,
        "SELECT origin_id, account_id, amount, currency, value_kind FROM history_origin_account_values ORDER BY origin_id, account_id",
        &[
            JsonCol::Text("origin_id", "originId"),
            JsonCol::Text("account_id", "accountId"),
            JsonCol::Text("amount", "amount"),
            JsonCol::Text("currency", "currency"),
            JsonCol::Text("value_kind", "valueKind"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "historyOriginCashValues",
        tx,
        "SELECT origin_id, account_id, currency, amount FROM history_origin_cash_values ORDER BY origin_id, account_id, currency",
        &[
            JsonCol::Text("origin_id", "originId"),
            JsonCol::Text("account_id", "accountId"),
            JsonCol::Text("currency", "currency"),
            JsonCol::Text("amount", "amount"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "historyOriginHoldings",
        tx,
        "SELECT origin_id, holding_id, account_id, instrument_id, quantity, active FROM history_origin_holdings ORDER BY origin_id, holding_id",
        &[
            JsonCol::Text("origin_id", "originId"),
            JsonCol::Text("holding_id", "holdingId"),
            JsonCol::Text("account_id", "accountId"),
            JsonCol::Text("instrument_id", "instrumentId"),
            JsonCol::Text("quantity", "quantity"),
            JsonCol::Bool("active", "active"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "historyOriginAccountStates",
        tx,
        "SELECT origin_id, account_id, primary_category, secondary_category, tracking_mode, include_in_net_worth, include_in_investment, include_in_liquid_assets, archived_at, institution_id, group_id FROM history_origin_account_states ORDER BY origin_id, account_id",
        &[
            JsonCol::Text("origin_id", "originId"),
            JsonCol::Text("account_id", "accountId"),
            JsonCol::Text("primary_category", "primaryCategory"),
            JsonCol::Text("secondary_category", "secondaryCategory"),
            JsonCol::Text("tracking_mode", "trackingMode"),
            JsonCol::Bool("include_in_net_worth", "includeInNetWorth"),
            JsonCol::Bool("include_in_investment", "includeInInvestment"),
            JsonCol::Bool("include_in_liquid_assets", "includeInLiquidAssets"),
            JsonCol::OptText("archived_at", "archivedAt"),
            JsonCol::OptText("institution_id", "institutionId"),
            JsonCol::OptText("group_id", "groupId"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "historyOriginOwnership",
        tx,
        "SELECT origin_id, account_id, member_id, share_bps FROM history_origin_ownership ORDER BY origin_id, account_id, member_id",
        &[
            JsonCol::Text("origin_id", "originId"),
            JsonCol::Text("account_id", "accountId"),
            JsonCol::Text("member_id", "memberId"),
            JsonCol::Int("share_bps", "shareBps"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "activities",
        tx,
        "SELECT id, household_id, kind, effective_at, effective_local_date, created_at, note, reverses, corrects, correction_group, income_kind, fee_kind, related_instrument_id FROM activities ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("kind", "kind"),
            JsonCol::Text("effective_at", "effectiveAt"),
            JsonCol::Text("effective_local_date", "effectiveLocalDate"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::OptText("note", "note"),
            JsonCol::OptText("reverses", "reverses"),
            JsonCol::OptText("corrects", "corrects"),
            JsonCol::OptText("correction_group", "correctionGroup"),
            JsonCol::OptText("income_kind", "incomeKind"),
            JsonCol::OptText("fee_kind", "feeKind"),
            JsonCol::OptText("related_instrument_id", "relatedInstrumentId"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "activityLegs",
        tx,
        "SELECT id, activity_id, account_id, role, direction, component_kind, amount, currency, holding_id, instrument_id, quantity, fx_rate, sort_order FROM activity_legs ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("activity_id", "activityId"),
            JsonCol::Text("account_id", "accountId"),
            JsonCol::Text("role", "role"),
            JsonCol::Text("direction", "direction"),
            JsonCol::Text("component_kind", "componentKind"),
            JsonCol::OptText("amount", "amount"),
            JsonCol::OptText("currency", "currency"),
            JsonCol::OptText("holding_id", "holdingId"),
            JsonCol::OptText("instrument_id", "instrumentId"),
            JsonCol::OptText("quantity", "quantity"),
            JsonCol::OptText("fx_rate", "fxRate"),
            JsonCol::Int("sort_order", "sortOrder"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "accountStateObservations",
        tx,
        "SELECT id, account_id, primary_category, secondary_category, tracking_mode, include_in_net_worth, include_in_investment, include_in_liquid_assets, archived_at, institution_id, group_id, effective_at, created_at FROM account_state_observations ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("account_id", "accountId"),
            JsonCol::Text("primary_category", "primaryCategory"),
            JsonCol::Text("secondary_category", "secondaryCategory"),
            JsonCol::Text("tracking_mode", "trackingMode"),
            JsonCol::Bool("include_in_net_worth", "includeInNetWorth"),
            JsonCol::Bool("include_in_investment", "includeInInvestment"),
            JsonCol::Bool("include_in_liquid_assets", "includeInLiquidAssets"),
            JsonCol::OptText("archived_at", "archivedAt"),
            JsonCol::OptText("institution_id", "institutionId"),
            JsonCol::OptText("group_id", "groupId"),
            JsonCol::Text("effective_at", "effectiveAt"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "accountStateOwnership",
        tx,
        "SELECT observation_id, member_id, share_bps FROM account_state_ownership ORDER BY observation_id, member_id",
        &[
            JsonCol::Text("observation_id", "observationId"),
            JsonCol::Text("member_id", "memberId"),
            JsonCol::Int("share_bps", "shareBps"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "holdingStateObservations",
        tx,
        "SELECT id, holding_id, active, archived_at, effective_at, created_at FROM holding_state_observations ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("holding_id", "holdingId"),
            JsonCol::Bool("active", "active"),
            JsonCol::OptText("archived_at", "archivedAt"),
            JsonCol::Text("effective_at", "effectiveAt"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "instrumentPreferenceObservations",
        tx,
        "SELECT id, instrument_id, quote_preference, effective_at, created_at FROM instrument_preference_observations ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("instrument_id", "instrumentId"),
            JsonCol::Text("quote_preference", "quotePreference"),
            JsonCol::Text("effective_at", "effectiveAt"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "fxPreferenceObservations",
        tx,
        "SELECT id, household_id, currency_a, currency_b, source_kind, effective_at, created_at FROM fx_preference_observations ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("currency_a", "currencyA"),
            JsonCol::Text("currency_b", "currencyB"),
            JsonCol::Text("source_kind", "sourceKind"),
            JsonCol::Text("effective_at", "effectiveAt"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "snapshotRevisions",
        tx,
        "SELECT id, household_id, snapshot_on, cutoff_at, revision, supersedes_snapshot_id, assets_amount, liabilities_amount, net_worth_amount, currency, is_complete, valued_component_count, total_component_count, coverage_bps, generation_reason, created_at FROM daily_valuation_snapshots ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("snapshot_on", "snapshotOn"),
            JsonCol::Text("cutoff_at", "cutoffAt"),
            JsonCol::Int("revision", "revision"),
            JsonCol::OptText("supersedes_snapshot_id", "supersedesSnapshotId"),
            JsonCol::Text("assets_amount", "assetsAmount"),
            JsonCol::Text("liabilities_amount", "liabilitiesAmount"),
            JsonCol::Text("net_worth_amount", "netWorthAmount"),
            JsonCol::Text("currency", "currency"),
            JsonCol::Bool("is_complete", "isComplete"),
            JsonCol::Int("valued_component_count", "valuedComponentCount"),
            JsonCol::Int("total_component_count", "totalComponentCount"),
            JsonCol::Int("coverage_bps", "coverageBps"),
            JsonCol::Text("generation_reason", "generationReason"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "snapshotItems",
        tx,
        "SELECT id, snapshot_id, account_id, holding_id, instrument_id, component_kind, native_amount, native_currency, base_amount, instrument_quote_id, fx_quote_id, account_state_observation_id, origin_id, activity_id, is_complete, missing_reason, sort_order FROM daily_valuation_snapshot_items ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("snapshot_id", "snapshotId"),
            JsonCol::Text("account_id", "accountId"),
            JsonCol::OptText("holding_id", "holdingId"),
            JsonCol::OptText("instrument_id", "instrumentId"),
            JsonCol::Text("component_kind", "componentKind"),
            JsonCol::OptText("native_amount", "nativeAmount"),
            JsonCol::OptText("native_currency", "nativeCurrency"),
            JsonCol::OptText("base_amount", "baseAmount"),
            JsonCol::OptText("instrument_quote_id", "instrumentQuoteId"),
            JsonCol::OptText("fx_quote_id", "fxQuoteId"),
            JsonCol::OptText("account_state_observation_id", "accountStateObservationId"),
            JsonCol::OptText("origin_id", "originId"),
            JsonCol::OptText("activity_id", "activityId"),
            JsonCol::Bool("is_complete", "isComplete"),
            JsonCol::OptText("missing_reason", "missingReason"),
            JsonCol::Int("sort_order", "sortOrder"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "costBasisDeclarations",
        tx,
        "SELECT id, household_id, origin_holding_id, activity_leg_id, instrument_id, declared_cost, declared_currency, acquired_on, revokes, is_revocation, note, created_at FROM cost_basis_declarations ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::OptText("origin_holding_id", "originHoldingId"),
            JsonCol::OptText("activity_leg_id", "activityLegId"),
            JsonCol::Text("instrument_id", "instrumentId"),
            JsonCol::OptText("declared_cost", "declaredCost"),
            JsonCol::OptText("declared_currency", "declaredCurrency"),
            JsonCol::OptText("acquired_on", "acquiredOn"),
            JsonCol::OptText("revokes", "revokes"),
            JsonCol::Bool("is_revocation", "isRevocation"),
            JsonCol::OptText("note", "note"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_sustainable_sections(writer, tx, household_id).await
}

async fn write_sustainable_sections(
    writer: &mut impl Write,
    tx: &mut Transaction<'_, Sqlite>,
    _household_id: &str,
) -> Result<(), AppError> {
    write_named_array(
        writer,
        "recurringRules",
        tx,
        "SELECT id, household_id, cadence, interval_value, start_local_date, end_local_date, anchor_local_date, kind, endpoint_account_id, endpoint_component, amount, currency, source_account_id, source_component, source_amount, source_currency, destination_account_id, destination_component, destination_amount, destination_currency, fee_amount, fee_currency, fee_kind, income_kind, related_instrument_id, liability_account_id, principal_amount, principal_currency, cash_account_id, cash_component, cash_amount, cash_currency, fx_rate, note, revision, archived_at, created_at, updated_at FROM recurring_activity_rules ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("cadence", "cadence"),
            JsonCol::Int("interval_value", "intervalValue"),
            JsonCol::Text("start_local_date", "startLocalDate"),
            JsonCol::OptText("end_local_date", "endLocalDate"),
            JsonCol::Text("anchor_local_date", "anchorLocalDate"),
            JsonCol::Text("kind", "kind"),
            JsonCol::OptText("endpoint_account_id", "endpointAccountId"),
            JsonCol::OptText("endpoint_component", "endpointComponent"),
            JsonCol::OptText("amount", "amount"),
            JsonCol::OptText("currency", "currency"),
            JsonCol::OptText("source_account_id", "sourceAccountId"),
            JsonCol::OptText("source_component", "sourceComponent"),
            JsonCol::OptText("source_amount", "sourceAmount"),
            JsonCol::OptText("source_currency", "sourceCurrency"),
            JsonCol::OptText("destination_account_id", "destinationAccountId"),
            JsonCol::OptText("destination_component", "destinationComponent"),
            JsonCol::OptText("destination_amount", "destinationAmount"),
            JsonCol::OptText("destination_currency", "destinationCurrency"),
            JsonCol::OptText("fee_amount", "feeAmount"),
            JsonCol::OptText("fee_currency", "feeCurrency"),
            JsonCol::OptText("fee_kind", "feeKind"),
            JsonCol::OptText("income_kind", "incomeKind"),
            JsonCol::OptText("related_instrument_id", "relatedInstrumentId"),
            JsonCol::OptText("liability_account_id", "liabilityAccountId"),
            JsonCol::OptText("principal_amount", "principalAmount"),
            JsonCol::OptText("principal_currency", "principalCurrency"),
            JsonCol::OptText("cash_account_id", "cashAccountId"),
            JsonCol::OptText("cash_component", "cashComponent"),
            JsonCol::OptText("cash_amount", "cashAmount"),
            JsonCol::OptText("cash_currency", "cashCurrency"),
            JsonCol::OptText("fx_rate", "fxRate"),
            JsonCol::OptText("note", "note"),
            JsonCol::Int("revision", "revision"),
            JsonCol::OptText("archived_at", "archivedAt"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::Text("updated_at", "updatedAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "pendingActivities",
        tx,
        "SELECT id, household_id, recurring_rule_id, recurring_rule_revision, scheduled_local_date, creation_source, kind, endpoint_account_id, endpoint_component, amount, currency, source_account_id, source_component, source_amount, source_currency, destination_account_id, destination_component, destination_amount, destination_currency, fee_amount, fee_currency, fee_kind, income_kind, related_instrument_id, source_holding_id, source_instrument_id, destination_holding_id, destination_instrument_id, quantity, holding_id, instrument_id, unit_price, gross_amount, gross_currency, confirm_zero_unit_price, liability_account_id, principal_amount, principal_currency, cash_account_id, cash_component, cash_amount, cash_currency, fx_rate, note, status, posted_activity_id, skipped_at, created_at, updated_at FROM pending_activities ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::OptText("recurring_rule_id", "recurringRuleId"),
            JsonCol::OptInt("recurring_rule_revision", "recurringRuleRevision"),
            JsonCol::Text("scheduled_local_date", "scheduledLocalDate"),
            JsonCol::Text("creation_source", "creationSource"),
            JsonCol::Text("kind", "kind"),
            JsonCol::OptText("endpoint_account_id", "endpointAccountId"),
            JsonCol::OptText("endpoint_component", "endpointComponent"),
            JsonCol::OptText("amount", "amount"),
            JsonCol::OptText("currency", "currency"),
            JsonCol::OptText("source_account_id", "sourceAccountId"),
            JsonCol::OptText("source_component", "sourceComponent"),
            JsonCol::OptText("source_amount", "sourceAmount"),
            JsonCol::OptText("source_currency", "sourceCurrency"),
            JsonCol::OptText("destination_account_id", "destinationAccountId"),
            JsonCol::OptText("destination_component", "destinationComponent"),
            JsonCol::OptText("destination_amount", "destinationAmount"),
            JsonCol::OptText("destination_currency", "destinationCurrency"),
            JsonCol::OptText("fee_amount", "feeAmount"),
            JsonCol::OptText("fee_currency", "feeCurrency"),
            JsonCol::OptText("fee_kind", "feeKind"),
            JsonCol::OptText("income_kind", "incomeKind"),
            JsonCol::OptText("related_instrument_id", "relatedInstrumentId"),
            JsonCol::OptText("source_holding_id", "sourceHoldingId"),
            JsonCol::OptText("source_instrument_id", "sourceInstrumentId"),
            JsonCol::OptText("destination_holding_id", "destinationHoldingId"),
            JsonCol::OptText("destination_instrument_id", "destinationInstrumentId"),
            JsonCol::OptText("quantity", "quantity"),
            JsonCol::OptText("holding_id", "holdingId"),
            JsonCol::OptText("instrument_id", "instrumentId"),
            JsonCol::OptText("unit_price", "unitPrice"),
            JsonCol::OptText("gross_amount", "grossAmount"),
            JsonCol::OptText("gross_currency", "grossCurrency"),
            JsonCol::Bool("confirm_zero_unit_price", "confirmZeroUnitPrice"),
            JsonCol::OptText("liability_account_id", "liabilityAccountId"),
            JsonCol::OptText("principal_amount", "principalAmount"),
            JsonCol::OptText("principal_currency", "principalCurrency"),
            JsonCol::OptText("cash_account_id", "cashAccountId"),
            JsonCol::OptText("cash_component", "cashComponent"),
            JsonCol::OptText("cash_amount", "cashAmount"),
            JsonCol::OptText("cash_currency", "cashCurrency"),
            JsonCol::OptText("fx_rate", "fxRate"),
            JsonCol::OptText("note", "note"),
            JsonCol::Text("status", "status"),
            JsonCol::OptText("posted_activity_id", "postedActivityId"),
            JsonCol::OptText("skipped_at", "skippedAt"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::Text("updated_at", "updatedAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "freshnessPolicies",
        tx,
        "SELECT id, household_id, kind, target_account_id, target_instrument_id, target_currency_a, target_currency_b, review_interval_days, archived_at, created_at, updated_at FROM freshness_policies ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("kind", "kind"),
            JsonCol::OptText("target_account_id", "targetAccountId"),
            JsonCol::OptText("target_instrument_id", "targetInstrumentId"),
            JsonCol::OptText("target_currency_a", "targetCurrencyA"),
            JsonCol::OptText("target_currency_b", "targetCurrencyB"),
            JsonCol::OptInt("review_interval_days", "reviewIntervalDays"),
            JsonCol::OptText("archived_at", "archivedAt"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::Text("updated_at", "updatedAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "maintenanceSnoozes",
        tx,
        "SELECT id, household_id, policy_kind, target_account_id, target_instrument_id, target_currency_a, target_currency_b, snoozed_until, created_at FROM maintenance_snoozes ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("policy_kind", "policyKind"),
            JsonCol::OptText("target_account_id", "targetAccountId"),
            JsonCol::OptText("target_instrument_id", "targetInstrumentId"),
            JsonCol::OptText("target_currency_a", "targetCurrencyA"),
            JsonCol::OptText("target_currency_b", "targetCurrencyB"),
            JsonCol::Text("snoozed_until", "snoozedUntil"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "importBatches",
        tx,
        "SELECT id, household_id, template, file_sha256, source_namespace, row_count, committed_count, duplicate_count, rejected_count, status, created_at, completed_at FROM import_batches ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("template", "template"),
            JsonCol::Text("file_sha256", "fileSha256"),
            JsonCol::OptText("source_namespace", "sourceNamespace"),
            JsonCol::Int("row_count", "rowCount"),
            JsonCol::Int("committed_count", "committedCount"),
            JsonCol::Int("duplicate_count", "duplicateCount"),
            JsonCol::Int("rejected_count", "rejectedCount"),
            JsonCol::Text("status", "status"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::OptText("completed_at", "completedAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "importItems",
        tx,
        "SELECT id, batch_id, row_number, source_namespace, external_id, fingerprint, outcome, diagnostic_code, activity_id, instrument_quote_id, fx_quote_id, benchmark_observation_id, created_at FROM import_items ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("batch_id", "batchId"),
            JsonCol::Int("row_number", "rowNumber"),
            JsonCol::OptText("source_namespace", "sourceNamespace"),
            JsonCol::OptText("external_id", "externalId"),
            JsonCol::Text("fingerprint", "fingerprint"),
            JsonCol::Text("outcome", "outcome"),
            JsonCol::OptText("diagnostic_code", "diagnosticCode"),
            JsonCol::OptText("activity_id", "activityId"),
            JsonCol::OptText("instrument_quote_id", "instrumentQuoteId"),
            JsonCol::OptText("fx_quote_id", "fxQuoteId"),
            JsonCol::OptText("benchmark_observation_id", "benchmarkObservationId"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "benchmarks",
        tx,
        "SELECT id, household_id, name, currency, series_kind, max_carry_days, archived_at, created_at, updated_at FROM benchmarks ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("name", "name"),
            JsonCol::Text("currency", "currency"),
            JsonCol::Text("series_kind", "seriesKind"),
            JsonCol::Int("max_carry_days", "maxCarryDays"),
            JsonCol::OptText("archived_at", "archivedAt"),
            JsonCol::Text("created_at", "createdAt"),
            JsonCol::Text("updated_at", "updatedAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "benchmarkObservations",
        tx,
        "SELECT id, benchmark_id, level, observed_on, note, source_kind, import_item_id, created_at FROM benchmark_observations ORDER BY id",
        &[
            JsonCol::Text("id", "id"),
            JsonCol::Text("benchmark_id", "benchmarkId"),
            JsonCol::Text("level", "level"),
            JsonCol::Text("observed_on", "observedOn"),
            JsonCol::OptText("note", "note"),
            JsonCol::Text("source_kind", "sourceKind"),
            JsonCol::OptText("import_item_id", "importItemId"),
            JsonCol::Text("created_at", "createdAt"),
        ],
    )
    .await?;
    write_named_array(
        writer,
        "householdBenchmarkPreferences",
        tx,
        "SELECT household_id, benchmark_id, updated_at FROM household_benchmark_preferences ORDER BY household_id",
        &[
            JsonCol::Text("household_id", "householdId"),
            JsonCol::Text("benchmark_id", "benchmarkId"),
            JsonCol::Text("updated_at", "updatedAt"),
        ],
    )
    .await
}

async fn write_named_object(
    writer: &mut impl Write,
    key: &str,
    tx: &mut Transaction<'_, Sqlite>,
    sql: &str,
    columns: &[JsonCol],
) -> Result<(), AppError> {
    query_count::record("export.json_section");
    let row = sqlx::query(sql)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| map_read_error("export.json_section_failed", error))?;
    write!(writer, ",{}:", json_string(key))
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    match row {
        Some(row) => write_json_object(writer, &row, columns),
        None => writer
            .write_all(b"null")
            .map_err(|_| AppError::export_failed("The export could not be written.")),
    }
}

async fn write_named_array(
    writer: &mut impl Write,
    key: &str,
    tx: &mut Transaction<'_, Sqlite>,
    sql: &str,
    columns: &[JsonCol],
) -> Result<(), AppError> {
    query_count::record("export.json_section");
    let rows = sqlx::query(sql)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("export.json_section_failed", error))?;
    write!(writer, ",{}:[", json_string(key))
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            writer
                .write_all(b",")
                .map_err(|_| AppError::export_failed("The export could not be written."))?;
        }
        write_json_object(writer, row, columns)?;
    }
    writer
        .write_all(b"]")
        .map_err(|_| AppError::export_failed("The export could not be written."))
}

fn write_json_object(
    writer: &mut impl Write,
    row: &sqlx::sqlite::SqliteRow,
    columns: &[JsonCol],
) -> Result<(), AppError> {
    writer
        .write_all(b"{")
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            writer
                .write_all(b",")
                .map_err(|_| AppError::export_failed("The export could not be written."))?;
        }
        let (json_key, value) = json_cell(row, *column)?;
        write!(writer, "{}:{}", json_string(json_key), value)
            .map_err(|_| AppError::export_failed("The export could not be written."))?;
    }
    writer
        .write_all(b"}")
        .map_err(|_| AppError::export_failed("The export could not be written."))
}

fn json_cell(
    row: &sqlx::sqlite::SqliteRow,
    column: JsonCol,
) -> Result<(&'static str, String), AppError> {
    let failed = |_| AppError::export_failed("The export could not be written.");
    match column {
        JsonCol::Text(sql, json) => {
            let value: String = row.try_get(sql).map_err(failed)?;
            Ok((json, json_string(&value)))
        }
        JsonCol::OptText(sql, json) => {
            let value: Option<String> = row.try_get(sql).map_err(failed)?;
            Ok((
                json,
                value.as_deref().map_or("null".to_owned(), json_string),
            ))
        }
        JsonCol::Int(sql, json) => {
            let value: i64 = row.try_get(sql).map_err(failed)?;
            Ok((json, value.to_string()))
        }
        JsonCol::OptInt(sql, json) => {
            let value: Option<i64> = row.try_get(sql).map_err(failed)?;
            Ok((
                json,
                value.map_or("null".to_owned(), |value| value.to_string()),
            ))
        }
        JsonCol::Bool(sql, json) => {
            let value: i64 = row.try_get(sql).map_err(failed)?;
            Ok((json, if value == 0 { "false" } else { "true" }.to_owned()))
        }
        JsonCol::Blob(sql, json) => {
            let value: Vec<u8> = row.try_get(sql).map_err(failed)?;
            Ok((json, json_string(&STANDARD.encode(value))))
        }
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

async fn write_csv_dataset(
    state: &AppState,
    writer: &mut impl Write,
    dataset: CsvExportDataset,
) -> Result<i32, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        match dataset {
            CsvExportDataset::Activity => write_activity_csv(&mut tx, writer, &household.id).await,
            CsvExportDataset::InstrumentQuote => {
                write_instrument_quote_csv(&mut tx, writer, &household.id).await
            }
            CsvExportDataset::FxQuote => write_fx_quote_csv(&mut tx, writer, &household.id).await,
            CsvExportDataset::Benchmark => {
                write_benchmark_csv(&mut tx, writer, &household.id).await
            }
        }
    }
    .await;
    finish_read_tx(tx, result).await
}

async fn write_activity_csv(
    tx: &mut Transaction<'_, Sqlite>,
    writer: &mut impl Write,
    household_id: &str,
) -> Result<i32, AppError> {
    let origin = super::history_repositories::get_origin_by_household(tx, household_id)
        .await?
        .ok_or_else(|| AppError::export_failed("History Origin is required."))?;
    let timezone = HistoryTimezone::parse(&origin.timezone)?;
    let accounts = load_name_map(
        tx,
        "SELECT id, name FROM accounts WHERE household_id = ? ORDER BY id",
        household_id,
    )
    .await?;
    let instruments = load_name_map(
        tx,
        "SELECT id, name FROM instruments WHERE household_id = ? ORDER BY id",
        household_id,
    )
    .await?;
    query_count::record("export.activity_rows");
    let activities = sqlx::query(
        "SELECT id, kind, effective_at, effective_local_date, note, income_kind, fee_kind, related_instrument_id, corrects
         FROM activities
         WHERE household_id = ?
         ORDER BY effective_local_date ASC, effective_at ASC, id ASC",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("export.activity_rows_failed", error))?;
    query_count::record("export.activity_legs");
    let legs = sqlx::query(
        "SELECT activity_id, account_id, role, component_kind, amount, currency, holding_id, instrument_id, quantity, fx_rate
         FROM activity_legs
         ORDER BY activity_id, sort_order, id",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("export.activity_legs_failed", error))?;
    let mut legs_by_activity: HashMap<String, Vec<sqlx::sqlite::SqliteRow>> = HashMap::new();
    for leg in legs {
        let activity_id: String = leg
            .try_get("activity_id")
            .map_err(|_| AppError::export_failed("Activity legs could not be exported."))?;
        legs_by_activity.entry(activity_id).or_default().push(leg);
    }
    writer
        .write_all(b"# nestworth-csv:activity:v1\r\n")
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    let mut csv = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(writer);
    csv.write_record(ACTIVITY_CSV_HEADERS)
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    let mut count = 0_i32;
    for activity in activities {
        let kind: String = activity.try_get("kind").map_err(|_| AppError::Internal)?;
        let corrects: Option<String> = activity
            .try_get("corrects")
            .map_err(|_| AppError::Internal)?;
        if kind == ActivityKind::OpeningAdjustment.as_str()
            || kind == ActivityKind::Reversal.as_str()
            || corrects.is_some()
        {
            continue;
        }
        let id: String = activity.try_get("id").map_err(|_| AppError::Internal)?;
        let effective_at = Timestamp::parse(
            activity
                .try_get("effective_at")
                .map_err(|_| AppError::Internal)?,
        )?;
        let local_date: String = activity
            .try_get("effective_local_date")
            .map_err(|_| AppError::Internal)?;
        let local_time = timezone.local_clock_hm(&effective_at);
        let ambiguous = timezone
            .ambiguous_offset_for(&effective_at)
            .map(|value| value.as_str().to_owned());
        let note: Option<String> = activity.try_get("note").map_err(|_| AppError::Internal)?;
        let income_kind: Option<String> = activity
            .try_get("income_kind")
            .map_err(|_| AppError::Internal)?;
        let fee_kind: Option<String> = activity
            .try_get("fee_kind")
            .map_err(|_| AppError::Internal)?;
        let related_instrument: Option<String> = activity
            .try_get("related_instrument_id")
            .map_err(|_| AppError::Internal)?;
        let empty = vec![];
        let activity_legs = legs_by_activity.get(&id).unwrap_or(&empty);
        let record = activity_csv_record(
            &kind,
            &local_date,
            &local_time,
            ambiguous.as_deref(),
            note.as_deref(),
            income_kind.as_deref(),
            fee_kind.as_deref(),
            related_instrument.as_deref(),
            activity_legs,
            &accounts,
            &instruments,
        )?;
        csv.write_record(&record)
            .map_err(|_| AppError::export_failed("The export could not be written."))?;
        count += 1;
    }
    csv.flush()
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    let _ = CalendarDate::parse(&origin.origin_local_date);
    Ok(count)
}

#[allow(clippy::too_many_arguments)]
fn activity_csv_record(
    kind: &str,
    local_date: &str,
    local_time: &str,
    ambiguous: Option<&str>,
    note: Option<&str>,
    income_kind: Option<&str>,
    fee_kind: Option<&str>,
    related_instrument: Option<&str>,
    legs: &[sqlx::sqlite::SqliteRow],
    accounts: &HashMap<String, String>,
    instruments: &HashMap<String, String>,
) -> Result<Vec<String>, AppError> {
    let mut values = empty_activity_row();
    set_cell(&mut values, "kind", kind);
    set_cell(&mut values, "effective_local_date", local_date);
    set_cell(&mut values, "effective_local_time", local_time);
    if let Some(value) = ambiguous {
        set_cell(&mut values, "ambiguous_offset", value);
    }
    let (note_out, note_escaped) = harden_optional(note);
    if let Some(note) = note_out {
        set_cell(&mut values, "note", &note);
    }
    if let Some(value) = income_kind {
        set_cell(&mut values, "income_kind", value);
    }
    if let Some(value) = fee_kind {
        set_cell(&mut values, "fee_kind", value);
    }
    if let Some(value) = related_instrument {
        set_cell(&mut values, "instrument_id", value);
        if let Some(label) = instruments.get(value) {
            set_cell(&mut values, "instrument_label", label);
        }
    }
    let escaped = note_escaped;
    match kind {
        "deposit" | "withdrawal" | "income" | "fee" | "balance_adjustment" | "debt_adjustment"
        | "manual_valuation" => {
            if let Some(leg) = legs.first() {
                assign_endpoint(&mut values, "", leg, accounts)?;
            }
        }
        "position_adjustment" | "buy" | "sell" => {
            if let Some(holding) = find_leg(legs, "holding").or_else(|| legs.first()) {
                assign_holding(&mut values, holding, instruments)?;
            }
            if let Some(settlement) = find_leg(legs, "settlement") {
                let amount: Option<String> = settlement
                    .try_get("amount")
                    .map_err(|_| AppError::Internal)?;
                let currency: Option<String> = settlement
                    .try_get("currency")
                    .map_err(|_| AppError::Internal)?;
                if let (Some(amount), Some(currency)) = (amount, currency) {
                    set_cell(&mut values, "gross_amount", &amount);
                    set_cell(&mut values, "settlement_currency", &currency);
                }
            }
            if let Some(fee) = find_leg(legs, "fee") {
                if let Some(amount) = opt_text(fee, "amount")? {
                    set_cell(&mut values, "fee_amount", &amount);
                }
            }
        }
        "transfer" => {
            if let Some(source) = find_leg(legs, "source") {
                assign_endpoint(&mut values, "source_", source, accounts)?;
                assign_holding_prefixed(&mut values, "source_", source, instruments)?;
            }
            if let Some(destination) = find_leg(legs, "destination") {
                assign_endpoint(&mut values, "destination_", destination, accounts)?;
                assign_holding_prefixed(&mut values, "destination_", destination, instruments)?;
            }
            if let Some(fee) = find_leg(legs, "fee") {
                if let Some(amount) = opt_text(fee, "amount")? {
                    set_cell(&mut values, "fee_amount", &amount);
                }
            }
        }
        "debt_draw" | "debt_payment" => {
            if let Some(liability) = find_leg(legs, "liability") {
                if let Some(account_id) = opt_text(liability, "account_id")? {
                    set_cell(&mut values, "liability_account_id", &account_id);
                }
                if let Some(amount) = opt_text(liability, "amount")? {
                    set_cell(&mut values, "principal_amount", &amount);
                }
                if let Some(currency) = opt_text(liability, "currency")? {
                    set_cell(&mut values, "principal_currency", &currency);
                }
            }
            if let Some(cash) = find_leg(legs, "destination").or_else(|| find_leg(legs, "source")) {
                assign_cash_endpoint(&mut values, cash)?;
            }
            if let Some(fee) = find_leg(legs, "fee") {
                if let Some(amount) = opt_text(fee, "amount")? {
                    set_cell(&mut values, "fee_amount", &amount);
                }
            }
        }
        _ => {}
    }
    if let Some(unit_price) = values_get(&values, "unit_price") {
        if unit_price == "0" {
            set_cell(&mut values, "confirm_zero_unit_price", "true");
        }
    }
    if escaped {
        set_cell(&mut values, "escaped_for_spreadsheet", "true");
    } else {
        set_cell(&mut values, "escaped_for_spreadsheet", "false");
    }
    let _ = escaped;
    Ok(values)
}

fn empty_activity_row() -> Vec<String> {
    vec![String::new(); ACTIVITY_CSV_HEADERS.len()]
}

fn set_cell(values: &mut [String], header: &str, value: &str) {
    if let Some(index) = ACTIVITY_CSV_HEADERS.iter().position(|name| *name == header) {
        values[index] = value.to_owned();
    }
}

fn values_get(values: &[String], header: &str) -> Option<String> {
    ACTIVITY_CSV_HEADERS
        .iter()
        .position(|name| *name == header)
        .and_then(|index| values.get(index).cloned())
        .filter(|value| !value.is_empty())
}

fn harden_optional(value: Option<&str>) -> (Option<String>, bool) {
    match value {
        Some(value) => {
            let (text, escaped) = harden_spreadsheet_text(value);
            (Some(text), escaped)
        }
        None => (None, false),
    }
}

fn find_leg<'a>(
    legs: &'a [sqlx::sqlite::SqliteRow],
    role: &str,
) -> Option<&'a sqlx::sqlite::SqliteRow> {
    legs.iter().find(|leg| {
        leg.try_get::<String, _>("role")
            .ok()
            .is_some_and(|value| value == role)
    })
}

fn opt_text(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<String>, AppError> {
    row.try_get(column).map_err(|_| AppError::Internal)
}

fn assign_endpoint(
    values: &mut [String],
    prefix: &str,
    leg: &sqlx::sqlite::SqliteRow,
    accounts: &HashMap<String, String>,
) -> Result<(), AppError> {
    let account_id: String = leg.try_get("account_id").map_err(|_| AppError::Internal)?;
    let component: String = leg
        .try_get("component_kind")
        .map_err(|_| AppError::Internal)?;
    let amount = opt_text(leg, "amount")?;
    let currency = opt_text(leg, "currency")?;
    if prefix.is_empty() {
        set_cell(values, "account_id", &account_id);
        set_cell(values, "component_kind", &component);
        if let Some(label) = accounts.get(&account_id) {
            set_cell(values, "account_label", label);
        }
        if let Some(amount) = amount {
            set_cell(values, "amount", &amount);
        }
        if let Some(currency) = currency {
            set_cell(values, "currency", &currency);
        }
    } else {
        set_cell(values, &format!("{prefix}account_id"), &account_id);
        set_cell(values, &format!("{prefix}component"), &component);
        if let Some(label) = accounts.get(&account_id) {
            set_cell(values, &format!("{prefix}account_label"), label);
        }
        if let Some(amount) = amount {
            set_cell(values, &format!("{prefix}amount"), &amount);
        }
        if let Some(currency) = currency {
            set_cell(values, &format!("{prefix}currency"), &currency);
        }
    }
    Ok(())
}

fn assign_holding(
    values: &mut [String],
    leg: &sqlx::sqlite::SqliteRow,
    instruments: &HashMap<String, String>,
) -> Result<(), AppError> {
    if let Some(holding_id) = opt_text(leg, "holding_id")? {
        set_cell(values, "holding_id", &holding_id);
    }
    if let Some(instrument_id) = opt_text(leg, "instrument_id")? {
        set_cell(values, "instrument_id", &instrument_id);
        if let Some(label) = instruments.get(&instrument_id) {
            set_cell(values, "instrument_label", label);
        }
    }
    if let Some(quantity) = opt_text(leg, "quantity")? {
        set_cell(values, "quantity", &quantity);
    }
    Ok(())
}

fn assign_holding_prefixed(
    values: &mut [String],
    prefix: &str,
    leg: &sqlx::sqlite::SqliteRow,
    instruments: &HashMap<String, String>,
) -> Result<(), AppError> {
    if prefix == "source_" {
        if let Some(holding_id) = opt_text(leg, "holding_id")? {
            set_cell(values, "holding_id", &holding_id);
        }
    }
    if let Some(instrument_id) = opt_text(leg, "instrument_id")? {
        set_cell(values, "instrument_id", &instrument_id);
        if let Some(label) = instruments.get(&instrument_id) {
            set_cell(values, "instrument_label", label);
        }
    }
    if let Some(quantity) = opt_text(leg, "quantity")? {
        set_cell(values, "quantity", &quantity);
    }
    if let Some(fx_rate) = opt_text(leg, "fx_rate")? {
        set_cell(values, "fx_rate", &fx_rate);
    }
    Ok(())
}

fn assign_cash_endpoint(
    values: &mut [String],
    leg: &sqlx::sqlite::SqliteRow,
) -> Result<(), AppError> {
    if let Some(account_id) = opt_text(leg, "account_id")? {
        set_cell(values, "cash_account_id", &account_id);
    }
    if let Some(component) = opt_text(leg, "component_kind")? {
        set_cell(values, "cash_component", &component);
    }
    if let Some(amount) = opt_text(leg, "amount")? {
        set_cell(values, "cash_amount", &amount);
    }
    if let Some(currency) = opt_text(leg, "currency")? {
        set_cell(values, "cash_currency", &currency);
    }
    if let Some(fx_rate) = opt_text(leg, "fx_rate")? {
        set_cell(values, "fx_rate", &fx_rate);
    }
    Ok(())
}

async fn write_instrument_quote_csv(
    tx: &mut Transaction<'_, Sqlite>,
    writer: &mut impl Write,
    household_id: &str,
) -> Result<i32, AppError> {
    let instruments = load_name_map(
        tx,
        "SELECT id, name FROM instruments WHERE household_id = ? ORDER BY id",
        household_id,
    )
    .await?;
    query_count::record("export.instrument_quotes");
    let rows = sqlx::query(
        "SELECT q.instrument_id, q.unit_price, q.quoted_at
         FROM instrument_quotes q
         INNER JOIN instruments i ON i.id = q.instrument_id
         WHERE i.household_id = ? AND q.source_kind = 'manual'
         ORDER BY q.instrument_id, q.quoted_at, q.id",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("export.instrument_quotes_failed", error))?;
    writer
        .write_all(b"# nestworth-csv:quote:v1\r\n")
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    let mut csv = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(writer);
    csv.write_record(QUOTE_CSV_HEADERS)
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    for row in &rows {
        let instrument_id: String = row
            .try_get("instrument_id")
            .map_err(|_| AppError::Internal)?;
        let unit_price: String = row.try_get("unit_price").map_err(|_| AppError::Internal)?;
        let quoted_at: String = row.try_get("quoted_at").map_err(|_| AppError::Internal)?;
        let label = instruments.get(&instrument_id).cloned().unwrap_or_default();
        csv.write_record([
            "",
            "",
            "instrument",
            &instrument_id,
            &label,
            "",
            "",
            &unit_price,
            "",
            &quoted_at,
            "",
            "false",
        ])
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    }
    csv.flush()
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    i32::try_from(rows.len()).map_err(|_| AppError::export_failed("The export is too large."))
}

async fn write_fx_quote_csv(
    tx: &mut Transaction<'_, Sqlite>,
    writer: &mut impl Write,
    household_id: &str,
) -> Result<i32, AppError> {
    query_count::record("export.fx_quotes");
    let rows = sqlx::query(
        "SELECT base_currency, quote_currency, rate, quoted_at
         FROM fx_quotes
         WHERE household_id = ? AND source_kind = 'manual'
         ORDER BY base_currency, quote_currency, quoted_at, id",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("export.fx_quotes_failed", error))?;
    writer
        .write_all(b"# nestworth-csv:quote:v1\r\n")
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    let mut csv = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(writer);
    csv.write_record(QUOTE_CSV_HEADERS)
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    for row in &rows {
        csv.write_record([
            "",
            "",
            "fx",
            "",
            "",
            row.try_get::<String, _>("base_currency")
                .map_err(|_| AppError::Internal)?
                .as_str(),
            row.try_get::<String, _>("quote_currency")
                .map_err(|_| AppError::Internal)?
                .as_str(),
            "",
            row.try_get::<String, _>("rate")
                .map_err(|_| AppError::Internal)?
                .as_str(),
            row.try_get::<String, _>("quoted_at")
                .map_err(|_| AppError::Internal)?
                .as_str(),
            "",
            "false",
        ])
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    }
    csv.flush()
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    i32::try_from(rows.len()).map_err(|_| AppError::export_failed("The export is too large."))
}

async fn write_benchmark_csv(
    tx: &mut Transaction<'_, Sqlite>,
    writer: &mut impl Write,
    household_id: &str,
) -> Result<i32, AppError> {
    let benchmarks = load_name_map(
        tx,
        "SELECT id, name FROM benchmarks WHERE household_id = ? ORDER BY id",
        household_id,
    )
    .await?;
    query_count::record("export.benchmark_observations");
    let rows = sqlx::query(
        "SELECT o.benchmark_id, o.observed_on, o.level, o.note
         FROM benchmark_observations o
         INNER JOIN benchmarks b ON b.id = o.benchmark_id
         WHERE b.household_id = ?
         ORDER BY o.benchmark_id, o.observed_on, o.created_at, o.id",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("export.benchmark_observations_failed", error))?;
    writer
        .write_all(b"# nestworth-csv:benchmark:v1\r\n")
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    let mut csv = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(writer);
    csv.write_record(BENCHMARK_CSV_HEADERS)
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    for row in &rows {
        let benchmark_id: String = row
            .try_get("benchmark_id")
            .map_err(|_| AppError::Internal)?;
        let note: Option<String> = row.try_get("note").map_err(|_| AppError::Internal)?;
        let (note_out, escaped) = harden_optional(note.as_deref());
        csv.write_record([
            "",
            "",
            &benchmark_id,
            benchmarks
                .get(&benchmark_id)
                .cloned()
                .unwrap_or_default()
                .as_str(),
            row.try_get::<String, _>("observed_on")
                .map_err(|_| AppError::Internal)?
                .as_str(),
            row.try_get::<String, _>("level")
                .map_err(|_| AppError::Internal)?
                .as_str(),
            note_out.as_deref().unwrap_or(""),
            if escaped { "true" } else { "false" },
        ])
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    }
    csv.flush()
        .map_err(|_| AppError::export_failed("The export could not be written."))?;
    i32::try_from(rows.len()).map_err(|_| AppError::export_failed("The export is too large."))
}

async fn load_name_map(
    tx: &mut Transaction<'_, Sqlite>,
    sql: &str,
    household_id: &str,
) -> Result<HashMap<String, String>, AppError> {
    query_count::record("export.reference_names");
    let rows = sqlx::query(sql)
        .bind(household_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("export.reference_names_failed", error))?;
    let mut map = HashMap::new();
    for row in rows {
        map.insert(
            row.try_get("id").map_err(|_| AppError::Internal)?,
            row.try_get("name").map_err(|_| AppError::Internal)?,
        );
    }
    Ok(map)
}

fn validate_destination(
    raw_path: &str,
    overwrite_confirmed: bool,
    extension: &str,
) -> Result<PathBuf, AppError> {
    if raw_path.trim().is_empty() {
        return Err(AppError::validation(
            "destinationPath",
            "An export destination is required.",
        ));
    }
    let path = PathBuf::from(raw_path);
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| value != extension)
    {
        return Err(AppError::export_failed(
            "The destination does not use the required export extension.",
        ));
    }
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value == BACKUP_EXTENSION)
    {
        return Err(AppError::export_failed(
            "Backup files cannot be used as export destinations.",
        ));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_metadata = fs::metadata(parent)
        .map_err(|_| AppError::export_failed("The destination folder is unavailable."))?;
    if !parent_metadata.is_dir() {
        return Err(AppError::export_failed(
            "The destination folder is unavailable.",
        ));
    }
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || metadata.is_dir() {
                return Err(AppError::export_failed(
                    "The destination is not a regular file.",
                ));
            }
            if !overwrite_confirmed {
                return Err(AppError::validation(
                    "overwriteConfirmed",
                    "Overwriting an existing export requires explicit confirmation.",
                ));
            }
            Ok(path)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path),
        Err(_error) => Err(AppError::export_failed(
            "The destination could not be inspected.",
        )),
    }
}

fn destination_parent(destination: &Path) -> &Path {
    destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn finalize_replaced(
    temp: &Path,
    destination: &Path,
    parent: &Path,
    result: Result<(), AppError>,
) -> Result<(), AppError> {
    match result {
        Ok(()) => {
            File::open(temp)
                .and_then(|file| file.sync_all())
                .map_err(|_| AppError::export_failed("The export could not be synchronized."))?;
            fs::rename(temp, destination).map_err(|_| {
                AppError::export_failed("The export could not replace the destination.")
            })?;
            File::open(parent)
                .and_then(|file| file.sync_all())
                .map_err(|_| AppError::export_failed("The export could not be synchronized."))?;
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(temp);
            Err(error)
        }
    }
}

fn unique_temp(parent: &Path) -> Result<PathBuf, AppError> {
    for _ in 0..8 {
        let path = parent.join(format!(".nestworth-export-{}.tmp", uuid::Uuid::now_v7()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                drop(file);
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_error) => return Err(AppError::export_failed("Destination is not writable.")),
        }
    }
    Err(AppError::export_failed(
        "A temporary destination could not be allocated.",
    ))
}

pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
    Checksum::sha256(bytes).hex()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            account_service::{self, CreateAccountInput, OwnershipShareInput},
            backup_service::{self, CreateBackupInput, InspectBackupInput},
            history_query_service::{
                confirm_history_timezone, create_activity, get_history_origin,
                ConfirmHistoryTimezoneInput, CreateActivityInput,
            },
            onboarding_service::complete_onboarding,
        },
        test_support::{test_path, valid_onboarding_input},
    };
    use std::fs;

    fn cleanup(path: &PathBuf) {
        let _ = fs::remove_file(path);
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = path.as_os_str().to_os_string();
            sidecar.push(suffix);
            let _ = fs::remove_file(PathBuf::from(sidecar));
        }
    }

    async fn onboarded_state(name: &str) -> (AppState, PathBuf) {
        let root = test_path("phase7", name);
        cleanup(&root);
        let state = AppState::initialize(root.clone()).await;
        complete_onboarding(&state, valid_onboarding_input())
            .await
            .expect("onboarding");
        let origin = get_history_origin(&state).await.expect("origin");
        if !origin.timezone_confirmed {
            confirm_history_timezone(
                &state,
                ConfirmHistoryTimezoneInput {
                    timezone: origin.timezone.clone(),
                },
            )
            .await
            .expect("confirm timezone");
        }
        sqlx::query("UPDATE history_origins SET origin_at = '2000-01-01T00:00:00.000Z'")
            .execute(state.writable_db().expect("db"))
            .await
            .expect("origin start");
        (state, root)
    }

    async fn create_cash_account(state: &AppState) -> String {
        let member_id: String = sqlx::query_scalar("SELECT id FROM members LIMIT 1")
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("member");
        account_service::create_account(
            state,
            CreateAccountInput {
                name: "Salary".to_owned(),
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
                owners: vec![OwnershipShareInput {
                    member_id,
                    percent: Some("100".to_owned()),
                    share_bps: None,
                }],
                initial_amount: Some("1000".to_owned()),
            },
        )
        .await
        .expect("account")
        .id
    }

    fn strip_exported_at(bytes: &[u8]) -> String {
        let value: serde_json::Value = serde_json::from_slice(bytes).expect("json");
        let mut object = value.as_object().expect("object").clone();
        object.remove("exportedAt");
        serde_json::to_string(&object).expect("canonical")
    }

    #[test]
    fn identical_state_exports_byte_identical_json_except_exported_at() {
        tauri::async_runtime::block_on(async {
            let (state, root) = onboarded_state("json-identical").await;
            let first = root.with_extension("first.json");
            let second = root.with_extension("second.json");
            super::export_canonical_json(
                &state,
                ExportCanonicalJsonInput {
                    destination_path: first.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                },
            )
            .await
            .expect("first export");
            super::export_canonical_json(
                &state,
                ExportCanonicalJsonInput {
                    destination_path: second.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                },
            )
            .await
            .expect("second export");
            let left = fs::read(&first).expect("first bytes");
            let right = fs::read(&second).expect("second bytes");
            assert_ne!(left, right);
            assert_eq!(strip_exported_at(&left), strip_exported_at(&right));
            let parsed: serde_json::Value = serde_json::from_slice(&left).expect("json");
            assert_eq!(parsed["formatId"], EXPORT_FORMAT_ID);
            assert!(parsed.get("household").is_some());
            assert!(parsed.get("members").is_some());
            assert!(parsed.get("activities").is_some());
            assert!(parsed.get("activityLegs").is_some());
            assert!(parsed.get("media").is_some());
            assert!(parsed.get("freshnessPolicies").is_some());
            assert!(parsed.get("holdingQuantityValues").is_none());
            assert!(parsed.get("historySnapshotState").is_none());
            assert!(parsed.get("lots").is_none());
            let raw = String::from_utf8(left).expect("utf8");
            assert!(!raw.contains("holding_quantity_values"));
            assert!(!raw.contains("history_snapshot_state"));
            drop(state);
            let _ = fs::remove_file(first);
            let _ = fs::remove_file(second);
            cleanup(&root);
        });
    }

    #[test]
    fn csv_export_is_kind_specific_and_round_trips_hardened_text() {
        tauri::async_runtime::block_on(async {
            let (state, root) = onboarded_state("csv-roundtrip").await;
            let account_id = create_cash_account(&state).await;
            let origin = get_history_origin(&state).await.expect("origin");
            create_activity(
                &state,
                CreateActivityInput::Deposit {
                    local_date: origin.origin_local_date.clone(),
                    local_time: "09:00".to_owned(),
                    ambiguous_offset: None,
                    note: Some("=SUM(A1) January salary".to_owned()),
                    account_id: account_id.clone(),
                    component: "account_value".to_owned(),
                    amount: "5000".to_owned(),
                    currency: "CNY".to_owned(),
                },
            )
            .await
            .expect("deposit");
            let destination = root.with_extension("activities.csv");
            let exported = super::export_csv(
                &state,
                ExportCsvInput {
                    destination_path: destination.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                    dataset: CsvExportDataset::Activity,
                },
            )
            .await
            .expect("csv export");
            assert_eq!(exported.template, "# nestworth-csv:activity:v1");
            assert!(exported.row_count >= 1);
            let text = fs::read_to_string(&destination).expect("csv text");
            assert!(text.starts_with("# nestworth-csv:activity:v1\r\n"));
            assert!(text.contains("deposit"));
            assert!(text.contains("'=SUM(A1) January salary"));
            assert!(!text.contains("\nrole,"));
            assert!(!text.contains("sort_order"));
            drop(state);
            let _ = fs::remove_file(destination);
            cleanup(&root);
        });
    }

    #[test]
    fn json_is_rejected_as_restore_input() {
        tauri::async_runtime::block_on(async {
            let (state, root) = onboarded_state("json-not-restore").await;
            let json_path = root.with_extension("export.json");
            super::export_canonical_json(
                &state,
                ExportCanonicalJsonInput {
                    destination_path: json_path.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                },
            )
            .await
            .expect("json export");
            let inspect = backup_service::inspect_backup(
                &state,
                InspectBackupInput {
                    source_path: json_path.to_string_lossy().into_owned(),
                },
            )
            .await;
            assert!(inspect.is_err());
            let renamed = root.with_extension(BACKUP_EXTENSION);
            fs::copy(&json_path, &renamed).expect("copy json as backup extension");
            let inspect_renamed = backup_service::inspect_backup(
                &state,
                InspectBackupInput {
                    source_path: renamed.to_string_lossy().into_owned(),
                },
            )
            .await;
            assert!(inspect_renamed.is_err());
            drop(state);
            let _ = fs::remove_file(json_path);
            let _ = fs::remove_file(renamed);
            cleanup(&root);
        });
    }

    #[test]
    fn export_does_not_delete_recovery_files() {
        tauri::async_runtime::block_on(async {
            let (state, root) = onboarded_state("recovery-untouched").await;
            let recovery = PathBuf::from(format!(
                "{}{}",
                root.display(),
                ".recovery-keep.nestworth-backup"
            ));
            fs::write(&recovery, b"keep").expect("recovery fixture");
            let destination = root.with_extension("keep.json");
            super::export_canonical_json(
                &state,
                ExportCanonicalJsonInput {
                    destination_path: destination.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                },
            )
            .await
            .expect("export");
            assert_eq!(fs::read(&recovery).expect("recovery bytes"), b"keep");
            drop(state);
            let _ = fs::remove_file(destination);
            let _ = fs::remove_file(recovery);
            cleanup(&root);
        });
    }

    #[test]
    fn existing_destination_is_unchanged_without_overwrite() {
        tauri::async_runtime::block_on(async {
            let (state, root) = onboarded_state("overwrite-guard").await;
            let destination = root.with_extension("guard.json");
            fs::write(&destination, b"original").expect("existing");
            let error = super::export_canonical_json(
                &state,
                ExportCanonicalJsonInput {
                    destination_path: destination.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                },
            )
            .await
            .expect_err("overwrite should be rejected");
            assert!(matches!(error, AppError::Validation { .. }));
            assert_eq!(fs::read(&destination).expect("bytes"), b"original");
            drop(state);
            let _ = fs::remove_file(destination);
            cleanup(&root);
        });
    }

    #[test]
    fn backup_create_input_is_not_used_by_export_paths() {
        let _ = CreateBackupInput {
            destination_path: "unused.nestworth-backup".to_owned(),
            overwrite_confirmed: false,
        };
    }
}
