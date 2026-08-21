use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Cursor, Read},
    path::PathBuf,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};

use super::{
    backup_service::{file_metadata, same_file_metadata, BACKUP_EXTENSION},
    export_service::hash_bytes,
    history_query_service::{preview_create_activity_in_tx, CreateActivityInput},
    query_count,
    reference::{begin_read_tx, finish_read_tx, map_read_error, require_household_tx},
};
use crate::{
    domain::{
        activity_fingerprint, benchmark_fingerprint, looks_localized_boolean, looks_localized_date,
        looks_localized_decimal, optional_text, parse_optional_external_id,
        parse_optional_namespace, parse_strict_boolean, quote_fingerprint,
        unescape_spreadsheet_text, BenchmarkLevel, CalendarDate, CurrencyCode, FxRate, ImportField,
        ImportFingerprint, ImportTemplate, UnitPrice, CSV_ESCAPED_COLUMN,
        DIAGNOSTIC_DOMAIN_INVALID, DIAGNOSTIC_DUPLICATE_CONFLICT, DIAGNOSTIC_EXACT_DUPLICATE,
        DIAGNOSTIC_KIND_FORBIDDEN, DIAGNOSTIC_LOCALIZED_VALUE, DIAGNOSTIC_NO_IDENTITY_WARNING,
        DIAGNOSTIC_REFERENCE_ARCHIVED, DIAGNOSTIC_REFERENCE_MISSING, MAX_CSV_IMPORT_ROWS,
    },
    error::AppError,
    state::{AppState, StoredCsvPreview},
};

const PREVIEW_TOKEN_TTL: Duration = Duration::from_secs(30 * 60);
const MAX_CSV_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewCsvImportInput {
    pub source_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CsvImportDiagnosticDto {
    pub row: i32,
    pub field: String,
    pub code: String,
    pub severity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CsvImportPreviewDto {
    pub preview_token: String,
    pub template: String,
    pub sha256: String,
    pub row_count: i32,
    pub valid_count: i32,
    pub duplicate_count: i32,
    pub warning_count: i32,
    pub error_count: i32,
    pub can_commit: bool,
    pub diagnostics: Vec<CsvImportDiagnosticDto>,
}

#[derive(Debug)]
struct ParsedCsv {
    template: ImportTemplate,
    rows: Vec<ParsedRow>,
}

#[derive(Debug)]
struct ParsedRow {
    number: i32,
    cells: HashMap<String, String>,
}

struct Catalog {
    accounts: HashMap<String, CatalogRef>,
    instruments: HashMap<String, CatalogRef>,
    holdings: HashMap<String, CatalogRef>,
    benchmarks: HashMap<String, CatalogRef>,
    identities: HashMap<(String, String), String>,
}

struct CatalogRef {
    archived: bool,
}

pub async fn preview_csv_import(
    state: &AppState,
    input: PreviewCsvImportInput,
) -> Result<CsvImportPreviewDto, AppError> {
    let source = open_csv_source(&input.source_path)?;
    let parsed = parse_csv_bytes(&source.bytes)?;
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let catalog = load_catalog(&mut tx, &household.id, &parsed).await?;
        replay_rows(&mut tx, &household.id, &parsed, &catalog).await
    }
    .await;
    let (valid_count, duplicate_count, diagnostics) = finish_read_tx(tx, result).await?;
    let error_count = diagnostics
        .iter()
        .filter(|item| item.severity == "error")
        .count();
    let warning_count = diagnostics
        .iter()
        .filter(|item| item.severity == "warning")
        .count();
    let token = state.issue_csv_preview(StoredCsvPreview {
        canonical_path: source.canonical_path,
        file_size: source.metadata.len,
        modified_at: source.metadata.modified,
        file_device: source.metadata.device,
        file_inode: source.metadata.inode,
        sha256: source.sha256.clone(),
        expires_at: std::time::Instant::now() + PREVIEW_TOKEN_TTL,
    });
    Ok(CsvImportPreviewDto {
        preview_token: token,
        template: parsed.template.as_str().to_owned(),
        sha256: source.sha256,
        row_count: i32::try_from(parsed.rows.len()).unwrap_or(i32::MAX),
        valid_count,
        duplicate_count,
        warning_count: i32::try_from(warning_count).unwrap_or(i32::MAX),
        error_count: i32::try_from(error_count).unwrap_or(i32::MAX),
        can_commit: error_count == 0,
        diagnostics,
    })
}

#[allow(dead_code)]
pub(crate) fn revalidate_csv_preview_token(
    state: &AppState,
    token: &str,
) -> Result<String, AppError> {
    let preview = state
        .csv_preview(token)
        .ok_or_else(AppError::import_preview_expired)?;
    let metadata =
        file_metadata(&preview.canonical_path).map_err(|_| AppError::import_preview_expired())?;
    let expected = crate::application::backup_service::SourceMetadata {
        len: preview.file_size,
        modified: preview.modified_at,
        device: preview.file_device,
        inode: preview.file_inode,
    };
    let bytes =
        fs::read(&preview.canonical_path).map_err(|_| AppError::import_preview_expired())?;
    let sha256 = hash_bytes(&bytes);
    if !same_file_metadata(&metadata, &expected) || sha256 != preview.sha256 {
        return Err(AppError::import_preview_expired());
    }
    Ok(preview.sha256)
}

struct OpenedCsv {
    canonical_path: PathBuf,
    metadata: crate::application::backup_service::SourceMetadata,
    bytes: Vec<u8>,
    sha256: String,
}

fn open_csv_source(raw_path: &str) -> Result<OpenedCsv, AppError> {
    if raw_path.trim().is_empty() {
        return Err(AppError::validation(
            "sourcePath",
            "A CSV source is required.",
        ));
    }
    let raw = PathBuf::from(raw_path);
    if raw
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value == BACKUP_EXTENSION || value == "json")
    {
        return Err(AppError::invalid_import_row(
            "JSON and backup files are not accepted as CSV import input.",
        ));
    }
    let metadata = fs::symlink_metadata(&raw)
        .map_err(|_| AppError::invalid_import_row("The selected file is unavailable."))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::invalid_import_row(
            "The selected file is not a regular file.",
        ));
    }
    if metadata.len() > MAX_CSV_BYTES {
        return Err(AppError::invalid_import_row(
            "The selected file is too large.",
        ));
    }
    let canonical = fs::canonicalize(&raw)
        .map_err(|_| AppError::invalid_import_row("The selected file is unavailable."))?;
    let metadata = file_metadata(&canonical)
        .map_err(|_| AppError::invalid_import_row("The selected file is unavailable."))?;
    let mut file = fs::File::open(&canonical)
        .map_err(|_| AppError::invalid_import_row("The selected file is unavailable."))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| AppError::invalid_import_row("The selected file is unavailable."))?;
    drop(file);
    if bytes.contains(&0) {
        return Err(AppError::invalid_import_row(
            "CSV input cannot contain NUL bytes.",
        ));
    }
    let sha256 = hash_bytes(&bytes);
    Ok(OpenedCsv {
        canonical_path: canonical,
        metadata,
        bytes,
        sha256,
    })
}

