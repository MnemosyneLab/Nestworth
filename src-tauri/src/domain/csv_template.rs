//! Versioned Nestworth CSV templates, spreadsheet hardening, and diagnostic codes.

use super::sustainable::{
    CanonicalImportRow, ExternalId, ImportField, ImportFingerprint, ImportTemplate, SourceNamespace,
};
use crate::error::AppError;

pub const MAX_CSV_IMPORT_ROWS: usize = 2_000;
pub const CSV_ESCAPED_COLUMN: &str = "escaped_for_spreadsheet";

pub const ACTIVITY_CSV_HEADERS: &[&str] = &[
    "source_namespace",
    "external_id",
    "kind",
    "effective_local_date",
    "effective_local_time",
    "ambiguous_offset",
    "account_id",
    "component_kind",
    "amount",
    "currency",
    "note",
    "account_label",
    "source_account_id",
    "source_account_label",
    "source_component",
    "source_amount",
    "source_currency",
    "destination_account_id",
    "destination_account_label",
    "destination_component",
    "destination_amount",
    "destination_currency",
    "holding_id",
    "instrument_id",
    "instrument_label",
    "quantity",
    "unit_price",
    "gross_amount",
    "settlement_currency",
    "fee_amount",
    "fee_kind",
    "income_kind",
    "liability_account_id",
    "principal_amount",
    "principal_currency",
    "cash_account_id",
    "cash_component",
    "cash_amount",
    "cash_currency",
    "fx_rate",
    "confirm_zero_unit_price",
    CSV_ESCAPED_COLUMN,
];

pub const QUOTE_CSV_HEADERS: &[&str] = &[
    "source_namespace",
    "external_id",
    "quote_kind",
    "instrument_id",
    "instrument_label",
    "base_currency",
    "quote_currency",
    "unit_price",
    "rate",
    "quoted_at",
    "note",
    CSV_ESCAPED_COLUMN,
];

pub const BENCHMARK_CSV_HEADERS: &[&str] = &[
    "source_namespace",
    "external_id",
    "benchmark_id",
    "benchmark_label",
    "observed_on",
    "level",
    "note",
    CSV_ESCAPED_COLUMN,
];

pub const DIAGNOSTIC_TEMPLATE_INVALID: &str = "CSV_TEMPLATE_INVALID";
pub const DIAGNOSTIC_HEADER_UNKNOWN: &str = "CSV_HEADER_UNKNOWN";
pub const DIAGNOSTIC_HEADER_MISSING: &str = "CSV_HEADER_MISSING";
pub const DIAGNOSTIC_HEADER_DUPLICATE: &str = "CSV_HEADER_DUPLICATE";
pub const DIAGNOSTIC_UTF8_INVALID: &str = "CSV_UTF8_INVALID";
pub const DIAGNOSTIC_NUL: &str = "CSV_NUL";
pub const DIAGNOSTIC_MALFORMED_QUOTE: &str = "CSV_MALFORMED_QUOTE";
pub const DIAGNOSTIC_ROW_LIMIT: &str = "CSV_ROW_LIMIT";
pub const DIAGNOSTIC_LOCALIZED_VALUE: &str = "CSV_LOCALIZED_VALUE";
pub const DIAGNOSTIC_KIND_FORBIDDEN: &str = "CSV_KIND_FORBIDDEN";
pub const DIAGNOSTIC_REFERENCE_MISSING: &str = "CSV_REFERENCE_MISSING";
pub const DIAGNOSTIC_REFERENCE_ARCHIVED: &str = "CSV_REFERENCE_ARCHIVED";
pub const DIAGNOSTIC_EXACT_DUPLICATE: &str = "CSV_EXACT_DUPLICATE";
pub const DIAGNOSTIC_DUPLICATE_CONFLICT: &str = "CSV_DUPLICATE_CONFLICT";
pub const DIAGNOSTIC_NO_IDENTITY_WARNING: &str = "CSV_NO_IDENTITY_WARNING";
pub const DIAGNOSTIC_DOMAIN_INVALID: &str = "CSV_DOMAIN_INVALID";

impl ImportTemplate {
    #[must_use]
    pub fn headers(self) -> &'static [&'static str] {
        match self {
            Self::ActivityV1 => ACTIVITY_CSV_HEADERS,
            Self::QuoteV1 => QUOTE_CSV_HEADERS,
            Self::BenchmarkV1 => BENCHMARK_CSV_HEADERS,
        }
    }
}

#[must_use]
pub fn needs_spreadsheet_hardening(value: &str) -> bool {
    matches!(
        value.trim_start_matches(' ').chars().next(),
        Some('=' | '+' | '-' | '@' | '\t' | '\r')
    )
}

#[must_use]
pub fn harden_spreadsheet_text(value: &str) -> (String, bool) {
    if needs_spreadsheet_hardening(value) {
        (format!("'{value}"), true)
    } else {
        (value.to_owned(), false)
    }
}

#[must_use]
pub fn unescape_spreadsheet_text(value: &str, escaped: bool) -> String {
    if !escaped {
        return value.to_owned();
    }
    match value.strip_prefix('\'') {
        Some(rest) if needs_spreadsheet_hardening(rest) => rest.to_owned(),
        _ => value.to_owned(),
    }
}

#[must_use]
pub fn looks_localized_decimal(value: &str) -> bool {
    value.contains(',') || value.contains(' ') || value.contains('\u{00a0}')
}

#[must_use]
pub fn looks_localized_date(value: &str) -> bool {
    value.contains('/') || value.contains('.')
}

