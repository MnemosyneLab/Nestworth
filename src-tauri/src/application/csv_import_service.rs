//! Atomic CSV import commit, provenance persistence, and import-batch queries.

use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    backup_service::same_file_metadata,
    benchmark_service,
    csv_preview_service::{
        activity_input, activity_row_fingerprint, benchmark_row_fingerprint, check_reference,
        csv_cell, diagnostic, load_catalog, open_csv_source, parse_csv_bytes, parse_escaped,
        quote_row_fingerprint, reject_localized, resolve_identity, revalidate_csv_preview_token,
        unescape, Catalog, CatalogRef, CsvImportDiagnosticDto, IdentityAction, IdentityRecord,
        ParsedCsv, ParsedRow, RowOutcome,
    },
    history_query_service::post_create_activity_in_tx,
    quote_service::{
        append_imported_manual_fx_quote_in_tx, append_imported_manual_instrument_quote_in_tx,
    },
    reference::{
        begin_read_tx, begin_write_tx, finish_read_tx, finish_write_tx, require_household_tx,
    },
    sustainable_repositories::{self, ImportBatchRecord, ImportItemRecord},
};
use crate::{
    domain::{
        looks_localized_date, looks_localized_decimal, optional_text, parse_optional_note,
        BenchmarkLevel, BenchmarkObservationSourceKind, CalendarDate, CurrencyCode, FxRate,
        ImportBatchId, ImportItemId, ImportTemplate, Timestamp, UnitPrice, CSV_ESCAPED_COLUMN,
        DIAGNOSTIC_DOMAIN_INVALID, DIAGNOSTIC_DUPLICATE_CONFLICT, DIAGNOSTIC_EXACT_DUPLICATE,
        DIAGNOSTIC_KIND_FORBIDDEN, DIAGNOSTIC_LOCALIZED_VALUE, DIAGNOSTIC_NO_IDENTITY_WARNING,
    },
    error::AppError,
    state::AppState,
};

const DEFAULT_PAGE_SIZE: i64 = 50;
const MAX_PAGE_SIZE: i64 = 100;

thread_local! {
    static IMPORT_FAILPOINT: Cell<Option<&'static str>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_import_failpoint(name: Option<&'static str>) {
    IMPORT_FAILPOINT.with(|cell| cell.set(name));
}