fn parse_csv_bytes(bytes: &[u8]) -> Result<ParsedCsv, AppError> {
    let text = std::str::from_utf8(strip_bom(bytes))
        .map_err(|_| AppError::invalid_import_row("CSV input must be valid UTF-8."))?;
    if text.contains('\0') {
        return Err(AppError::invalid_import_row(
            "CSV input cannot contain NUL bytes.",
        ));
    }
    let (first, rest) = split_first_line(text);
    let template = ImportTemplate::parse(first.trim())
        .map_err(|_| AppError::invalid_import_row("CSV template version is not supported."))?;
    let mut reader = csv::ReaderBuilder::new()
        .flexible(false)
        .has_headers(true)
        .from_reader(Cursor::new(rest.as_bytes()));
    let headers: Vec<String> = reader
        .headers()
        .map_err(|_| AppError::invalid_import_row("CSV quoting is malformed."))?
        .iter()
        .map(str::to_owned)
        .collect();
    validate_headers(template, &headers)?;
    let mut rows = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let record =
            record.map_err(|_| AppError::invalid_import_row("CSV quoting is malformed."))?;
        if record.iter().all(str::is_empty) {
            continue;
        }
        if rows.len() >= MAX_CSV_IMPORT_ROWS {
            return Err(AppError::invalid_import_row(
                "CSV import cannot contain more than 2000 data rows.",
            ));
        }
        let mut cells = HashMap::new();
        for (header, value) in headers.iter().zip(record.iter()) {
            cells.insert(header.clone(), value.to_owned());
        }
        rows.push(ParsedRow {
            number: i32::try_from(index + 1).unwrap_or(i32::MAX),
            cells,
        });
    }
    Ok(ParsedCsv { template, rows })
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes)
}

fn split_first_line(text: &str) -> (&str, &str) {
    match text.find(['\r', '\n']) {
        Some(index) => {
            let first = &text[..index];
            let rest = if text[index..].starts_with("\r\n") {
                &text[index + 2..]
            } else {
                &text[index + 1..]
            };
            (first, rest)
        }
        None => (text, ""),
    }
}

fn validate_headers(template: ImportTemplate, headers: &[String]) -> Result<(), AppError> {
    let expected = template.headers();
    let mut seen = HashSet::new();
    for header in headers {
        if !seen.insert(header) {
            return Err(AppError::invalid_import_row(
                "CSV headers cannot be duplicated.",
            ));
        }
        if !expected.contains(&header.as_str()) {
            return Err(AppError::invalid_import_row(
                "CSV contains an unknown column.",
            ));
        }
    }
    if headers.len() != expected.len()
        || headers
            .iter()
            .zip(expected)
            .any(|(actual, expected)| actual != expected)
    {
        return Err(AppError::invalid_import_row(
            "CSV headers do not match the template.",
        ));
    }
    Ok(())
}

async fn load_catalog(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    parsed: &ParsedCsv,
) -> Result<Catalog, AppError> {
    let mut account_ids = Vec::new();
    let mut instrument_ids = Vec::new();
    let mut holding_ids = Vec::new();
    let mut benchmark_ids = Vec::new();
    let mut namespaces = HashSet::new();
    for row in &parsed.rows {
        push_id(&mut account_ids, cell(row, "account_id"));
        push_id(&mut account_ids, cell(row, "source_account_id"));
        push_id(&mut account_ids, cell(row, "destination_account_id"));
        push_id(&mut account_ids, cell(row, "liability_account_id"));
        push_id(&mut account_ids, cell(row, "cash_account_id"));
        push_id(&mut instrument_ids, cell(row, "instrument_id"));
        push_id(&mut holding_ids, cell(row, "holding_id"));
        push_id(&mut benchmark_ids, cell(row, "benchmark_id"));
        if let Some(namespace) = optional_text(cell(row, "source_namespace")) {
            namespaces.insert(namespace);
        }
    }
    Ok(Catalog {
        accounts: load_refs(
            tx,
            "csv_preview.accounts_batch",
            "SELECT id, archived_at FROM accounts WHERE household_id = ?",
            household_id,
            &account_ids,
        )
        .await?,
        instruments: load_refs(
            tx,
            "csv_preview.instruments_batch",
            "SELECT id, archived_at FROM instruments WHERE household_id = ?",
            household_id,
            &instrument_ids,
        )
        .await?,
        holdings: load_refs(
            tx,
            "csv_preview.holdings_batch",
            "SELECT h.id, h.archived_at FROM holdings h INNER JOIN accounts a ON a.id = h.account_id WHERE a.household_id = ?",
            household_id,
            &holding_ids,
        )
        .await?,
        benchmarks: load_refs(
            tx,
            "csv_preview.benchmarks_batch",
            "SELECT id, archived_at FROM benchmarks WHERE household_id = ?",
            household_id,
            &benchmark_ids,
        )
        .await?,
        identities: load_identities(tx, &namespaces).await?,
    })
}

fn push_id(ids: &mut Vec<String>, value: &str) {
    if let Some(value) = optional_text(value) {
        if !ids.contains(&value) {
            ids.push(value);
        }
    }
}

fn cell<'a>(row: &'a ParsedRow, header: &str) -> &'a str {
    row.cells.get(header).map(String::as_str).unwrap_or("")
}

async fn load_refs(
    tx: &mut Transaction<'_, Sqlite>,
    family: &'static str,
    sql: &str,
    household_id: &str,
    ids: &[String],
) -> Result<HashMap<String, CatalogRef>, AppError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    query_count::record(family);
    let rows = sqlx::query(sql)
        .bind(household_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("csv_preview.reference_batch_failed", error))?;
    let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut map = HashMap::new();
    for row in rows {
        let id: String = row.try_get("id").map_err(|_| AppError::Internal)?;
        if wanted.contains(id.as_str()) {
            let archived: Option<String> =
                row.try_get("archived_at").map_err(|_| AppError::Internal)?;
            map.insert(
                id,
                CatalogRef {
                    archived: archived.is_some(),
                },
            );
        }
    }
    Ok(map)
}