#[must_use]
pub fn looks_localized_boolean(value: &str) -> bool {
    matches!(
        value,
        "TRUE" | "FALSE" | "True" | "False" | "yes" | "no" | "YES" | "NO" | "1" | "0"
    )
}

pub fn parse_strict_boolean(field: &str, value: &str) -> Result<bool, AppError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ if looks_localized_boolean(value) => Err(AppError::invalid_import_row(
            "CSV booleans must be lowercase true or false.",
        )),
        _ => Err(AppError::validation(
            field,
            "CSV booleans must be lowercase true or false.",
        )),
    }
}

pub fn optional_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

pub fn required_text(field: &str, value: &str) -> Result<String, AppError> {
    optional_text(value).ok_or_else(|| AppError::validation(field, "This field is required."))
}

pub fn parse_optional_namespace(value: &str) -> Result<Option<SourceNamespace>, AppError> {
    match optional_text(value) {
        None => Ok(None),
        Some(value) => SourceNamespace::parse(&value).map(Some),
    }
}

pub fn parse_optional_external_id(value: &str) -> Result<Option<ExternalId>, AppError> {
    match optional_text(value) {
        None => Ok(None),
        Some(value) => ExternalId::parse(&value).map(Some),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn activity_fingerprint(
    namespace: Option<&SourceNamespace>,
    external_id: Option<&ExternalId>,
    kind: &str,
    effective_local_date: &str,
    effective_local_time: &str,
    ambiguous_offset: Option<&str>,
    account_id: Option<&str>,
    component_kind: Option<&str>,
    amount: Option<&str>,
    currency: Option<&str>,
    note: Option<&str>,
    extras: &[ImportField],
) -> Result<ImportFingerprint, AppError> {
    let mut fields = vec![
        ImportField::Text("activity".to_owned()),
        optional_owned(namespace.map(SourceNamespace::as_str)),
        optional_owned(external_id.map(ExternalId::as_str)),
        ImportField::Text(kind.to_owned()),
        ImportField::Text(effective_local_date.to_owned()),
        ImportField::Text(effective_local_time.to_owned()),
        optional_owned(ambiguous_offset),
        optional_owned(account_id),
        optional_owned(component_kind),
        optional_owned(amount),
        optional_owned(currency),
        optional_owned(note),
    ];
    if kind != "deposit" && kind != "withdrawal" {
        fields.extend_from_slice(extras);
    }
    CanonicalImportRow::new(ImportTemplate::ActivityV1, fields)?.fingerprint()
}

#[allow(clippy::too_many_arguments)]
pub fn quote_fingerprint(
    namespace: Option<&SourceNamespace>,
    external_id: Option<&ExternalId>,
    quote_kind: &str,
    instrument_id: Option<&str>,
    unit_price: Option<&str>,
    base_currency: Option<&str>,
    quote_currency: Option<&str>,
    rate: Option<&str>,
    quoted_at: Option<&str>,
    note: Option<&str>,
) -> Result<ImportFingerprint, AppError> {
    CanonicalImportRow::new(
        ImportTemplate::QuoteV1,
        vec![
            ImportField::Text("quote".to_owned()),
            optional_owned(namespace.map(SourceNamespace::as_str)),
            optional_owned(external_id.map(ExternalId::as_str)),
            ImportField::Text(quote_kind.to_owned()),
            optional_owned(instrument_id),
            optional_owned(unit_price),
            optional_owned(base_currency),
            optional_owned(quote_currency),
            optional_owned(rate),
            optional_owned(quoted_at),
            optional_owned(note),
        ],
    )?
    .fingerprint()
}

pub fn benchmark_fingerprint(
    namespace: Option<&SourceNamespace>,
    external_id: Option<&ExternalId>,
    benchmark_id: &str,
    observed_on: &str,
    level: &str,
    note: Option<&str>,
) -> Result<ImportFingerprint, AppError> {
    CanonicalImportRow::new(
        ImportTemplate::BenchmarkV1,
        vec![
            ImportField::Text("benchmark".to_owned()),
            optional_owned(namespace.map(SourceNamespace::as_str)),
            optional_owned(external_id.map(ExternalId::as_str)),
            ImportField::Text(benchmark_id.to_owned()),
            ImportField::Text(observed_on.to_owned()),
            ImportField::Text(level.to_owned()),
            optional_owned(note),
        ],
    )?
    .fingerprint()
}

fn optional_owned(value: Option<&str>) -> ImportField {
    value.map_or(ImportField::Missing, |value| {
        ImportField::Text(value.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreadsheet_hardening_round_trips_formula_text() {
        let (hard, flagged) = harden_spreadsheet_text("=SUM(A1)");
        assert_eq!(hard, "'=SUM(A1)");
        assert!(flagged);
        assert_eq!(unescape_spreadsheet_text(&hard, true), "=SUM(A1)");
        assert_eq!(unescape_spreadsheet_text("'keep", false), "'keep");
        assert!(!needs_spreadsheet_hardening("January salary"));
        assert!(needs_spreadsheet_hardening("-paid"));
    }

    #[test]
    fn template_headers_are_exact_and_ordered() {
        assert_eq!(ACTIVITY_CSV_HEADERS[0], "source_namespace");
        assert_eq!(ACTIVITY_CSV_HEADERS[11], "account_label");
        assert_eq!(
            *ACTIVITY_CSV_HEADERS.last().expect("headers"),
            CSV_ESCAPED_COLUMN
        );
        assert_eq!(QUOTE_CSV_HEADERS[2], "quote_kind");
        assert_eq!(BENCHMARK_CSV_HEADERS[2], "benchmark_id");
    }
}