fn failpoint(name: &'static str) -> Result<(), AppError> {
    let hit = IMPORT_FAILPOINT.with(|cell| cell.get() == Some(name));
    if hit {
        Err(AppError::import_commit_failed("Import persistence failed."))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommitCsvImportInput {
    pub preview_token: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CsvImportCommitDto {
    pub batch_id: String,
    pub template: String,
    pub row_count: i32,
    pub committed_count: i32,
    pub duplicate_count: i32,
    pub warning_count: i32,
    pub diagnostics: Vec<CsvImportDiagnosticDto>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListImportBatchesInput {
    pub cursor: Option<String>,
    pub limit: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportBatchDto {
    pub id: String,
    pub template: String,
    pub file_sha256: String,
    pub source_namespace: Option<String>,
    pub row_count: i32,
    pub committed_count: i32,
    pub duplicate_count: i32,
    pub rejected_count: i32,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportBatchPageDto {
    pub items: Vec<ImportBatchDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GetImportBatchInput {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportItemDto {
    pub id: String,
    pub row_number: i32,
    pub source_namespace: Option<String>,
    pub external_id: Option<String>,
    pub fingerprint: String,
    pub outcome: String,
    pub diagnostic_code: Option<String>,
    pub activity_id: Option<String>,
    pub instrument_quote_id: Option<String>,
    pub fx_quote_id: Option<String>,
    pub benchmark_observation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportBatchDetailDto {
    pub batch: ImportBatchDto,
    pub items: Vec<ImportItemDto>,
}

struct PlannedItem {
    row_number: i64,
    source_namespace: Option<String>,
    external_id: Option<String>,
    fingerprint: String,
    outcome: &'static str,
    diagnostic_code: Option<String>,
    activity_id: Option<String>,
    instrument_quote_id: Option<String>,
    fx_quote_id: Option<String>,
    benchmark_observation_id: Option<String>,
}

pub async fn commit_csv_import(
    state: &AppState,
    input: CommitCsvImportInput,
) -> Result<CsvImportCommitDto, AppError> {
    if !input.confirmed {
        return Err(AppError::validation(
            "confirmed",
            "Import commit must be confirmed.",
        ));
    }
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = commit_in_tx(&mut tx, state, &input.preview_token).await;
    finish_write_tx(tx, result).await
}

async fn commit_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    state: &AppState,
    preview_token: &str,
) -> Result<CsvImportCommitDto, AppError> {
    let stored = revalidate_csv_preview_token(state, preview_token)?;
    let opened = open_csv_source(&stored.canonical_path.to_string_lossy())
        .map_err(|_| AppError::import_file_changed())?;
    let expected = crate::application::backup_service::SourceMetadata {
        len: stored.file_size,
        modified: stored.modified_at,
        device: stored.file_device,
        inode: stored.file_inode,
    };
    if !same_file_metadata(&opened.metadata, &expected) || opened.sha256 != stored.sha256 {
        return Err(AppError::import_file_changed());
    }
    let parsed = parse_csv_bytes(&opened.bytes)?;
    let household = require_household_tx(tx).await?;
    let catalog = load_catalog(tx, &household.id, &parsed).await?;
    let mut seen_identities: HashMap<(String, String), IdentityRecord> = HashMap::new();
    let mut seen_no_id = HashSet::new();
    let mut planned = Vec::new();
    let mut diagnostics = Vec::new();
    let mut errors = Vec::new();
    for row in sorted_rows(&parsed) {
        let outcome = match parsed.template {
            ImportTemplate::ActivityV1 => {
                commit_activity_row(
                    tx,
                    &household.id,
                    row,
                    &catalog,
                    &mut seen_identities,
                    &mut seen_no_id,
                )
                .await
            }
            ImportTemplate::QuoteV1 => {
                commit_quote_row(
                    tx,
                    &household.id,
                    row,
                    &catalog,
                    &mut seen_identities,
                    &mut seen_no_id,
                )
                .await
            }
            ImportTemplate::BenchmarkV1 => {
                commit_benchmark_row(
                    tx,
                    &household.id,
                    row,
                    &catalog,
                    &mut seen_identities,
                    &mut seen_no_id,
                )
                .await
            }
        };
        match outcome {
            Ok(CommitRow::Committed(item, warning)) => {
                if let Some(warning) = warning {
                    diagnostics.push(warning);
                }
                planned.push(item);
            }
            Ok(CommitRow::Duplicate(item, diagnostic)) => {
                diagnostics.push(diagnostic);
                planned.push(item);
            }
            Err(diagnostic) => errors.push(diagnostic),
        }
    }
    if let Some(first) = errors.into_iter().next() {
        return Err(AppError::import_rejected(
            first.row,
            &first.field,
            &first.code,
        ));
    }
    failpoint("batch")?;
    let now = Timestamp::now();
    let committed_count = planned
        .iter()
        .filter(|item| item.outcome == "committed")
        .count();
    let duplicate_count = planned
        .iter()
        .filter(|item| item.outcome == "exact_duplicate")
        .count();
    let batch_id = ImportBatchId::new().to_string();
    let source_namespace = unique_namespace(&planned);
    let batch = ImportBatchRecord {
        id: batch_id.clone(),
        household_id: household.id.clone(),
        template: parsed.template.as_str().to_owned(),
        file_sha256: opened.sha256,
        source_namespace,
        row_count: i64::from(i32::try_from(parsed.rows.len()).unwrap_or(i32::MAX)),
        committed_count: i64::try_from(committed_count).unwrap_or(i64::MAX),
        duplicate_count: i64::try_from(duplicate_count).unwrap_or(i64::MAX),
        rejected_count: 0,
        status: "committed".to_owned(),
        created_at: now.to_rfc3339(),
        completed_at: Some(now.to_rfc3339()),
    };
    sustainable_repositories::insert_import_batch(tx, &batch).await?;
    for item in &planned {
        failpoint("item")?;
        sustainable_repositories::insert_import_item(
            tx,
            &ImportItemRecord {
                id: ImportItemId::new().to_string(),
                batch_id: batch_id.clone(),
                row_number: item.row_number,
                source_namespace: item.source_namespace.clone(),
                external_id: item.external_id.clone(),
                fingerprint: item.fingerprint.clone(),
                outcome: item.outcome.to_owned(),
                diagnostic_code: item.diagnostic_code.clone(),
                activity_id: item.activity_id.clone(),
                instrument_quote_id: item.instrument_quote_id.clone(),
                fx_quote_id: item.fx_quote_id.clone(),
                benchmark_observation_id: item.benchmark_observation_id.clone(),
                created_at: now.to_rfc3339(),
            },
        )
        .await?;
    }
    let warning_count = diagnostics
        .iter()
        .filter(|item| item.severity == "warning")
        .count();
    tracing::info!(
        event = "import.commit",
        template = parsed.template.as_str(),
        row_count = batch.row_count,
        committed_count = batch.committed_count,
        duplicate_count = batch.duplicate_count,
        "csv import committed"
    );
    Ok(CsvImportCommitDto {
        batch_id,
        template: parsed.template.as_str().to_owned(),
        row_count: i32::try_from(parsed.rows.len()).unwrap_or(i32::MAX),
        committed_count: i32::try_from(committed_count).unwrap_or(i32::MAX),
        duplicate_count: i32::try_from(duplicate_count).unwrap_or(i32::MAX),
        warning_count: i32::try_from(warning_count).unwrap_or(i32::MAX),
        diagnostics,
    })
}

enum CommitRow {
    Committed(PlannedItem, Option<CsvImportDiagnosticDto>),
    Duplicate(PlannedItem, CsvImportDiagnosticDto),
}

fn sorted_rows(parsed: &ParsedCsv) -> Vec<&ParsedRow> {
    let mut rows: Vec<&ParsedRow> = parsed.rows.iter().collect();
    if parsed.template == ImportTemplate::ActivityV1 {
        rows.sort_by_key(|row| {
            let escaped = parse_escaped(row).unwrap_or(false);
            (
                unescape(csv_cell(row, "effective_local_date"), escaped),
                unescape(csv_cell(row, "effective_local_time"), escaped),
                row.number,
            )
        });
    }
    rows
}

fn unique_namespace(items: &[PlannedItem]) -> Option<String> {
    let mut namespaces = HashSet::new();
    for item in items {
        if let Some(namespace) = &item.source_namespace {
            namespaces.insert(namespace.clone());
        }
    }
    if namespaces.len() == 1 {
        namespaces.into_iter().next()
    } else {
        None
    }
}

fn remember_identity(
    seen: &mut HashMap<(String, String), IdentityRecord>,
    namespace: Option<String>,
    external_id: Option<String>,
    record: IdentityRecord,
) {
    if let (Some(namespace), Some(external_id)) = (namespace, external_id) {
        seen.insert((namespace, external_id), record);
    }
}

fn planned_from_identity(
    row: &ParsedRow,
    namespace: Option<String>,
    external_id: Option<String>,
    fingerprint: String,
    outcome: &'static str,
    diagnostic_code: Option<String>,
    record: &IdentityRecord,
) -> PlannedItem {
    PlannedItem {
        row_number: i64::from(row.number),
        source_namespace: namespace,
        external_id,
        fingerprint,
        outcome,
        diagnostic_code,
        activity_id: record.activity_id.clone(),
        instrument_quote_id: record.instrument_quote_id.clone(),
        fx_quote_id: record.fx_quote_id.clone(),
        benchmark_observation_id: record.benchmark_observation_id.clone(),
    }
}

fn duplicate_item(
    row: &ParsedRow,
    namespace: String,
    external_id: String,
    fingerprint: String,
    prior: IdentityRecord,
) -> Result<CommitRow, CsvImportDiagnosticDto> {
    if prior.activity_id.is_none()
        && prior.instrument_quote_id.is_none()
        && prior.fx_quote_id.is_none()
        && prior.benchmark_observation_id.is_none()
    {
        return Err(diagnostic(
            row.number,
            "external_id",
            DIAGNOSTIC_DOMAIN_INVALID,
            "error",
        ));
    }
    let diagnostic = diagnostic(
        row.number,
        "external_id",
        DIAGNOSTIC_EXACT_DUPLICATE,
        "warning",
    );
    Ok(CommitRow::Duplicate(
        planned_from_identity(
            row,
            Some(namespace),
            Some(external_id),
            fingerprint,
            "exact_duplicate",
            Some(DIAGNOSTIC_EXACT_DUPLICATE.to_owned()),
            &prior,
        ),
        diagnostic,
    ))
}

fn identity_error(row: &ParsedRow, action: IdentityAction) -> CsvImportDiagnosticDto {
    match action {
        IdentityAction::Conflict => diagnostic(
            row.number,
            "external_id",
            DIAGNOSTIC_DUPLICATE_CONFLICT,
            "error",
        ),
        IdentityAction::Invalid { field, code } => diagnostic(row.number, field, code, "error"),
        IdentityAction::Accept { .. } | IdentityAction::Duplicate { .. } => diagnostic(
            row.number,
            "external_id",
            DIAGNOSTIC_DOMAIN_INVALID,
            "error",
        ),
    }
}

fn reference_error(
    row: &ParsedRow,
    field: &str,
    catalog: &HashMap<String, CatalogRef>,
) -> Option<CsvImportDiagnosticDto> {
    match check_reference(row.number, field, csv_cell(row, field), catalog) {
        Some(RowOutcome::Invalid(diagnostic)) => Some(diagnostic),
        _ => None,
    }
}

async fn commit_activity_row(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    row: &ParsedRow,
    catalog: &Catalog,
    seen_identities: &mut HashMap<(String, String), IdentityRecord>,
    seen_no_id: &mut HashSet<String>,
) -> Result<CommitRow, CsvImportDiagnosticDto> {
    let escaped = parse_escaped(row)
        .map_err(|code| diagnostic(row.number, CSV_ESCAPED_COLUMN, code, "error"))?;
    let kind = unescape(csv_cell(row, "kind"), escaped);
    if matches!(
        kind.as_str(),
        "opening_adjustment" | "reversal" | "correction"
    ) {
        return Err(diagnostic(
            row.number,
            "kind",
            DIAGNOSTIC_KIND_FORBIDDEN,
            "error",
        ));
    }
    if let Err(field) = reject_localized(
        row,
        &[
            "amount",
            "source_amount",
            "destination_amount",
            "quantity",
            "unit_price",
            "gross_amount",
            "fee_amount",
            "principal_amount",
            "cash_amount",
            "fx_rate",
        ],
    ) {
        return Err(diagnostic(
            row.number,
            field,
            DIAGNOSTIC_LOCALIZED_VALUE,
            "error",
        ));
    }
    if looks_localized_date(&unescape(csv_cell(row, "effective_local_date"), escaped)) {
        return Err(diagnostic(
            row.number,
            "effective_local_date",
            DIAGNOSTIC_LOCALIZED_VALUE,
            "error",
        ));
    }
    let input = activity_input(row, escaped)
        .map_err(|_| diagnostic(row.number, "kind", DIAGNOSTIC_DOMAIN_INVALID, "error"))?;
    for field in [
        "account_id",
        "source_account_id",
        "destination_account_id",
        "liability_account_id",
        "cash_account_id",
    ] {
        if let Some(error) = reference_error(row, field, &catalog.accounts) {
            return Err(error);
        }
    }
    if let Some(error) = reference_error(row, "holding_id", &catalog.holdings) {
        return Err(error);
    }
    if let Some(error) = reference_error(row, "instrument_id", &catalog.instruments) {
        return Err(error);
    }
    let fingerprint = activity_row_fingerprint(row, escaped)
        .map_err(|_| diagnostic(row.number, "kind", DIAGNOSTIC_DOMAIN_INVALID, "error"))?;
    let fingerprint_hex = fingerprint.checksum().hex().to_owned();
    match resolve_identity(
        row,
        fingerprint_hex.clone(),
        catalog,
        seen_identities,
        seen_no_id,
    ) {
        IdentityAction::Duplicate {
            namespace,
            external_id,
            fingerprint,
            prior,
        } => {
            remember_identity(
                seen_identities,
                Some(namespace.clone()),
                Some(external_id.clone()),
                IdentityRecord {
                    fingerprint: fingerprint.clone(),
                    ..prior.clone()
                },
            );
            duplicate_item(row, namespace, external_id, fingerprint, prior)
        }
        IdentityAction::Accept {
            namespace,
            external_id,
            fingerprint,
            warn_no_identity,
        } => {
            let posted = post_create_activity_in_tx(tx, household_id, input)
                .await
                .map_err(|_| diagnostic(row.number, "kind", DIAGNOSTIC_DOMAIN_INVALID, "error"))?;
            failpoint("activity")
                .map_err(|_| diagnostic(row.number, "kind", DIAGNOSTIC_DOMAIN_INVALID, "error"))?;
            let record = IdentityRecord {
                fingerprint: fingerprint.clone(),
                activity_id: Some(posted.id().to_string()),
                instrument_quote_id: None,
                fx_quote_id: None,
                benchmark_observation_id: None,
            };
            remember_identity(
                seen_identities,
                namespace.clone(),
                external_id.clone(),
                record.clone(),
            );
            let warning = warn_no_identity.then(|| {
                diagnostic(
                    row.number,
                    "external_id",
                    DIAGNOSTIC_NO_IDENTITY_WARNING,
                    "warning",
                )
            });
            Ok(CommitRow::Committed(
                planned_from_identity(
                    row,
                    namespace,
                    external_id,
                    fingerprint,
                    "committed",
                    warning.as_ref().map(|item| item.code.clone()),
                    &record,
                ),
                warning,
            ))
        }
        other => Err(identity_error(row, other)),
    }
}

async fn commit_quote_row(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    row: &ParsedRow,
    catalog: &Catalog,
    seen_identities: &mut HashMap<(String, String), IdentityRecord>,
    seen_no_id: &mut HashSet<String>,
) -> Result<CommitRow, CsvImportDiagnosticDto> {
    let escaped = parse_escaped(row)
        .map_err(|code| diagnostic(row.number, CSV_ESCAPED_COLUMN, code, "error"))?;
    if let Err(field) = reject_localized(row, &["unit_price", "rate"]) {
        return Err(diagnostic(
            row.number,
            field,
            DIAGNOSTIC_LOCALIZED_VALUE,
            "error",
        ));
    }
    let quote_kind = unescape(csv_cell(row, "quote_kind"), escaped);
    match quote_kind.as_str() {
        "instrument" => {
            if let Some(error) = reference_error(row, "instrument_id", &catalog.instruments) {
                return Err(error);
            }
            if UnitPrice::parse(&unescape(csv_cell(row, "unit_price"), escaped)).is_err() {
                return Err(diagnostic(
                    row.number,
                    "unit_price",
                    DIAGNOSTIC_DOMAIN_INVALID,
                    "error",
                ));
            }
        }
        "fx" => {
            let base = unescape(csv_cell(row, "base_currency"), escaped);
            let quote = unescape(csv_cell(row, "quote_currency"), escaped);
            if CurrencyCode::parse_supported(&base).is_err() {
                return Err(diagnostic(
                    row.number,
                    "base_currency",
                    DIAGNOSTIC_DOMAIN_INVALID,
                    "error",
                ));
            }
            if CurrencyCode::parse_supported(&quote).is_err() {
                return Err(diagnostic(
                    row.number,
                    "quote_currency",
                    DIAGNOSTIC_DOMAIN_INVALID,
                    "error",
                ));
            }
            if FxRate::parse(&unescape(csv_cell(row, "rate"), escaped)).is_err() {
                return Err(diagnostic(
                    row.number,
                    "rate",
                    DIAGNOSTIC_DOMAIN_INVALID,
                    "error",
                ));
            }
        }
        _ => {
            return Err(diagnostic(
                row.number,
                "quote_kind",
                DIAGNOSTIC_DOMAIN_INVALID,
                "error",
            ))
        }
    }
    let fingerprint = quote_row_fingerprint(row, escaped)
        .map_err(|_| diagnostic(row.number, "quote_kind", DIAGNOSTIC_DOMAIN_INVALID, "error"))?;
    let fingerprint_hex = fingerprint.checksum().hex().to_owned();
    match resolve_identity(row, fingerprint_hex, catalog, seen_identities, seen_no_id) {
        IdentityAction::Duplicate {
            namespace,
            external_id,
            fingerprint,
            prior,
        } => {
            remember_identity(
                seen_identities,
                Some(namespace.clone()),
                Some(external_id.clone()),
                IdentityRecord {
                    fingerprint: fingerprint.clone(),
                    ..prior.clone()
                },
            );
            duplicate_item(row, namespace, external_id, fingerprint, prior)
        }
        IdentityAction::Accept {
            namespace,
            external_id,
            fingerprint,
            warn_no_identity,
        } => {
            let quoted_at = optional_text(&unescape(csv_cell(row, "quoted_at"), escaped));
            let record = match quote_kind.as_str() {
                "instrument" => {
                    let posted = append_imported_manual_instrument_quote_in_tx(
                        tx,
                        household_id,
                        &unescape(csv_cell(row, "instrument_id"), escaped),
                        &unescape(csv_cell(row, "unit_price"), escaped),
                        quoted_at.as_deref(),
                    )
                    .await
                    .map_err(|_| {
                        diagnostic(row.number, "unit_price", DIAGNOSTIC_DOMAIN_INVALID, "error")
                    })?;
                    failpoint("quote").map_err(|_| {
                        diagnostic(row.number, "unit_price", DIAGNOSTIC_DOMAIN_INVALID, "error")
                    })?;
                    IdentityRecord {
                        fingerprint: fingerprint.clone(),
                        activity_id: None,
                        instrument_quote_id: Some(posted.id),
                        fx_quote_id: None,
                        benchmark_observation_id: None,
                    }
                }
                _ => {
                    let posted = append_imported_manual_fx_quote_in_tx(
                        tx,
                        household_id,
                        &unescape(csv_cell(row, "base_currency"), escaped),
                        &unescape(csv_cell(row, "quote_currency"), escaped),
                        &unescape(csv_cell(row, "rate"), escaped),
                        quoted_at.as_deref(),
                    )
                    .await
                    .map_err(|_| {
                        diagnostic(row.number, "rate", DIAGNOSTIC_DOMAIN_INVALID, "error")
                    })?;
                    failpoint("fx").map_err(|_| {
                        diagnostic(row.number, "rate", DIAGNOSTIC_DOMAIN_INVALID, "error")
                    })?;
                    IdentityRecord {
                        fingerprint: fingerprint.clone(),
                        activity_id: None,
                        instrument_quote_id: None,
                        fx_quote_id: Some(posted.id),
                        benchmark_observation_id: None,
                    }
                }
            };
            remember_identity(
                seen_identities,
                namespace.clone(),
                external_id.clone(),
                record.clone(),
            );
            let warning = warn_no_identity.then(|| {
                diagnostic(
                    row.number,
                    "external_id",
                    DIAGNOSTIC_NO_IDENTITY_WARNING,
                    "warning",
                )
            });
            Ok(CommitRow::Committed(
                planned_from_identity(
                    row,
                    namespace,
                    external_id,
                    fingerprint,
                    "committed",
                    warning.as_ref().map(|item| item.code.clone()),
                    &record,
                ),
                warning,
            ))
        }
        other => Err(identity_error(row, other)),
    }
}

async fn commit_benchmark_row(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    row: &ParsedRow,
    catalog: &Catalog,
    seen_identities: &mut HashMap<(String, String), IdentityRecord>,
    seen_no_id: &mut HashSet<String>,
) -> Result<CommitRow, CsvImportDiagnosticDto> {
    let escaped = parse_escaped(row)
        .map_err(|code| diagnostic(row.number, CSV_ESCAPED_COLUMN, code, "error"))?;
    if looks_localized_decimal_or_date(row, escaped) {
        return Err(diagnostic(
            row.number,
            "level",
            DIAGNOSTIC_LOCALIZED_VALUE,
            "error",
        ));
    }
    if let Some(error) = reference_error(row, "benchmark_id", &catalog.benchmarks) {
        return Err(error);
    }
    let observed_on = unescape(csv_cell(row, "observed_on"), escaped);
    let level = unescape(csv_cell(row, "level"), escaped);
    if CalendarDate::parse(&observed_on).is_err() {
        return Err(diagnostic(
            row.number,
            "observed_on",
            DIAGNOSTIC_DOMAIN_INVALID,
            "error",
        ));
    }
    let parsed_level = BenchmarkLevel::parse(&level)
        .map_err(|_| diagnostic(row.number, "level", DIAGNOSTIC_DOMAIN_INVALID, "error"))?;
    let note =
        parse_optional_note(optional_text(&unescape(csv_cell(row, "note"), escaped)).as_deref())
            .map_err(|_| diagnostic(row.number, "note", DIAGNOSTIC_DOMAIN_INVALID, "error"))?;
    let fingerprint = benchmark_row_fingerprint(row, escaped).map_err(|_| {
        diagnostic(
            row.number,
            "benchmark_id",
            DIAGNOSTIC_DOMAIN_INVALID,
            "error",
        )
    })?;
    let fingerprint_hex = fingerprint.checksum().hex().to_owned();
    match resolve_identity(row, fingerprint_hex, catalog, seen_identities, seen_no_id) {
        IdentityAction::Duplicate {
            namespace,
            external_id,
            fingerprint,
            prior,
        } => {
            remember_identity(
                seen_identities,
                Some(namespace.clone()),
                Some(external_id.clone()),
                IdentityRecord {
                    fingerprint: fingerprint.clone(),
                    ..prior.clone()
                },
            );
            duplicate_item(row, namespace, external_id, fingerprint, prior)
        }
        IdentityAction::Accept {
            namespace,
            external_id,
            fingerprint,
            warn_no_identity,
        } => {
            let benchmark_id = unescape(csv_cell(row, "benchmark_id"), escaped);
            let posted = benchmark_service::append_benchmark_observation_in_tx(
                tx,
                household_id,
                &benchmark_id,
                parsed_level.canonical().as_str(),
                &observed_on,
                note.as_deref(),
                BenchmarkObservationSourceKind::Csv,
            )
            .await
            .map_err(|_| {
                diagnostic(
                    row.number,
                    "benchmark_id",
                    DIAGNOSTIC_DOMAIN_INVALID,
                    "error",
                )
            })?;
            failpoint("benchmark").map_err(|_| {
                diagnostic(
                    row.number,
                    "benchmark_id",
                    DIAGNOSTIC_DOMAIN_INVALID,
                    "error",
                )
            })?;
            let record = IdentityRecord {
                fingerprint: fingerprint.clone(),
                activity_id: None,
                instrument_quote_id: None,
                fx_quote_id: None,
                benchmark_observation_id: Some(posted.id),
            };
            remember_identity(
                seen_identities,
                namespace.clone(),
                external_id.clone(),
                record.clone(),
            );
            let warning = warn_no_identity.then(|| {
                diagnostic(
                    row.number,
                    "external_id",
                    DIAGNOSTIC_NO_IDENTITY_WARNING,
                    "warning",
                )
            });
            Ok(CommitRow::Committed(
                planned_from_identity(
                    row,
                    namespace,
                    external_id,
                    fingerprint,
                    "committed",
                    warning.as_ref().map(|item| item.code.clone()),
                    &record,
                ),
                warning,
            ))
        }
        other => Err(identity_error(row, other)),
    }
}

fn looks_localized_decimal_or_date(row: &ParsedRow, escaped: bool) -> bool {
    looks_localized_decimal(&unescape(csv_cell(row, "level"), escaped))
        || looks_localized_date(&unescape(csv_cell(row, "observed_on"), escaped))
}

pub async fn list_import_batches(
    state: &AppState,
    input: ListImportBatchesInput,
) -> Result<ImportBatchPageDto, AppError> {
    let limit = page_size(input.limit)?;
    let cursor = input
        .cursor
        .as_deref()
        .map(decode_batch_cursor)
        .transpose()?;
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let mut rows = sustainable_repositories::list_import_batches_page(
            &mut tx,
            &household.id,
            cursor
                .as_ref()
                .map(|(created_at, id)| (created_at.as_str(), id.as_str())),
            limit + 1,
        )
        .await?;
        let has_more = i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit;
        if has_more {
            rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        }
        let next_cursor = has_more
            .then(|| rows.last().map(encode_batch_cursor))
            .flatten();
        Ok(ImportBatchPageDto {
            items: rows.iter().map(batch_dto).collect(),
            next_cursor,
        })
    }
    .await;
    finish_read_tx(tx, result).await
}

pub async fn get_import_batch(
    state: &AppState,
    input: GetImportBatchInput,
) -> Result<ImportBatchDetailDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let batch = sustainable_repositories::get_import_batch(&mut tx, &household.id, &input.id)
            .await?
            .ok_or_else(|| AppError::not_found("importBatch", &input.id))?;
        let items = sustainable_repositories::list_import_items(&mut tx, &batch.id).await?;
        Ok(ImportBatchDetailDto {
            batch: batch_dto(&batch),
            items: items.into_iter().map(item_dto).collect(),
        })
    }
    .await;
    finish_read_tx(tx, result).await
}

fn batch_dto(row: &ImportBatchRecord) -> ImportBatchDto {
    ImportBatchDto {
        id: row.id.clone(),
        template: row.template.clone(),
        file_sha256: row.file_sha256.clone(),
        source_namespace: row.source_namespace.clone(),
        row_count: i32::try_from(row.row_count).unwrap_or(i32::MAX),
        committed_count: i32::try_from(row.committed_count).unwrap_or(i32::MAX),
        duplicate_count: i32::try_from(row.duplicate_count).unwrap_or(i32::MAX),
        rejected_count: i32::try_from(row.rejected_count).unwrap_or(i32::MAX),
        status: row.status.clone(),
        created_at: row.created_at.clone(),
        completed_at: row.completed_at.clone(),
    }
}

fn item_dto(row: ImportItemRecord) -> ImportItemDto {
    ImportItemDto {
        id: row.id,
        row_number: i32::try_from(row.row_number).unwrap_or(i32::MAX),
        source_namespace: row.source_namespace,
        external_id: row.external_id,
        fingerprint: row.fingerprint,
        outcome: row.outcome,
        diagnostic_code: row.diagnostic_code,
        activity_id: row.activity_id,
        instrument_quote_id: row.instrument_quote_id,
        fx_quote_id: row.fx_quote_id,
        benchmark_observation_id: row.benchmark_observation_id,
    }
}

fn page_size(limit: Option<i32>) -> Result<i64, AppError> {
    let limit = i64::from(limit.unwrap_or(DEFAULT_PAGE_SIZE as i32));
    if !(1..=MAX_PAGE_SIZE).contains(&limit) {
        return Err(AppError::validation(
            "limit",
            "The page size must be between 1 and 100.",
        ));
    }
    Ok(limit)
}

fn encode_batch_cursor(row: &ImportBatchRecord) -> String {
    URL_SAFE_NO_PAD.encode(format!("{}\n{}", row.created_at, row.id))
}

fn decode_batch_cursor(value: &str) -> Result<(String, String), AppError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| AppError::validation("cursor", "The import batch cursor is invalid."))?;
    let text = String::from_utf8(decoded)
        .map_err(|_| AppError::validation("cursor", "The import batch cursor is invalid."))?;
    let mut parts = text.splitn(2, '\n');
    let created_at = parts
        .next()
        .ok_or_else(|| AppError::validation("cursor", "The import batch cursor is invalid."))?;
    let id = parts
        .next()
        .ok_or_else(|| AppError::validation("cursor", "The import batch cursor is invalid."))?;
    if created_at.is_empty() || id.is_empty() {
        return Err(AppError::validation(
            "cursor",
            "The import batch cursor is invalid.",
        ));
    }
    Ok((created_at.to_owned(), id.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            account_service::{self, CreateAccountInput, OwnershipShareInput},
            analytics_query_service::{
                self, AnalyticsScopeDto, DeclareLotCostBasisInput, ListUnknownBasisLotsInput,
            },
            cash_service::{self, AppendAccountCashInput},
            csv_preview_service::{preview_csv_import, PreviewCsvImportInput},
            history_query_service::{
                confirm_history_timezone, get_history_origin, ConfirmHistoryTimezoneInput,
            },
            holding_service::{self, CreateHoldingInput},
            instrument_service::{self, CreateInstrumentInput},
            onboarding_service::complete_onboarding,
            quote_service::{self, SetInstrumentQuotePreferenceInput},
        },
        domain::{
            ACTIVITY_CSV_HEADERS, BENCHMARK_CSV_HEADERS, DIAGNOSTIC_KIND_FORBIDDEN,
            QUOTE_CSV_HEADERS,
        },
        error::AppError,
        test_support::{
            blocked_future_state, stable_sqlite_hash, test_path, valid_onboarding_input,
        },
    };
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    fn cleanup(path: &Path) {
        let _ = fs::remove_file(path);
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = path.as_os_str().to_os_string();
            sidecar.push(suffix);
            let _ = fs::remove_file(PathBuf::from(sidecar));
        }
    }

    fn activity_csv(rows: &[&str]) -> String {
        let mut out = String::from("# nestworth-csv:activity:v1\r\n");
        out.push_str(&ACTIVITY_CSV_HEADERS.join(","));
        out.push_str("\r\n");
        for row in rows {
            out.push_str(row);
            out.push_str("\r\n");
        }
        out
    }

    fn quote_csv(rows: &[&str]) -> String {
        let mut out = String::from("# nestworth-csv:quote:v1\r\n");
        out.push_str(&QUOTE_CSV_HEADERS.join(","));
        out.push_str("\r\n");
        for row in rows {
            out.push_str(row);
            out.push_str("\r\n");
        }
        out
    }

    fn benchmark_csv(rows: &[&str]) -> String {
        let mut out = String::from("# nestworth-csv:benchmark:v1\r\n");
        out.push_str(&BENCHMARK_CSV_HEADERS.join(","));
        out.push_str("\r\n");
        for row in rows {
            out.push_str(row);
            out.push_str("\r\n");
        }
        out
    }

    fn empty_activity_cells() -> Vec<String> {
        vec![String::new(); ACTIVITY_CSV_HEADERS.len()]
    }

    fn empty_quote_cells() -> Vec<String> {
        vec![String::new(); QUOTE_CSV_HEADERS.len()]
    }

    fn empty_benchmark_cells() -> Vec<String> {
        vec![String::new(); BENCHMARK_CSV_HEADERS.len()]
    }

    fn set_cell(headers: &[&str], values: &mut [String], header: &str, value: &str) {
        if let Some(index) = headers.iter().position(|name| *name == header) {
            values[index] = value.to_owned();
        }
    }

    fn encode_row(values: &[String]) -> String {
        let mut writer = csv::WriterBuilder::new()
            .terminator(csv::Terminator::CRLF)
            .from_writer(Vec::new());
        writer.write_record(values).expect("record");
        writer.flush().expect("flush");
        let bytes = writer.into_inner().expect("bytes");
        let text = String::from_utf8(bytes).expect("utf8");
        text.trim_end_matches(['\r', '\n']).to_owned()
    }

    async fn onboarded(name: &str) -> (AppState, PathBuf, String, String) {
        let root = test_path("phase8-import", name);
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
            .expect("confirm");
        }
        sqlx::query("UPDATE history_origins SET origin_at = '2000-01-01T00:00:00.000Z'")
            .execute(state.writable_db().expect("db"))
            .await
            .expect("origin start");
        let member_id: String = sqlx::query_scalar("SELECT id FROM members LIMIT 1")
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("member");
        let account = account_service::create_account(
            &state,
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
        .expect("account");
        (state, root, account.id, origin.origin_local_date)
    }

    fn deposit_row(
        account_id: &str,
        date: &str,
        external_id: &str,
        amount: &str,
        note: &str,
    ) -> String {
        let mut values = empty_activity_cells();
        set_cell(
            ACTIVITY_CSV_HEADERS,
            &mut values,
            "source_namespace",
            "acct.example",
        );
        set_cell(
            ACTIVITY_CSV_HEADERS,
            &mut values,
            "external_id",
            external_id,
        );
        set_cell(ACTIVITY_CSV_HEADERS, &mut values, "kind", "deposit");
        set_cell(
            ACTIVITY_CSV_HEADERS,
            &mut values,
            "effective_local_date",
            date,
        );
        set_cell(
            ACTIVITY_CSV_HEADERS,
            &mut values,
            "effective_local_time",
            "09:00",
        );
        set_cell(ACTIVITY_CSV_HEADERS, &mut values, "account_id", account_id);
        set_cell(
            ACTIVITY_CSV_HEADERS,
            &mut values,
            "component_kind",
            "account_value",
        );
        set_cell(ACTIVITY_CSV_HEADERS, &mut values, "amount", amount);
        set_cell(ACTIVITY_CSV_HEADERS, &mut values, "currency", "CNY");
        set_cell(ACTIVITY_CSV_HEADERS, &mut values, "note", note);
        set_cell(
            ACTIVITY_CSV_HEADERS,
            &mut values,
            "escaped_for_spreadsheet",
            "false",
        );
        encode_row(&values)
    }

    fn kind_row(account_id: &str, date: &str, kind: &str) -> String {
        let mut values = empty_activity_cells();
        set_cell(ACTIVITY_CSV_HEADERS, &mut values, "kind", kind);
        set_cell(
            ACTIVITY_CSV_HEADERS,
            &mut values,
            "effective_local_date",
            date,
        );
        set_cell(
            ACTIVITY_CSV_HEADERS,
            &mut values,
            "effective_local_time",
            "09:00",
        );
        set_cell(ACTIVITY_CSV_HEADERS, &mut values, "account_id", account_id);
        set_cell(
            ACTIVITY_CSV_HEADERS,
            &mut values,
            "component_kind",
            "account_value",
        );
        set_cell(ACTIVITY_CSV_HEADERS, &mut values, "amount", "1");
        set_cell(ACTIVITY_CSV_HEADERS, &mut values, "currency", "CNY");
        set_cell(
            ACTIVITY_CSV_HEADERS,
            &mut values,
            "escaped_for_spreadsheet",
            "false",
        );
        encode_row(&values)
    }

    async fn count(state: &AppState, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("count")
    }

    async fn preview_and_commit(
        state: &AppState,
        path: &Path,
        confirmed: bool,
    ) -> Result<CsvImportCommitDto, AppError> {
        let preview = preview_csv_import(
            state,
            PreviewCsvImportInput {
                source_path: path.to_string_lossy().into_owned(),
            },
        )
        .await?;
        commit_csv_import(
            state,
            CommitCsvImportInput {
                preview_token: preview.preview_token,
                confirmed,
            },
        )
        .await
    }

    fn write_csv(root: &Path, name: &str, contents: &str) -> PathBuf {
        let path = root.with_extension(name);
        fs::write(&path, contents).expect("write csv");
        path
    }

    #[test]
    fn commit_fails_when_file_changes_after_preview() {
        tauri::async_runtime::block_on(async {
            let (state, root, account_id, date) = onboarded("file-changed").await;
            let csv_path = write_csv(
                &root,
                "changed.csv",
                &activity_csv(&[&deposit_row(
                    &account_id,
                    &date,
                    "salary-1",
                    "5000",
                    "January",
                )]),
            );
            let preview = preview_csv_import(
                &state,
                PreviewCsvImportInput {
                    source_path: csv_path.to_string_lossy().into_owned(),
                },
            )
            .await
            .expect("preview");
            fs::write(&csv_path, b"changed").expect("mutate");
            let error = commit_csv_import(
                &state,
                CommitCsvImportInput {
                    preview_token: preview.preview_token,
                    confirmed: true,
                },
            )
            .await
            .expect_err("changed file");
            assert!(matches!(error, AppError::ImportFileChanged));
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM import_batches").await,
                0
            );
            drop(state);
            let _ = fs::remove_file(csv_path);
            cleanup(&root);
        });
    }

    #[test]
    fn invalid_non_duplicate_row_writes_nothing() {
        tauri::async_runtime::block_on(async {
            let (state, root, account_id, date) = onboarded("invalid-row").await;
            let activities_before = count(&state, "SELECT COUNT(*) FROM activities").await;
            let csv_path = write_csv(
                &root,
                "invalid.csv",
                &activity_csv(&[
                    &deposit_row(&account_id, &date, "ok-1", "100", "ok"),
                    &deposit_row(&account_id, &date, "bad-1", "not-a-number", "bad"),
                ]),
            );
            let error = preview_and_commit(&state, &csv_path, true)
                .await
                .expect_err("invalid");
            assert!(matches!(
                error,
                AppError::ImportRejected { .. } | AppError::InvalidImportRow { .. }
            ));
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                activities_before
            );
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM import_batches").await,
                0
            );
            assert_eq!(count(&state, "SELECT COUNT(*) FROM import_items").await, 0);
            drop(state);
            let _ = fs::remove_file(csv_path);
            cleanup(&root);
        });
    }

    #[test]
    fn exact_duplicate_skips_and_conflict_rejects() {
        tauri::async_runtime::block_on(async {
            let (state, root, account_id, date) = onboarded("duplicate-conflict").await;
            let csv_path = write_csv(
                &root,
                "dup.csv",
                &activity_csv(&[&deposit_row(
                    &account_id,
                    &date,
                    "salary-1",
                    "5000",
                    "January",
                )]),
            );
            let first = preview_and_commit(&state, &csv_path, true)
                .await
                .expect("first commit");
            assert_eq!(first.committed_count, 1);
            let activities_after_first = count(&state, "SELECT COUNT(*) FROM activities").await;
            let second = preview_and_commit(&state, &csv_path, true)
                .await
                .expect("reimport");
            assert_eq!(second.committed_count, 0);
            assert_eq!(second.duplicate_count, 1);
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                activities_after_first
            );
            let conflict_path = write_csv(
                &root,
                "conflict.csv",
                &activity_csv(&[&deposit_row(
                    &account_id,
                    &date,
                    "salary-1",
                    "8000",
                    "changed",
                )]),
            );
            let error = preview_and_commit(&state, &conflict_path, true)
                .await
                .expect_err("conflict");
            match error {
                AppError::ImportRejected { code, .. } => {
                    assert_eq!(code, DIAGNOSTIC_DUPLICATE_CONFLICT);
                }
                other => panic!("expected conflict, got {other:?}"),
            }
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                activities_after_first
            );
            drop(state);
            let _ = fs::remove_file(csv_path);
            let _ = fs::remove_file(conflict_path);
            cleanup(&root);
        });
    }

    #[test]
    fn no_external_id_rows_warn_and_are_not_guessed_duplicates() {
        tauri::async_runtime::block_on(async {
            let (state, root, account_id, date) = onboarded("no-id").await;
            let mut first = empty_activity_cells();
            set_cell(ACTIVITY_CSV_HEADERS, &mut first, "kind", "deposit");
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut first,
                "effective_local_date",
                &date,
            );
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut first,
                "effective_local_time",
                "09:00",
            );
            set_cell(ACTIVITY_CSV_HEADERS, &mut first, "account_id", &account_id);
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut first,
                "component_kind",
                "account_value",
            );
            set_cell(ACTIVITY_CSV_HEADERS, &mut first, "amount", "50");
            set_cell(ACTIVITY_CSV_HEADERS, &mut first, "currency", "CNY");
            set_cell(ACTIVITY_CSV_HEADERS, &mut first, "note", "same looking");
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut first,
                "escaped_for_spreadsheet",
                "false",
            );
            let row = encode_row(&first);
            let csv_path = write_csv(&root, "noid.csv", &activity_csv(&[&row, &row]));
            let committed = preview_and_commit(&state, &csv_path, true)
                .await
                .expect("commit");
            assert_eq!(committed.committed_count, 2);
            assert!(committed
                .diagnostics
                .iter()
                .any(|item| item.code == DIAGNOSTIC_NO_IDENTITY_WARNING));
            drop(state);
            let _ = fs::remove_file(csv_path);
            cleanup(&root);
        });
    }

    #[test]
    fn forbidden_future_pre_origin_and_raw_leg_activity_rows_are_rejected() {
        tauri::async_runtime::block_on(async {
            let (state, root, account_id, date) = onboarded("forbidden").await;
            assert!(!ACTIVITY_CSV_HEADERS.contains(&"legs"));
            assert!(!ACTIVITY_CSV_HEADERS.contains(&"raw_legs"));
            let activities_before = count(&state, "SELECT COUNT(*) FROM activities").await;
            for kind in ["opening_adjustment", "reversal", "correction"] {
                let csv_path = write_csv(
                    &root,
                    &format!("{kind}.csv"),
                    &activity_csv(&[&kind_row(&account_id, &date, kind)]),
                );
                let error = preview_and_commit(&state, &csv_path, true)
                    .await
                    .expect_err(kind);
                match error {
                    AppError::ImportRejected { code, .. } => {
                        assert_eq!(code, DIAGNOSTIC_KIND_FORBIDDEN);
                    }
                    other => panic!("{kind} expected forbidden, got {other:?}"),
                }
                let _ = fs::remove_file(csv_path);
            }
            let future = write_csv(
                &root,
                "future.csv",
                &activity_csv(&[&deposit_row(
                    &account_id,
                    "2099-01-01",
                    "future-1",
                    "10",
                    "future",
                )]),
            );
            assert!(preview_and_commit(&state, &future, true).await.is_err());
            let pre = write_csv(
                &root,
                "pre.csv",
                &activity_csv(&[&deposit_row(
                    &account_id,
                    "1999-01-01",
                    "pre-1",
                    "10",
                    "pre",
                )]),
            );
            assert!(preview_and_commit(&state, &pre, true).await.is_err());
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                activities_before
            );
            drop(state);
            let _ = fs::remove_file(future);
            let _ = fs::remove_file(pre);
            cleanup(&root);
        });
    }

    #[test]
    fn sequential_activity_rows_validate_against_earlier_accepted_state() {
        tauri::async_runtime::block_on(async {
            let (state, root, _account_id, date) = onboarded("sequential").await;
            let member_id: String = sqlx::query_scalar("SELECT id FROM members LIMIT 1")
                .fetch_one(state.writable_db().expect("db"))
                .await
                .expect("member");
            let empty = account_service::create_account(
                &state,
                CreateAccountInput {
                    name: "Empty".to_owned(),
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
                    initial_amount: Some("0".to_owned()),
                },
            )
            .await
            .expect("empty account");
            sqlx::query("UPDATE account_values SET effective_at = '2000-01-01T00:00:00.000Z' WHERE account_id = ?")
                .bind(&empty.id)
                .execute(state.writable_db().expect("db"))
                .await
                .expect("backdate opening value");
            let mut withdrawal = empty_activity_cells();
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut withdrawal,
                "source_namespace",
                "acct.example",
            );
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut withdrawal,
                "external_id",
                "out-1",
            );
            set_cell(ACTIVITY_CSV_HEADERS, &mut withdrawal, "kind", "withdrawal");
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut withdrawal,
                "effective_local_date",
                &date,
            );
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut withdrawal,
                "effective_local_time",
                "10:00",
            );
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut withdrawal,
                "account_id",
                &empty.id,
            );
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut withdrawal,
                "component_kind",
                "account_value",
            );
            set_cell(ACTIVITY_CSV_HEADERS, &mut withdrawal, "amount", "4000");
            set_cell(ACTIVITY_CSV_HEADERS, &mut withdrawal, "currency", "CNY");
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut withdrawal,
                "escaped_for_spreadsheet",
                "false",
            );
            let csv_path = write_csv(
                &root,
                "seq.csv",
                &activity_csv(&[
                    &encode_row(&withdrawal),
                    &deposit_row(&empty.id, &date, "in-1", "5000", "fund first"),
                ]),
            );
            let committed = preview_and_commit(&state, &csv_path, true)
                .await
                .expect("sequential commit");
            assert_eq!(committed.committed_count, 2);
            drop(state);
            let _ = fs::remove_file(csv_path);
            cleanup(&root);
        });
    }

    #[test]
    fn persistence_failpoints_roll_back_business_rows_and_provenance() {
        tauri::async_runtime::block_on(async {
            let (state, root, account_id, date) = onboarded("failpoints").await;
            let activities_before = count(&state, "SELECT COUNT(*) FROM activities").await;
            let csv_path = write_csv(
                &root,
                "fail.csv",
                &activity_csv(&[&deposit_row(&account_id, &date, "fail-1", "25", "fail")]),
            );
            for point in ["activity", "batch", "item"] {
                set_import_failpoint(Some(point));
                let error = preview_and_commit(&state, &csv_path, true)
                    .await
                    .expect_err(point);
                set_import_failpoint(None);
                assert!(
                    matches!(
                        error,
                        AppError::ImportCommitFailed { .. } | AppError::ImportRejected { .. }
                    ),
                    "{point}: {error:?}"
                );
                assert_eq!(
                    count(&state, "SELECT COUNT(*) FROM activities").await,
                    activities_before,
                    "{point} left activities"
                );
                assert_eq!(
                    count(&state, "SELECT COUNT(*) FROM import_batches").await,
                    0,
                    "{point} left batches"
                );
            }
            drop(state);
            let _ = fs::remove_file(csv_path);
            cleanup(&root);
        });
    }

    #[test]
    fn imported_quotes_are_manual_append_only_and_leave_preferences() {
        tauri::async_runtime::block_on(async {
            let (state, root, _, date) = onboarded("quotes").await;
            let member_id: String = sqlx::query_scalar("SELECT id FROM members LIMIT 1")
                .fetch_one(state.writable_db().expect("db"))
                .await
                .expect("member");
            let instrument = instrument_service::create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "QQQ".to_owned(),
                    symbol: Some("QQQ".to_owned()),
                    instrument_type: "etf".to_owned(),
                    quote_currency: "USD".to_owned(),
                    market_code: None,
                    country_code: Some("US".to_owned()),
                    isin: None,
                    provider_key: None,
                    provider_symbol: None,
                    quote_preference: Some("provider".to_owned()),
                    note: None,
                },
            )
            .await
            .expect("instrument");
            quote_service::set_instrument_quote_preference(
                &state,
                SetInstrumentQuotePreferenceInput {
                    instrument_id: instrument.id.clone(),
                    quote_preference: "provider".to_owned(),
                },
            )
            .await
            .ok();
            let preference_before: String =
                sqlx::query_scalar("SELECT quote_preference FROM instruments WHERE id = ?")
                    .bind(&instrument.id)
                    .fetch_one(state.writable_db().expect("db"))
                    .await
                    .expect("preference");
            let fx_pref_before = count(&state, "SELECT COUNT(*) FROM fx_quote_preferences").await;
            let mut instrument_row = empty_quote_cells();
            set_cell(
                QUOTE_CSV_HEADERS,
                &mut instrument_row,
                "source_namespace",
                "quotes.example",
            );
            set_cell(
                QUOTE_CSV_HEADERS,
                &mut instrument_row,
                "external_id",
                "qqq-1",
            );
            set_cell(
                QUOTE_CSV_HEADERS,
                &mut instrument_row,
                "quote_kind",
                "instrument",
            );
            set_cell(
                QUOTE_CSV_HEADERS,
                &mut instrument_row,
                "instrument_id",
                &instrument.id,
            );
            set_cell(
                QUOTE_CSV_HEADERS,
                &mut instrument_row,
                "unit_price",
                "100.5",
            );
            set_cell(
                QUOTE_CSV_HEADERS,
                &mut instrument_row,
                "quoted_at",
                &format!("{date}T00:00:00.000Z"),
            );
            set_cell(
                QUOTE_CSV_HEADERS,
                &mut instrument_row,
                "escaped_for_spreadsheet",
                "false",
            );
            let mut fx_row = empty_quote_cells();
            set_cell(
                QUOTE_CSV_HEADERS,
                &mut fx_row,
                "source_namespace",
                "quotes.example",
            );
            set_cell(QUOTE_CSV_HEADERS, &mut fx_row, "external_id", "usd-cny-1");
            set_cell(QUOTE_CSV_HEADERS, &mut fx_row, "quote_kind", "fx");
            set_cell(QUOTE_CSV_HEADERS, &mut fx_row, "base_currency", "USD");
            set_cell(QUOTE_CSV_HEADERS, &mut fx_row, "quote_currency", "CNY");
            set_cell(QUOTE_CSV_HEADERS, &mut fx_row, "rate", "7.1");
            set_cell(
                QUOTE_CSV_HEADERS,
                &mut fx_row,
                "quoted_at",
                &format!("{date}T00:00:00.000Z"),
            );
            set_cell(
                QUOTE_CSV_HEADERS,
                &mut fx_row,
                "escaped_for_spreadsheet",
                "false",
            );
            let csv_path = write_csv(
                &root,
                "quotes.csv",
                &quote_csv(&[&encode_row(&instrument_row), &encode_row(&fx_row)]),
            );
            let committed = preview_and_commit(&state, &csv_path, true)
                .await
                .expect("quotes");
            assert_eq!(committed.committed_count, 2);
            let preference_after: String =
                sqlx::query_scalar("SELECT quote_preference FROM instruments WHERE id = ?")
                    .bind(&instrument.id)
                    .fetch_one(state.writable_db().expect("db"))
                    .await
                    .expect("preference after");
            assert_eq!(preference_before, preference_after);
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM fx_quote_preferences").await,
                fx_pref_before
            );
            let source: String =
                sqlx::query_scalar("SELECT source_kind FROM instrument_quotes LIMIT 1")
                    .fetch_one(state.writable_db().expect("db"))
                    .await
                    .expect("source");
            assert_eq!(source, "manual");
            let _ = member_id;
            drop(state);
            let _ = fs::remove_file(csv_path);
            cleanup(&root);
        });
    }

    #[test]
    fn cost_basis_declarations_are_unchanged_by_import() {
        tauri::async_runtime::block_on(async {
            let (state, root, _, date) = onboarded("basis").await;
            let member_id: String = sqlx::query_scalar("SELECT id FROM members LIMIT 1")
                .fetch_one(state.writable_db().expect("db"))
                .await
                .expect("member");
            let brokerage = account_service::create_account(
                &state,
                CreateAccountInput {
                    name: "Brokerage".to_owned(),
                    primary_category: "investment".to_owned(),
                    secondary_category: "brokerage_account".to_owned(),
                    default_currency: "USD".to_owned(),
                    institution_id: None,
                    group_id: None,
                    tracking_mode: Some("holdings".to_owned()),
                    note: None,
                    include_in_net_worth: true,
                    include_in_investment: true,
                    include_in_liquid_assets: false,
                    opened_on: None,
                    closed_on: None,
                    owners: vec![OwnershipShareInput {
                        member_id,
                        percent: Some("100".to_owned()),
                        share_bps: None,
                    }],
                    initial_amount: None,
                },
            )
            .await
            .expect("brokerage");
            let instrument = instrument_service::create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "QQQ".to_owned(),
                    symbol: Some("QQQ".to_owned()),
                    instrument_type: "etf".to_owned(),
                    quote_currency: "USD".to_owned(),
                    market_code: None,
                    country_code: Some("US".to_owned()),
                    isin: None,
                    provider_key: None,
                    provider_symbol: None,
                    quote_preference: Some("manual".to_owned()),
                    note: None,
                },
            )
            .await
            .expect("instrument");
            let holding = holding_service::create_holding(
                &state,
                CreateHoldingInput {
                    account_id: brokerage.id.clone(),
                    instrument_id: instrument.id.clone(),
                    quantity: "10".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("holding");
            cash_service::append_account_cash(
                &state,
                AppendAccountCashInput {
                    account_id: brokerage.id.clone(),
                    amount: "1000".to_owned(),
                    currency: "USD".to_owned(),
                },
            )
            .await
            .expect("cash");
            let unknown = analytics_query_service::list_unknown_basis_lots(
                &state,
                ListUnknownBasisLotsInput {
                    scope: AnalyticsScopeDto::Holding {
                        account_id: brokerage.id.clone(),
                        instrument_id: instrument.id.clone(),
                    },
                    cursor: None,
                    limit: Some(10),
                },
            )
            .await
            .expect("unknown lots");
            let lot = unknown
                .items
                .into_iter()
                .next()
                .expect("created holding has an unknown-basis lot");
            analytics_query_service::declare_lot_cost_basis(
                &state,
                DeclareLotCostBasisInput {
                    lot_ref: lot.lot_ref,
                    instrument_id: instrument.id.clone(),
                    declared_cost: "1500".to_owned(),
                    declared_currency: "USD".to_owned(),
                    acquired_on: None,
                    note: None,
                },
            )
            .await
            .expect("declare");
            let declarations_before: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
                "SELECT id, origin_holding_id, declared_cost FROM cost_basis_declarations ORDER BY id",
            )
            .fetch_all(state.writable_db().expect("db"))
            .await
            .expect("declarations");
            let mut buy = empty_activity_cells();
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut buy,
                "source_namespace",
                "broker.example",
            );
            set_cell(ACTIVITY_CSV_HEADERS, &mut buy, "external_id", "buy-1");
            set_cell(ACTIVITY_CSV_HEADERS, &mut buy, "kind", "buy");
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut buy,
                "effective_local_date",
                &date,
            );
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut buy,
                "effective_local_time",
                "11:00",
            );
            set_cell(ACTIVITY_CSV_HEADERS, &mut buy, "holding_id", &holding.id);
            set_cell(ACTIVITY_CSV_HEADERS, &mut buy, "quantity", "1");
            set_cell(ACTIVITY_CSV_HEADERS, &mut buy, "unit_price", "10");
            set_cell(ACTIVITY_CSV_HEADERS, &mut buy, "gross_amount", "10");
            set_cell(ACTIVITY_CSV_HEADERS, &mut buy, "settlement_currency", "USD");
            set_cell(
                ACTIVITY_CSV_HEADERS,
                &mut buy,
                "escaped_for_spreadsheet",
                "false",
            );
            let csv_path = write_csv(&root, "buy.csv", &activity_csv(&[&encode_row(&buy)]));
            let committed = preview_and_commit(&state, &csv_path, true)
                .await
                .expect("buy import");
            assert_eq!(committed.committed_count, 1);
            let declarations_after: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
                "SELECT id, origin_holding_id, declared_cost FROM cost_basis_declarations ORDER BY id",
            )
            .fetch_all(state.writable_db().expect("db"))
            .await
            .expect("declarations after");
            assert_eq!(declarations_before, declarations_after);
            drop(state);
            let _ = fs::remove_file(csv_path);
            cleanup(&root);
        });
    }

    #[test]
    fn import_provenance_cannot_update_or_delete_its_target() {
        tauri::async_runtime::block_on(async {
            let (state, root, account_id, date) = onboarded("provenance").await;
            let csv_path = write_csv(
                &root,
                "prov.csv",
                &activity_csv(&[&deposit_row(&account_id, &date, "prov-1", "30", "keep")]),
            );
            let first = preview_and_commit(&state, &csv_path, true)
                .await
                .expect("commit");
            let detail = get_import_batch(
                &state,
                GetImportBatchInput {
                    id: first.batch_id.clone(),
                },
            )
            .await
            .expect("detail");
            let activity_id = detail.items[0].activity_id.clone().expect("activity link");
            let note_before: String =
                sqlx::query_scalar("SELECT note FROM activities WHERE id = ?")
                    .bind(&activity_id)
                    .fetch_one(state.writable_db().expect("db"))
                    .await
                    .expect("note");
            let _ = preview_and_commit(&state, &csv_path, true)
                .await
                .expect("reimport");
            let note_after: String = sqlx::query_scalar("SELECT note FROM activities WHERE id = ?")
                .bind(&activity_id)
                .fetch_one(state.writable_db().expect("db"))
                .await
                .expect("note after");
            assert_eq!(note_before, note_after);
            let still: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities WHERE id = ?")
                .bind(&activity_id)
                .fetch_one(state.writable_db().expect("db"))
                .await
                .expect("exists");
            assert_eq!(still, 1);
            let listed = list_import_batches(
                &state,
                ListImportBatchesInput {
                    cursor: None,
                    limit: Some(1),
                },
            )
            .await
            .expect("list");
            assert_eq!(listed.items.len(), 1);
            assert!(listed.next_cursor.is_some());
            let _ = note_before;
            drop(state);
            let _ = fs::remove_file(csv_path);
            cleanup(&root);
        });
    }

    #[test]
    fn unsupported_future_database_receives_zero_import_writes() {
        tauri::async_runtime::block_on(async {
            let (state, path, before) = blocked_future_state("phase8-import-future").await;
            let error = commit_csv_import(
                &state,
                CommitCsvImportInput {
                    preview_token: "missing".to_owned(),
                    confirmed: true,
                },
            )
            .await
            .expect_err("future");
            assert!(matches!(error, AppError::UnsupportedNewerDatabase { .. }));
            let listed = list_import_batches(
                &state,
                ListImportBatchesInput {
                    cursor: None,
                    limit: None,
                },
            )
            .await
            .expect_err("list future");
            assert!(matches!(listed, AppError::UnsupportedNewerDatabase { .. }));
            assert_eq!(stable_sqlite_hash(&path).await, before);
            drop(state);
            cleanup(&path);
        });
    }

    #[test]
    fn benchmark_import_appends_csv_observations() {
        tauri::async_runtime::block_on(async {
            let (state, root, _, date) = onboarded("benchmark").await;
            let household_id: String = sqlx::query_scalar("SELECT id FROM households LIMIT 1")
                .fetch_one(state.writable_db().expect("db"))
                .await
                .expect("household");
            let mut tx = begin_write_tx(state.writable_db().expect("db"))
                .await
                .expect("tx");
            let benchmark_id = crate::domain::BenchmarkId::new().to_string();
            sustainable_repositories::insert_benchmark(
                &mut tx,
                &sustainable_repositories::BenchmarkRecord {
                    id: benchmark_id.clone(),
                    household_id,
                    name: "Fixture".to_owned(),
                    currency: "CNY".to_owned(),
                    series_kind: "price_return".to_owned(),
                    max_carry_days: 7,
                    archived_at: None,
                    created_at: Timestamp::now().to_rfc3339(),
                    updated_at: Timestamp::now().to_rfc3339(),
                },
            )
            .await
            .expect("benchmark");
            finish_write_tx(tx, Ok(())).await.expect("commit bench");
            let mut row = empty_benchmark_cells();
            set_cell(
                BENCHMARK_CSV_HEADERS,
                &mut row,
                "source_namespace",
                "bench.example",
            );
            set_cell(BENCHMARK_CSV_HEADERS, &mut row, "external_id", "obs-1");
            set_cell(
                BENCHMARK_CSV_HEADERS,
                &mut row,
                "benchmark_id",
                &benchmark_id,
            );
            set_cell(BENCHMARK_CSV_HEADERS, &mut row, "observed_on", &date);
            set_cell(BENCHMARK_CSV_HEADERS, &mut row, "level", "100.25");
            set_cell(
                BENCHMARK_CSV_HEADERS,
                &mut row,
                "escaped_for_spreadsheet",
                "false",
            );
            let csv_path = write_csv(&root, "bench.csv", &benchmark_csv(&[&encode_row(&row)]));
            let committed = preview_and_commit(&state, &csv_path, true)
                .await
                .expect("benchmark import");
            assert_eq!(committed.committed_count, 1);
            let source: String =
                sqlx::query_scalar("SELECT source_kind FROM benchmark_observations")
                    .fetch_one(state.writable_db().expect("db"))
                    .await
                    .expect("source");
            assert_eq!(source, "csv");
            drop(state);
            let _ = fs::remove_file(csv_path);
            cleanup(&root);
        });
    }
}