async fn load_identities(
    tx: &mut Transaction<'_, Sqlite>,
    namespaces: &HashSet<String>,
) -> Result<HashMap<(String, String), String>, AppError> {
    if namespaces.is_empty() {
        return Ok(HashMap::new());
    }
    query_count::record("csv_preview.import_identities_batch");
    let mut map = HashMap::new();
    for namespace in namespaces {
        let rows = sqlx::query(
            "SELECT source_namespace, external_id, fingerprint FROM import_items WHERE source_namespace = ?",
        )
        .bind(namespace)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("csv_preview.import_identities_failed", error))?;
        for row in rows {
            let namespace: String = row
                .try_get("source_namespace")
                .map_err(|_| AppError::Internal)?;
            let external_id: String = row.try_get("external_id").map_err(|_| AppError::Internal)?;
            let fingerprint: String = row.try_get("fingerprint").map_err(|_| AppError::Internal)?;
            map.insert((namespace, external_id), fingerprint);
        }
    }
    Ok(map)
}

async fn replay_rows(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    parsed: &ParsedCsv,
    catalog: &Catalog,
) -> Result<(i32, i32, Vec<CsvImportDiagnosticDto>), AppError> {
    let mut diagnostics = Vec::new();
    let mut valid_count = 0_i32;
    let mut duplicate_count = 0_i32;
    let mut seen_identities: HashMap<(String, String), (i32, String)> = HashMap::new();
    let mut seen_no_id: HashSet<String> = HashSet::new();
    for row in &parsed.rows {
        match parsed.template {
            ImportTemplate::ActivityV1 => {
                match replay_activity(
                    tx,
                    household_id,
                    row,
                    catalog,
                    &mut seen_identities,
                    &mut seen_no_id,
                )
                .await
                {
                    RowOutcome::Valid => valid_count += 1,
                    RowOutcome::Warning(diagnostic) => {
                        valid_count += 1;
                        diagnostics.push(diagnostic);
                    }
                    RowOutcome::Duplicate(diagnostic) => {
                        duplicate_count += 1;
                        diagnostics.push(diagnostic);
                    }
                    RowOutcome::Invalid(diagnostic) => diagnostics.push(diagnostic),
                }
            }
            ImportTemplate::QuoteV1 => {
                match replay_quote(household_id, row, catalog, &mut seen_identities) {
                    RowOutcome::Valid => valid_count += 1,
                    RowOutcome::Warning(diagnostic) => {
                        valid_count += 1;
                        diagnostics.push(diagnostic);
                    }
                    RowOutcome::Duplicate(diagnostic) => {
                        duplicate_count += 1;
                        diagnostics.push(diagnostic);
                    }
                    RowOutcome::Invalid(diagnostic) => diagnostics.push(diagnostic),
                }
            }
            ImportTemplate::BenchmarkV1 => {
                match replay_benchmark(row, catalog, &mut seen_identities) {
                    RowOutcome::Valid => valid_count += 1,
                    RowOutcome::Warning(diagnostic) => {
                        valid_count += 1;
                        diagnostics.push(diagnostic);
                    }
                    RowOutcome::Duplicate(diagnostic) => {
                        duplicate_count += 1;
                        diagnostics.push(diagnostic);
                    }
                    RowOutcome::Invalid(diagnostic) => diagnostics.push(diagnostic),
                }
            }
        }
    }
    Ok((valid_count, duplicate_count, diagnostics))
}

enum RowOutcome {
    Valid,
    Warning(CsvImportDiagnosticDto),
    Duplicate(CsvImportDiagnosticDto),
    Invalid(CsvImportDiagnosticDto),
}

fn diagnostic(row: i32, field: &str, code: &str, severity: &str) -> CsvImportDiagnosticDto {
    CsvImportDiagnosticDto {
        row,
        field: field.to_owned(),
        code: code.to_owned(),
        severity: severity.to_owned(),
    }
}

async fn replay_activity(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    row: &ParsedRow,
    catalog: &Catalog,
    seen_identities: &mut HashMap<(String, String), (i32, String)>,
    seen_no_id: &mut HashSet<String>,
) -> RowOutcome {
    let escaped = match parse_escaped(row) {
        Ok(value) => value,
        Err(code) => {
            return RowOutcome::Invalid(diagnostic(row.number, CSV_ESCAPED_COLUMN, code, "error"))
        }
    };
    let kind = unescape(cell(row, "kind"), escaped);
    if matches!(
        kind.as_str(),
        "opening_adjustment" | "reversal" | "correction"
    ) {
        return RowOutcome::Invalid(diagnostic(
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
        return RowOutcome::Invalid(diagnostic(
            row.number,
            field,
            DIAGNOSTIC_LOCALIZED_VALUE,
            "error",
        ));
    }
    if looks_localized_date(&unescape(cell(row, "effective_local_date"), escaped)) {
        return RowOutcome::Invalid(diagnostic(
            row.number,
            "effective_local_date",
            DIAGNOSTIC_LOCALIZED_VALUE,
            "error",
        ));
    }
    let input = match activity_input(row, escaped) {
        Ok(input) => input,
        Err(_) => {
            return RowOutcome::Invalid(diagnostic(
                row.number,
                "kind",
                DIAGNOSTIC_DOMAIN_INVALID,
                "error",
            ))
        }
    };
    if let Some(outcome) = check_reference(
        row.number,
        "account_id",
        cell(row, "account_id"),
        &catalog.accounts,
    ) {
        return outcome;
    }
    if let Some(outcome) = check_reference(
        row.number,
        "holding_id",
        cell(row, "holding_id"),
        &catalog.holdings,
    ) {
        return outcome;
    }
    if let Some(outcome) = check_reference(
        row.number,
        "instrument_id",
        cell(row, "instrument_id"),
        &catalog.instruments,
    ) {
        return outcome;
    }
    let fingerprint = match activity_row_fingerprint(row, escaped) {
        Ok(value) => value,
        Err(_) => {
            return RowOutcome::Invalid(diagnostic(
                row.number,
                "kind",
                DIAGNOSTIC_DOMAIN_INVALID,
                "error",
            ))
        }
    };
    if let Some(outcome) = identity_outcome(
        row,
        fingerprint.checksum().hex(),
        catalog,
        seen_identities,
        seen_no_id,
    ) {
        return outcome;
    }
    if let Err(_error) = preview_create_activity_in_tx(tx, household_id, input).await {
        return RowOutcome::Invalid(diagnostic(
            row.number,
            "kind",
            DIAGNOSTIC_DOMAIN_INVALID,
            "error",
        ));
    }
    RowOutcome::Valid
}

fn replay_quote(
    _household_id: &str,
    row: &ParsedRow,
    catalog: &Catalog,
    seen_identities: &mut HashMap<(String, String), (i32, String)>,
) -> RowOutcome {
    let escaped = match parse_escaped(row) {
        Ok(value) => value,
        Err(code) => {
            return RowOutcome::Invalid(diagnostic(row.number, CSV_ESCAPED_COLUMN, code, "error"))
        }
    };
    let quote_kind = unescape(cell(row, "quote_kind"), escaped);
    if let Err(field) = reject_localized(row, &["unit_price", "rate"]) {
        return RowOutcome::Invalid(diagnostic(
            row.number,
            field,
            DIAGNOSTIC_LOCALIZED_VALUE,
            "error",
        ));
    }
    match quote_kind.as_str() {
        "instrument" => {
            if let Some(outcome) = check_reference(
                row.number,
                "instrument_id",
                cell(row, "instrument_id"),
                &catalog.instruments,
            ) {
                return outcome;
            }
            if UnitPrice::parse(&unescape(cell(row, "unit_price"), escaped)).is_err() {
                return RowOutcome::Invalid(diagnostic(
                    row.number,
                    "unit_price",
                    DIAGNOSTIC_DOMAIN_INVALID,
                    "error",
                ));
            }
        }
        "fx" => {
            let base = unescape(cell(row, "base_currency"), escaped);
            let quote = unescape(cell(row, "quote_currency"), escaped);
            if CurrencyCode::parse_supported(&base).is_err() {
                return RowOutcome::Invalid(diagnostic(
                    row.number,
                    "base_currency",
                    DIAGNOSTIC_DOMAIN_INVALID,
                    "error",
                ));
            }
            if CurrencyCode::parse_supported(&quote).is_err() {
                return RowOutcome::Invalid(diagnostic(
                    row.number,
                    "quote_currency",
                    DIAGNOSTIC_DOMAIN_INVALID,
                    "error",
                ));
            }
            if FxRate::parse(&unescape(cell(row, "rate"), escaped)).is_err() {
                return RowOutcome::Invalid(diagnostic(
                    row.number,
                    "rate",
                    DIAGNOSTIC_DOMAIN_INVALID,
                    "error",
                ));
            }
        }
        _ => {
            return RowOutcome::Invalid(diagnostic(
                row.number,
                "quote_kind",
                DIAGNOSTIC_DOMAIN_INVALID,
                "error",
            ))
        }
    }
    let fingerprint = match quote_row_fingerprint(row, escaped) {
        Ok(value) => value,
        Err(_) => {
            return RowOutcome::Invalid(diagnostic(
                row.number,
                "quote_kind",
                DIAGNOSTIC_DOMAIN_INVALID,
                "error",
            ))
        }
    };
    if let Some(outcome) = identity_outcome(
        row,
        fingerprint.checksum().hex(),
        catalog,
        seen_identities,
        &mut HashSet::new(),
    ) {
        return outcome;
    }
    RowOutcome::Valid
}

fn replay_benchmark(
    row: &ParsedRow,
    catalog: &Catalog,
    seen_identities: &mut HashMap<(String, String), (i32, String)>,
) -> RowOutcome {
    let escaped = match parse_escaped(row) {
        Ok(value) => value,
        Err(code) => {
            return RowOutcome::Invalid(diagnostic(row.number, CSV_ESCAPED_COLUMN, code, "error"))
        }
    };
    if looks_localized_decimal(&unescape(cell(row, "level"), escaped))
        || looks_localized_date(&unescape(cell(row, "observed_on"), escaped))
    {
        return RowOutcome::Invalid(diagnostic(
            row.number,
            "level",
            DIAGNOSTIC_LOCALIZED_VALUE,
            "error",
        ));
    }
    if let Some(outcome) = check_reference(
        row.number,
        "benchmark_id",
        cell(row, "benchmark_id"),
        &catalog.benchmarks,
    ) {
        return outcome;
    }
    if CalendarDate::parse(&unescape(cell(row, "observed_on"), escaped)).is_err() {
        return RowOutcome::Invalid(diagnostic(
            row.number,
            "observed_on",
            DIAGNOSTIC_DOMAIN_INVALID,
            "error",
        ));
    }
    if BenchmarkLevel::parse(&unescape(cell(row, "level"), escaped)).is_err() {
        return RowOutcome::Invalid(diagnostic(
            row.number,
            "level",
            DIAGNOSTIC_DOMAIN_INVALID,
            "error",
        ));
    }
    let fingerprint = match benchmark_row_fingerprint(row, escaped) {
        Ok(value) => value,
        Err(_) => {
            return RowOutcome::Invalid(diagnostic(
                row.number,
                "benchmark_id",
                DIAGNOSTIC_DOMAIN_INVALID,
                "error",
            ))
        }
    };
    if let Some(outcome) = identity_outcome(
        row,
        fingerprint.checksum().hex(),
        catalog,
        seen_identities,
        &mut HashSet::new(),
    ) {
        return outcome;
    }
    RowOutcome::Valid
}

fn check_reference(
    row: i32,
    field: &str,
    value: &str,
    catalog: &HashMap<String, CatalogRef>,
) -> Option<RowOutcome> {
    let id = optional_text(value)?;
    match catalog.get(&id) {
        None => Some(RowOutcome::Invalid(diagnostic(
            row,
            field,
            DIAGNOSTIC_REFERENCE_MISSING,
            "error",
        ))),
        Some(item) if item.archived => Some(RowOutcome::Invalid(diagnostic(
            row,
            field,
            DIAGNOSTIC_REFERENCE_ARCHIVED,
            "error",
        ))),
        Some(_) => None,
    }
}

fn identity_outcome(
    row: &ParsedRow,
    fingerprint: String,
    catalog: &Catalog,
    seen_identities: &mut HashMap<(String, String), (i32, String)>,
    seen_no_id: &mut HashSet<String>,
) -> Option<RowOutcome> {
    let namespace = optional_text(cell(row, "source_namespace"));
    let external_id = optional_text(cell(row, "external_id"));
    match (namespace, external_id) {
        (None, None) => {
            if !seen_no_id.insert(fingerprint) {
                return Some(RowOutcome::Warning(diagnostic(
                    row.number,
                    "external_id",
                    DIAGNOSTIC_NO_IDENTITY_WARNING,
                    "warning",
                )));
            }
            None
        }
        (Some(_), None) | (None, Some(_)) => Some(RowOutcome::Invalid(diagnostic(
            row.number,
            "external_id",
            DIAGNOSTIC_DOMAIN_INVALID,
            "error",
        ))),
        (Some(namespace), Some(external_id)) => {
            if parse_optional_namespace(&namespace).is_err()
                || parse_optional_external_id(&external_id).is_err()
            {
                return Some(RowOutcome::Invalid(diagnostic(
                    row.number,
                    "source_namespace",
                    DIAGNOSTIC_DOMAIN_INVALID,
                    "error",
                )));
            }
            let key = (namespace.clone(), external_id.clone());
            if let Some((_, previous)) = seen_identities.get(&key) {
                if previous == &fingerprint {
                    return Some(RowOutcome::Duplicate(diagnostic(
                        row.number,
                        "external_id",
                        DIAGNOSTIC_EXACT_DUPLICATE,
                        "warning",
                    )));
                }
                return Some(RowOutcome::Invalid(diagnostic(
                    row.number,
                    "external_id",
                    DIAGNOSTIC_DUPLICATE_CONFLICT,
                    "error",
                )));
            }
            if let Some(previous) = catalog.identities.get(&key) {
                if previous == &fingerprint {
                    seen_identities.insert(key, (row.number, fingerprint));
                    return Some(RowOutcome::Duplicate(diagnostic(
                        row.number,
                        "external_id",
                        DIAGNOSTIC_EXACT_DUPLICATE,
                        "warning",
                    )));
                }
                return Some(RowOutcome::Invalid(diagnostic(
                    row.number,
                    "external_id",
                    DIAGNOSTIC_DUPLICATE_CONFLICT,
                    "error",
                )));
            }
            seen_identities.insert(key, (row.number, fingerprint));
            None
        }
    }
}

fn parse_escaped(row: &ParsedRow) -> Result<bool, &'static str> {
    let value = cell(row, CSV_ESCAPED_COLUMN);
    if value.is_empty() {
        return Ok(false);
    }
    parse_strict_boolean(CSV_ESCAPED_COLUMN, value).map_err(|_| {
        if looks_localized_boolean(value) {
            DIAGNOSTIC_LOCALIZED_VALUE
        } else {
            DIAGNOSTIC_DOMAIN_INVALID
        }
    })
}

fn unescape(value: &str, escaped: bool) -> String {
    unescape_spreadsheet_text(value, escaped)
}

fn reject_localized<'a>(row: &'a ParsedRow, fields: &[&'a str]) -> Result<(), &'a str> {
    for field in fields {
        let value = cell(row, field);
        if !value.is_empty() && looks_localized_decimal(value) {
            return Err(*field);
        }
    }
    Ok(())
}

fn activity_input(row: &ParsedRow, escaped: bool) -> Result<CreateActivityInput, AppError> {
    let kind = unescape(cell(row, "kind"), escaped);
    let local_date = unescape(cell(row, "effective_local_date"), escaped);
    let local_time = unescape(cell(row, "effective_local_time"), escaped);
    let ambiguous_offset = optional_text(&unescape(cell(row, "ambiguous_offset"), escaped));
    let note = optional_text(&unescape(cell(row, "note"), escaped));
    match kind.as_str() {
        "deposit" => Ok(CreateActivityInput::Deposit {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            account_id: unescape(cell(row, "account_id"), escaped),
            component: unescape(cell(row, "component_kind"), escaped),
            amount: unescape(cell(row, "amount"), escaped),
            currency: unescape(cell(row, "currency"), escaped),
        }),
        "withdrawal" => Ok(CreateActivityInput::Withdrawal {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            account_id: unescape(cell(row, "account_id"), escaped),
            component: unescape(cell(row, "component_kind"), escaped),
            amount: unescape(cell(row, "amount"), escaped),
            currency: unescape(cell(row, "currency"), escaped),
        }),
        "income" => Ok(CreateActivityInput::Income {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            account_id: unescape(cell(row, "account_id"), escaped),
            component: unescape(cell(row, "component_kind"), escaped),
            amount: unescape(cell(row, "amount"), escaped),
            currency: unescape(cell(row, "currency"), escaped),
            income_kind: unescape(cell(row, "income_kind"), escaped),
            instrument_id: optional_text(&unescape(cell(row, "instrument_id"), escaped)),
        }),
        "fee" => Ok(CreateActivityInput::Fee {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            account_id: unescape(cell(row, "account_id"), escaped),
            component: unescape(cell(row, "component_kind"), escaped),
            amount: unescape(cell(row, "amount"), escaped),
            currency: unescape(cell(row, "currency"), escaped),
            fee_kind: unescape(cell(row, "fee_kind"), escaped),
            instrument_id: optional_text(&unescape(cell(row, "instrument_id"), escaped)),
        }),
        "balance_adjustment" => Ok(CreateActivityInput::BalanceAdjustment {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            account_id: unescape(cell(row, "account_id"), escaped),
            amount: unescape(cell(row, "amount"), escaped),
            currency: unescape(cell(row, "currency"), escaped),
        }),
        "position_adjustment" => Ok(CreateActivityInput::PositionAdjustment {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            holding_id: unescape(cell(row, "holding_id"), escaped),
            quantity: unescape(cell(row, "quantity"), escaped),
        }),
        "transfer" => Ok(CreateActivityInput::Transfer {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            source_account_id: unescape(cell(row, "source_account_id"), escaped),
            source_component: unescape(cell(row, "source_component"), escaped),
            source_amount: unescape(cell(row, "source_amount"), escaped),
            source_currency: unescape(cell(row, "source_currency"), escaped),
            destination_account_id: unescape(cell(row, "destination_account_id"), escaped),
            destination_component: unescape(cell(row, "destination_component"), escaped),
            destination_amount: unescape(cell(row, "destination_amount"), escaped),
            destination_currency: unescape(cell(row, "destination_currency"), escaped),
            source_holding_id: optional_text(&unescape(cell(row, "holding_id"), escaped)),
            destination_holding_id: None,
            quantity: optional_text(&unescape(cell(row, "quantity"), escaped)),
            fee_amount: optional_text(&unescape(cell(row, "fee_amount"), escaped)),
            fee_kind: optional_text(&unescape(cell(row, "fee_kind"), escaped)),
        }),
        "buy" => Ok(CreateActivityInput::Buy {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            holding_id: unescape(cell(row, "holding_id"), escaped),
            quantity: unescape(cell(row, "quantity"), escaped),
            unit_price: unescape(cell(row, "unit_price"), escaped),
            gross_amount: unescape(cell(row, "gross_amount"), escaped),
            settlement_currency: unescape(cell(row, "settlement_currency"), escaped),
            fee_amount: optional_text(&unescape(cell(row, "fee_amount"), escaped)),
            confirm_zero_unit_price: parse_strict_boolean(
                "confirm_zero_unit_price",
                &empty_as_false(&unescape(cell(row, "confirm_zero_unit_price"), escaped)),
            )
            .unwrap_or(false),
        }),
        "sell" => Ok(CreateActivityInput::Sell {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            holding_id: unescape(cell(row, "holding_id"), escaped),
            quantity: unescape(cell(row, "quantity"), escaped),
            unit_price: unescape(cell(row, "unit_price"), escaped),
            gross_amount: unescape(cell(row, "gross_amount"), escaped),
            settlement_currency: unescape(cell(row, "settlement_currency"), escaped),
            fee_amount: optional_text(&unescape(cell(row, "fee_amount"), escaped)),
            confirm_zero_unit_price: parse_strict_boolean(
                "confirm_zero_unit_price",
                &empty_as_false(&unescape(cell(row, "confirm_zero_unit_price"), escaped)),
            )
            .unwrap_or(false),
        }),
        "debt_draw" => Ok(CreateActivityInput::DebtDraw {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            liability_account_id: unescape(cell(row, "liability_account_id"), escaped),
            principal_amount: unescape(cell(row, "principal_amount"), escaped),
            principal_currency: unescape(cell(row, "principal_currency"), escaped),
            cash_account_id: optional_text(&unescape(cell(row, "cash_account_id"), escaped)),
            cash_component: optional_text(&unescape(cell(row, "cash_component"), escaped)),
            cash_amount: optional_text(&unescape(cell(row, "cash_amount"), escaped)),
            cash_currency: optional_text(&unescape(cell(row, "cash_currency"), escaped)),
            fx_rate: optional_text(&unescape(cell(row, "fx_rate"), escaped)),
        }),
        "debt_payment" => Ok(CreateActivityInput::DebtPayment {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            liability_account_id: unescape(cell(row, "liability_account_id"), escaped),
            principal_amount: unescape(cell(row, "principal_amount"), escaped),
            principal_currency: unescape(cell(row, "principal_currency"), escaped),
            cash_account_id: unescape(cell(row, "cash_account_id"), escaped),
            cash_component: unescape(cell(row, "cash_component"), escaped),
            cash_amount: unescape(cell(row, "cash_amount"), escaped),
            cash_currency: unescape(cell(row, "cash_currency"), escaped),
            fx_rate: optional_text(&unescape(cell(row, "fx_rate"), escaped)),
            fee_amount: optional_text(&unescape(cell(row, "fee_amount"), escaped)),
            fee_kind: optional_text(&unescape(cell(row, "fee_kind"), escaped)),
        }),
        "debt_adjustment" => Ok(CreateActivityInput::DebtAdjustment {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            account_id: unescape(cell(row, "account_id"), escaped),
            amount: unescape(cell(row, "amount"), escaped),
            currency: unescape(cell(row, "currency"), escaped),
        }),
        "manual_valuation" => Ok(CreateActivityInput::ManualValuation {
            local_date,
            local_time,
            ambiguous_offset,
            note,
            account_id: unescape(cell(row, "account_id"), escaped),
            amount: unescape(cell(row, "amount"), escaped),
            currency: unescape(cell(row, "currency"), escaped),
        }),
        _ => Err(AppError::invalid_import_row(
            "Activity kind is not supported.",
        )),
    }
}

fn empty_as_false(value: &str) -> String {
    if value.is_empty() {
        "false".to_owned()
    } else {
        value.to_owned()
    }
}

fn activity_row_fingerprint(row: &ParsedRow, escaped: bool) -> Result<ImportFingerprint, AppError> {
    let namespace = parse_optional_namespace(&unescape(cell(row, "source_namespace"), escaped))?;
    let external_id = parse_optional_external_id(&unescape(cell(row, "external_id"), escaped))?;
    let extras = vec![
        optional_field(cell(row, "source_account_id")),
        optional_field(cell(row, "source_component")),
        optional_field(cell(row, "source_amount")),
        optional_field(cell(row, "source_currency")),
        optional_field(cell(row, "destination_account_id")),
        optional_field(cell(row, "destination_component")),
        optional_field(cell(row, "destination_amount")),
        optional_field(cell(row, "destination_currency")),
        optional_field(cell(row, "holding_id")),
        optional_field(cell(row, "instrument_id")),
        optional_field(cell(row, "quantity")),
        optional_field(cell(row, "unit_price")),
        optional_field(cell(row, "gross_amount")),
        optional_field(cell(row, "settlement_currency")),
        optional_field(cell(row, "fee_amount")),
        optional_field(cell(row, "fee_kind")),
        optional_field(cell(row, "income_kind")),
        optional_field(cell(row, "liability_account_id")),
        optional_field(cell(row, "principal_amount")),
        optional_field(cell(row, "principal_currency")),
        optional_field(cell(row, "cash_account_id")),
        optional_field(cell(row, "cash_component")),
        optional_field(cell(row, "cash_amount")),
        optional_field(cell(row, "cash_currency")),
        optional_field(cell(row, "fx_rate")),
        optional_field(cell(row, "confirm_zero_unit_price")),
    ];
    activity_fingerprint(
        namespace.as_ref(),
        external_id.as_ref(),
        &unescape(cell(row, "kind"), escaped),
        &unescape(cell(row, "effective_local_date"), escaped),
        &unescape(cell(row, "effective_local_time"), escaped),
        optional_text(&unescape(cell(row, "ambiguous_offset"), escaped)).as_deref(),
        optional_text(&unescape(cell(row, "account_id"), escaped)).as_deref(),
        optional_text(&unescape(cell(row, "component_kind"), escaped)).as_deref(),
        optional_text(&unescape(cell(row, "amount"), escaped)).as_deref(),
        optional_text(&unescape(cell(row, "currency"), escaped)).as_deref(),
        optional_text(&unescape(cell(row, "note"), escaped)).as_deref(),
        &extras,
    )
}

fn quote_row_fingerprint(row: &ParsedRow, escaped: bool) -> Result<ImportFingerprint, AppError> {
    quote_fingerprint(
        parse_optional_namespace(&unescape(cell(row, "source_namespace"), escaped))?.as_ref(),
        parse_optional_external_id(&unescape(cell(row, "external_id"), escaped))?.as_ref(),
        &unescape(cell(row, "quote_kind"), escaped),
        optional_text(&unescape(cell(row, "instrument_id"), escaped)).as_deref(),
        optional_text(&unescape(cell(row, "unit_price"), escaped)).as_deref(),
        optional_text(&unescape(cell(row, "base_currency"), escaped)).as_deref(),
        optional_text(&unescape(cell(row, "quote_currency"), escaped)).as_deref(),
        optional_text(&unescape(cell(row, "rate"), escaped)).as_deref(),
        optional_text(&unescape(cell(row, "quoted_at"), escaped)).as_deref(),
        optional_text(&unescape(cell(row, "note"), escaped)).as_deref(),
    )
}

fn benchmark_row_fingerprint(
    row: &ParsedRow,
    escaped: bool,
) -> Result<ImportFingerprint, AppError> {
    benchmark_fingerprint(
        parse_optional_namespace(&unescape(cell(row, "source_namespace"), escaped))?.as_ref(),
        parse_optional_external_id(&unescape(cell(row, "external_id"), escaped))?.as_ref(),
        &unescape(cell(row, "benchmark_id"), escaped),
        &unescape(cell(row, "observed_on"), escaped),
        &unescape(cell(row, "level"), escaped),
        optional_text(&unescape(cell(row, "note"), escaped)).as_deref(),
    )
}

fn optional_field(value: &str) -> ImportField {
    optional_text(value).map_or(ImportField::Missing, ImportField::Text)
}

#[cfg(test)]
pub(crate) fn preview_parse_error_code(error: &AppError) -> Option<&'static str> {
    use crate::domain::{
        DIAGNOSTIC_HEADER_DUPLICATE, DIAGNOSTIC_HEADER_MISSING, DIAGNOSTIC_HEADER_UNKNOWN,
        DIAGNOSTIC_MALFORMED_QUOTE, DIAGNOSTIC_NUL, DIAGNOSTIC_ROW_LIMIT,
        DIAGNOSTIC_TEMPLATE_INVALID, DIAGNOSTIC_UTF8_INVALID,
    };

    let AppError::InvalidImportRow { message } = error else {
        return None;
    };
    if message.contains("UTF-8") {
        Some(DIAGNOSTIC_UTF8_INVALID)
    } else if message.contains("NUL") {
        Some(DIAGNOSTIC_NUL)
    } else if message.contains("quoting") {
        Some(DIAGNOSTIC_MALFORMED_QUOTE)
    } else if message.contains("2000") {
        Some(DIAGNOSTIC_ROW_LIMIT)
    } else if message.contains("duplicated") {
        Some(DIAGNOSTIC_HEADER_DUPLICATE)
    } else if message.contains("unknown column") {
        Some(DIAGNOSTIC_HEADER_UNKNOWN)
    } else if message.contains("headers") {
        Some(DIAGNOSTIC_HEADER_MISSING)
    } else if message.contains("template") || message.contains("JSON") {
        Some(DIAGNOSTIC_TEMPLATE_INVALID)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            account_service::{self, CreateAccountInput, OwnershipShareInput},
            export_service::{self, CsvExportDataset, ExportCsvInput},
            history_query_service::{
                confirm_history_timezone, create_activity, get_history_origin,
                ConfirmHistoryTimezoneInput, CreateActivityInput,
            },
            onboarding_service::complete_onboarding,
            query_count,
        },
        domain::{
            ACTIVITY_CSV_HEADERS, DIAGNOSTIC_LOCALIZED_VALUE, DIAGNOSTIC_NUL, DIAGNOSTIC_ROW_LIMIT,
        },
        test_support::{stable_sqlite_hash, test_path, valid_onboarding_input},
    };

    fn cleanup(path: &PathBuf) {
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

    fn empty_activity_cells() -> Vec<String> {
        vec![String::new(); ACTIVITY_CSV_HEADERS.len()]
    }

    fn set_cell(values: &mut [String], header: &str, value: &str) {
        if let Some(index) = ACTIVITY_CSV_HEADERS.iter().position(|name| *name == header) {
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
        let root = test_path("phase7-preview", name);
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

    fn deposit_row(account_id: &str, date: &str, note: &str) -> String {
        let mut values = empty_activity_cells();
        set_cell(&mut values, "source_namespace", "acct.example");
        set_cell(&mut values, "external_id", "salary-2026-01");
        set_cell(&mut values, "kind", "deposit");
        set_cell(&mut values, "effective_local_date", date);
        set_cell(&mut values, "effective_local_time", "09:00");
        set_cell(&mut values, "account_id", account_id);
        set_cell(&mut values, "component_kind", "account_value");
        set_cell(&mut values, "amount", "5000");
        set_cell(&mut values, "currency", "CNY");
        set_cell(&mut values, "note", note);
        set_cell(&mut values, "escaped_for_spreadsheet", "false");
        encode_row(&values)
    }

    #[test]
    fn parser_rejects_malformed_and_oversize_csv() {
        let headers = ACTIVITY_CSV_HEADERS.join(",");
        let unknown = format!("# nestworth-csv:activity:v1\r\n{headers},extra\r\none\r\n");
        assert!(parse_csv_bytes(unknown.as_bytes()).is_err());
        let missing = b"# nestworth-csv:activity:v1\r\nsource_namespace,kind\r\n";
        assert!(parse_csv_bytes(missing).is_err());
        let duplicate = format!(
            "# nestworth-csv:activity:v1\r\n{headers},{}\r\n",
            ACTIVITY_CSV_HEADERS[0]
        );
        assert!(parse_csv_bytes(duplicate.as_bytes()).is_err());
        assert!(parse_csv_bytes(b"# nestworth-csv:unknown:v1\r\n").is_err());
        assert!(parse_csv_bytes(&[0xff, 0xfe, b'a']).is_err());
        let mut nul = b"# nestworth-csv:activity:v1\r\n".to_vec();
        nul.push(0);
        let error = parse_csv_bytes(&nul).expect_err("nul");
        assert_eq!(preview_parse_error_code(&error), Some(DIAGNOSTIC_NUL));
        let malformed = b"# nestworth-csv:activity:v1\r\nsource_namespace,\"unterminated\r\n";
        assert!(parse_csv_bytes(malformed).is_err());
        let mut rows = Vec::new();
        let mut values = empty_activity_cells();
        set_cell(&mut values, "kind", "deposit");
        let filled = encode_row(&values);
        for _ in 0..2001 {
            rows.push(filled.as_str());
        }
        let oversize = activity_csv(&rows);
        let error = parse_csv_bytes(oversize.as_bytes()).expect_err("row limit");
        assert_eq!(preview_parse_error_code(&error), Some(DIAGNOSTIC_ROW_LIMIT));
    }

    #[test]
    fn preview_is_read_only_and_round_trips_hardened_csv() {
        tauri::async_runtime::block_on(async {
            let (state, root, account_id, date) = onboarded("preview-readonly").await;
            create_activity(
                &state,
                CreateActivityInput::Deposit {
                    local_date: date.clone(),
                    local_time: "09:00".to_owned(),
                    ambiguous_offset: None,
                    note: Some("=SUM(A1) salary".to_owned()),
                    account_id: account_id.clone(),
                    component: "account_value".to_owned(),
                    amount: "100".to_owned(),
                    currency: "CNY".to_owned(),
                },
            )
            .await
            .expect("seed deposit");
            let exported = root.with_extension("roundtrip.csv");
            export_service::export_csv(
                &state,
                ExportCsvInput {
                    destination_path: exported.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                    dataset: CsvExportDataset::Activity,
                },
            )
            .await
            .expect("export");
            let before = stable_sqlite_hash(&root).await;
            let (preview, families) = query_count::capture_async(|| {
                preview_csv_import(
                    &state,
                    PreviewCsvImportInput {
                        source_path: exported.to_string_lossy().into_owned(),
                    },
                )
            })
            .await;
            let preview = preview.expect("preview");
            assert!(preview.row_count >= 1);
            assert_eq!(preview.error_count, 0);
            for item in &preview.diagnostics {
                assert!(!item.code.is_empty());
                assert!(!item.field.contains('/'));
                assert!(!item.field.contains("=SUM"));
            }
            let account_batches = families
                .iter()
                .filter(|family| **family == "csv_preview.accounts_batch")
                .count();
            assert!(account_batches <= 1);
            assert!(!families
                .iter()
                .any(|family| family.contains("account_by_id")));
            let after = stable_sqlite_hash(&root).await;
            assert_eq!(before, after);
            fs::remove_file(&exported).expect("file handle should not remain");
            drop(state);
            cleanup(&root);
        });
    }

    #[test]
    fn preview_rejects_localized_values_and_json_input() {
        tauri::async_runtime::block_on(async {
            let (state, root, account_id, date) = onboarded("preview-reject").await;
            let mut values = empty_activity_cells();
            set_cell(&mut values, "kind", "deposit");
            set_cell(&mut values, "effective_local_date", "31/01/2026");
            set_cell(&mut values, "effective_local_time", "09:00");
            set_cell(&mut values, "account_id", &account_id);
            set_cell(&mut values, "component_kind", "account_value");
            set_cell(&mut values, "amount", "5.000,00");
            set_cell(&mut values, "currency", "CNY");
            set_cell(&mut values, "escaped_for_spreadsheet", "TRUE");
            let csv_path = root.with_extension("bad.csv");
            fs::write(&csv_path, activity_csv(&[&encode_row(&values)])).expect("write");
            let preview = preview_csv_import(
                &state,
                PreviewCsvImportInput {
                    source_path: csv_path.to_string_lossy().into_owned(),
                },
            )
            .await
            .expect("preview returns diagnostics");
            assert!(preview.error_count >= 1);
            assert!(preview
                .diagnostics
                .iter()
                .any(|item| item.code == DIAGNOSTIC_LOCALIZED_VALUE));
            let json_path = root.with_extension("not-csv.json");
            fs::write(&json_path, b"{\"formatId\":\"com.nestworth.export\"}").expect("json");
            let json_preview = preview_csv_import(
                &state,
                PreviewCsvImportInput {
                    source_path: json_path.to_string_lossy().into_owned(),
                },
            )
            .await;
            assert!(json_preview.is_err());
            let _ = date;
            drop(state);
            let _ = fs::remove_file(csv_path);
            let _ = fs::remove_file(json_path);
            cleanup(&root);
        });
    }

    #[test]
    fn preview_token_expires_and_binds_file_identity() {
        tauri::async_runtime::block_on(async {
            let (state, root, account_id, date) = onboarded("preview-token").await;
            let csv_path = root.with_extension("token.csv");
            fs::write(
                &csv_path,
                activity_csv(&[&deposit_row(&account_id, &date, "January salary")]),
            )
            .expect("write");
            let preview = preview_csv_import(
                &state,
                PreviewCsvImportInput {
                    source_path: csv_path.to_string_lossy().into_owned(),
                },
            )
            .await
            .expect("preview");
            assert!(!preview.preview_token.is_empty());
            assert_eq!(state.csv_preview_count(), 1);
            revalidate_csv_preview_token(&state, &preview.preview_token).expect("fresh token");
            fs::write(&csv_path, b"changed").expect("mutate");
            assert!(revalidate_csv_preview_token(&state, &preview.preview_token).is_err());
            fs::write(
                &csv_path,
                activity_csv(&[&deposit_row(&account_id, &date, "January salary")]),
            )
            .expect("restore");
            let preview = preview_csv_import(
                &state,
                PreviewCsvImportInput {
                    source_path: csv_path.to_string_lossy().into_owned(),
                },
            )
            .await
            .expect("preview again");
            state.expire_csv_preview(&preview.preview_token);
            assert!(revalidate_csv_preview_token(&state, &preview.preview_token).is_err());
            drop(state);
            let _ = fs::remove_file(csv_path);
            cleanup(&root);
        });
    }
}
