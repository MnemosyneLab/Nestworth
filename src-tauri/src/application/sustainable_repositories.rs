use sqlx::{Row, Sqlite, Transaction};

use super::{
    query_count,
    reference::{map_read_error, map_unique_or_write, map_write_error},
};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RulePayloadRecord {
    pub endpoint_account_id: Option<String>,
    pub endpoint_component: Option<String>,
    pub amount: Option<String>,
    pub currency: Option<String>,
    pub source_account_id: Option<String>,
    pub source_component: Option<String>,
    pub source_amount: Option<String>,
    pub source_currency: Option<String>,
    pub destination_account_id: Option<String>,
    pub destination_component: Option<String>,
    pub destination_amount: Option<String>,
    pub destination_currency: Option<String>,
    pub fee_amount: Option<String>,
    pub fee_currency: Option<String>,
    pub fee_kind: Option<String>,
    pub income_kind: Option<String>,
    pub related_instrument_id: Option<String>,
    pub liability_account_id: Option<String>,
    pub principal_amount: Option<String>,
    pub principal_currency: Option<String>,
    pub cash_account_id: Option<String>,
    pub cash_component: Option<String>,
    pub cash_amount: Option<String>,
    pub cash_currency: Option<String>,
    pub fx_rate: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PendingPayloadRecord {
    pub endpoint_account_id: Option<String>,
    pub endpoint_component: Option<String>,
    pub amount: Option<String>,
    pub currency: Option<String>,
    pub source_account_id: Option<String>,
    pub source_component: Option<String>,
    pub source_amount: Option<String>,
    pub source_currency: Option<String>,
    pub destination_account_id: Option<String>,
    pub destination_component: Option<String>,
    pub destination_amount: Option<String>,
    pub destination_currency: Option<String>,
    pub fee_amount: Option<String>,
    pub fee_currency: Option<String>,
    pub fee_kind: Option<String>,
    pub income_kind: Option<String>,
    pub related_instrument_id: Option<String>,
    pub source_holding_id: Option<String>,
    pub source_instrument_id: Option<String>,
    pub destination_holding_id: Option<String>,
    pub destination_instrument_id: Option<String>,
    pub quantity: Option<String>,
    pub holding_id: Option<String>,
    pub instrument_id: Option<String>,
    pub unit_price: Option<String>,
    pub gross_amount: Option<String>,
    pub gross_currency: Option<String>,
    pub confirm_zero_unit_price: bool,
    pub liability_account_id: Option<String>,
    pub principal_amount: Option<String>,
    pub principal_currency: Option<String>,
    pub cash_account_id: Option<String>,
    pub cash_component: Option<String>,
    pub cash_amount: Option<String>,
    pub cash_currency: Option<String>,
    pub fx_rate: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecurringActivityRuleRecord {
    pub id: String,
    pub household_id: String,
    pub cadence: String,
    pub interval_value: i64,
    pub start_local_date: String,
    pub end_local_date: Option<String>,
    pub anchor_local_date: String,
    pub kind: String,
    pub payload: RulePayloadRecord,
    pub note: Option<String>,
    pub revision: i64,
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingActivityRecord {
    pub id: String,
    pub household_id: String,
    pub recurring_rule_id: Option<String>,
    pub recurring_rule_revision: Option<i64>,
    pub scheduled_local_date: String,
    pub creation_source: String,
    pub kind: String,
    pub payload: PendingPayloadRecord,
    pub note: Option<String>,
    pub status: String,
    pub posted_activity_id: Option<String>,
    pub skipped_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreshnessPolicyRecord {
    pub id: String,
    pub household_id: String,
    pub kind: String,
    pub target_account_id: Option<String>,
    pub target_instrument_id: Option<String>,
    pub target_currency_a: Option<String>,
    pub target_currency_b: Option<String>,
    pub review_interval_days: Option<i64>,
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceSnoozeRecord {
    pub id: String,
    pub household_id: String,
    pub policy_kind: String,
    pub target_account_id: Option<String>,
    pub target_instrument_id: Option<String>,
    pub target_currency_a: Option<String>,
    pub target_currency_b: Option<String>,
    pub snoozed_until: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBatchRecord {
    pub id: String,
    pub household_id: String,
    pub template: String,
    pub file_sha256: String,
    pub source_namespace: Option<String>,
    pub row_count: i64,
    pub committed_count: i64,
    pub duplicate_count: i64,
    pub rejected_count: i64,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportItemRecord {
    pub id: String,
    pub batch_id: String,
    pub row_number: i64,
    pub source_namespace: Option<String>,
    pub external_id: Option<String>,
    pub fingerprint: String,
    pub outcome: String,
    pub diagnostic_code: Option<String>,
    pub activity_id: Option<String>,
    pub instrument_quote_id: Option<String>,
    pub fx_quote_id: Option<String>,
    pub benchmark_observation_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkRecord {
    pub id: String,
    pub household_id: String,
    pub name: String,
    pub currency: String,
    pub series_kind: String,
    pub max_carry_days: i64,
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkObservationRecord {
    pub id: String,
    pub benchmark_id: String,
    pub level: String,
    pub observed_on: String,
    pub note: Option<String>,
    pub source_kind: String,
    pub import_item_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkPreferenceRecord {
    pub household_id: String,
    pub benchmark_id: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingActivityCursor {
    pub scheduled_local_date: String,
    pub created_at: String,
    pub id: String,
}

fn required_text(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<String, AppError> {
    row.try_get(column)
        .map_err(|_| AppError::DatabaseUnavailable)
}

fn optional_text(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<String>, AppError> {
    row.try_get(column)
        .map_err(|_| AppError::DatabaseUnavailable)
}

fn required_i64(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<i64, AppError> {
    row.try_get(column)
        .map_err(|_| AppError::DatabaseUnavailable)
}

fn optional_i64(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<i64>, AppError> {
    row.try_get(column)
        .map_err(|_| AppError::DatabaseUnavailable)
}

fn flag(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<bool, AppError> {
    Ok(required_i64(row, column)? != 0)
}

fn rule_payload(row: &sqlx::sqlite::SqliteRow) -> Result<RulePayloadRecord, AppError> {
    Ok(RulePayloadRecord {
        endpoint_account_id: optional_text(row, "endpoint_account_id")?,
        endpoint_component: optional_text(row, "endpoint_component")?,
        amount: optional_text(row, "amount")?,
        currency: optional_text(row, "currency")?,
        source_account_id: optional_text(row, "source_account_id")?,
        source_component: optional_text(row, "source_component")?,
        source_amount: optional_text(row, "source_amount")?,
        source_currency: optional_text(row, "source_currency")?,
        destination_account_id: optional_text(row, "destination_account_id")?,
        destination_component: optional_text(row, "destination_component")?,
        destination_amount: optional_text(row, "destination_amount")?,
        destination_currency: optional_text(row, "destination_currency")?,
        fee_amount: optional_text(row, "fee_amount")?,
        fee_currency: optional_text(row, "fee_currency")?,
        fee_kind: optional_text(row, "fee_kind")?,
        income_kind: optional_text(row, "income_kind")?,
        related_instrument_id: optional_text(row, "related_instrument_id")?,
        liability_account_id: optional_text(row, "liability_account_id")?,
        principal_amount: optional_text(row, "principal_amount")?,
        principal_currency: optional_text(row, "principal_currency")?,
        cash_account_id: optional_text(row, "cash_account_id")?,
        cash_component: optional_text(row, "cash_component")?,
        cash_amount: optional_text(row, "cash_amount")?,
        cash_currency: optional_text(row, "cash_currency")?,
        fx_rate: optional_text(row, "fx_rate")?,
    })
}

fn pending_payload(row: &sqlx::sqlite::SqliteRow) -> Result<PendingPayloadRecord, AppError> {
    Ok(PendingPayloadRecord {
        endpoint_account_id: optional_text(row, "endpoint_account_id")?,
        endpoint_component: optional_text(row, "endpoint_component")?,
        amount: optional_text(row, "amount")?,
        currency: optional_text(row, "currency")?,
        source_account_id: optional_text(row, "source_account_id")?,
        source_component: optional_text(row, "source_component")?,
        source_amount: optional_text(row, "source_amount")?,
        source_currency: optional_text(row, "source_currency")?,
        destination_account_id: optional_text(row, "destination_account_id")?,
        destination_component: optional_text(row, "destination_component")?,
        destination_amount: optional_text(row, "destination_amount")?,
        destination_currency: optional_text(row, "destination_currency")?,
        fee_amount: optional_text(row, "fee_amount")?,
        fee_currency: optional_text(row, "fee_currency")?,
        fee_kind: optional_text(row, "fee_kind")?,
        income_kind: optional_text(row, "income_kind")?,
        related_instrument_id: optional_text(row, "related_instrument_id")?,
        source_holding_id: optional_text(row, "source_holding_id")?,
        source_instrument_id: optional_text(row, "source_instrument_id")?,
        destination_holding_id: optional_text(row, "destination_holding_id")?,
        destination_instrument_id: optional_text(row, "destination_instrument_id")?,
        quantity: optional_text(row, "quantity")?,
        holding_id: optional_text(row, "holding_id")?,
        instrument_id: optional_text(row, "instrument_id")?,
        unit_price: optional_text(row, "unit_price")?,
        gross_amount: optional_text(row, "gross_amount")?,
        gross_currency: optional_text(row, "gross_currency")?,
        confirm_zero_unit_price: flag(row, "confirm_zero_unit_price")?,
        liability_account_id: optional_text(row, "liability_account_id")?,
        principal_amount: optional_text(row, "principal_amount")?,
        principal_currency: optional_text(row, "principal_currency")?,
        cash_account_id: optional_text(row, "cash_account_id")?,
        cash_component: optional_text(row, "cash_component")?,
        cash_amount: optional_text(row, "cash_amount")?,
        cash_currency: optional_text(row, "cash_currency")?,
        fx_rate: optional_text(row, "fx_rate")?,
    })
}

fn rule_from_row(row: sqlx::sqlite::SqliteRow) -> Result<RecurringActivityRuleRecord, AppError> {
    Ok(RecurringActivityRuleRecord {
        id: required_text(&row, "id")?,
        household_id: required_text(&row, "household_id")?,
        cadence: required_text(&row, "cadence")?,
        interval_value: required_i64(&row, "interval_value")?,
        start_local_date: required_text(&row, "start_local_date")?,
        end_local_date: optional_text(&row, "end_local_date")?,
        anchor_local_date: required_text(&row, "anchor_local_date")?,
        kind: required_text(&row, "kind")?,
        payload: rule_payload(&row)?,
        note: optional_text(&row, "note")?,
        revision: required_i64(&row, "revision")?,
        archived_at: optional_text(&row, "archived_at")?,
        created_at: required_text(&row, "created_at")?,
        updated_at: required_text(&row, "updated_at")?,
    })
}

fn pending_from_row(row: sqlx::sqlite::SqliteRow) -> Result<PendingActivityRecord, AppError> {
    Ok(PendingActivityRecord {
        id: required_text(&row, "id")?,
        household_id: required_text(&row, "household_id")?,
        recurring_rule_id: optional_text(&row, "recurring_rule_id")?,
        recurring_rule_revision: optional_i64(&row, "recurring_rule_revision")?,
        scheduled_local_date: required_text(&row, "scheduled_local_date")?,
        creation_source: required_text(&row, "creation_source")?,
        kind: required_text(&row, "kind")?,
        payload: pending_payload(&row)?,
        note: optional_text(&row, "note")?,
        status: required_text(&row, "status")?,
        posted_activity_id: optional_text(&row, "posted_activity_id")?,
        skipped_at: optional_text(&row, "skipped_at")?,
        created_at: required_text(&row, "created_at")?,
        updated_at: required_text(&row, "updated_at")?,
    })
}

fn policy_from_row(row: sqlx::sqlite::SqliteRow) -> Result<FreshnessPolicyRecord, AppError> {
    Ok(FreshnessPolicyRecord {
        id: required_text(&row, "id")?,
        household_id: required_text(&row, "household_id")?,
        kind: required_text(&row, "kind")?,
        target_account_id: optional_text(&row, "target_account_id")?,
        target_instrument_id: optional_text(&row, "target_instrument_id")?,
        target_currency_a: optional_text(&row, "target_currency_a")?,
        target_currency_b: optional_text(&row, "target_currency_b")?,
        review_interval_days: optional_i64(&row, "review_interval_days")?,
        archived_at: optional_text(&row, "archived_at")?,
        created_at: required_text(&row, "created_at")?,
        updated_at: required_text(&row, "updated_at")?,
    })
}

fn snooze_from_row(row: sqlx::sqlite::SqliteRow) -> Result<MaintenanceSnoozeRecord, AppError> {
    Ok(MaintenanceSnoozeRecord {
        id: required_text(&row, "id")?,
        household_id: required_text(&row, "household_id")?,
        policy_kind: required_text(&row, "policy_kind")?,
        target_account_id: optional_text(&row, "target_account_id")?,
        target_instrument_id: optional_text(&row, "target_instrument_id")?,
        target_currency_a: optional_text(&row, "target_currency_a")?,
        target_currency_b: optional_text(&row, "target_currency_b")?,
        snoozed_until: required_text(&row, "snoozed_until")?,
        created_at: required_text(&row, "created_at")?,
    })
}

fn batch_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ImportBatchRecord, AppError> {
    Ok(ImportBatchRecord {
        id: required_text(&row, "id")?,
        household_id: required_text(&row, "household_id")?,
        template: required_text(&row, "template")?,
        file_sha256: required_text(&row, "file_sha256")?,
        source_namespace: optional_text(&row, "source_namespace")?,
        row_count: required_i64(&row, "row_count")?,
        committed_count: required_i64(&row, "committed_count")?,
        duplicate_count: required_i64(&row, "duplicate_count")?,
        rejected_count: required_i64(&row, "rejected_count")?,
        status: required_text(&row, "status")?,
        created_at: required_text(&row, "created_at")?,
        completed_at: optional_text(&row, "completed_at")?,
    })
}

fn item_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ImportItemRecord, AppError> {
    Ok(ImportItemRecord {
        id: required_text(&row, "id")?,
        batch_id: required_text(&row, "batch_id")?,
        row_number: required_i64(&row, "row_number")?,
        source_namespace: optional_text(&row, "source_namespace")?,
        external_id: optional_text(&row, "external_id")?,
        fingerprint: required_text(&row, "fingerprint")?,
        outcome: required_text(&row, "outcome")?,
        diagnostic_code: optional_text(&row, "diagnostic_code")?,
        activity_id: optional_text(&row, "activity_id")?,
        instrument_quote_id: optional_text(&row, "instrument_quote_id")?,
        fx_quote_id: optional_text(&row, "fx_quote_id")?,
        benchmark_observation_id: optional_text(&row, "benchmark_observation_id")?,
        created_at: required_text(&row, "created_at")?,
    })
}

fn benchmark_from_row(row: sqlx::sqlite::SqliteRow) -> Result<BenchmarkRecord, AppError> {
    Ok(BenchmarkRecord {
        id: required_text(&row, "id")?,
        household_id: required_text(&row, "household_id")?,
        name: required_text(&row, "name")?,
        currency: required_text(&row, "currency")?,
        series_kind: required_text(&row, "series_kind")?,
        max_carry_days: required_i64(&row, "max_carry_days")?,
        archived_at: optional_text(&row, "archived_at")?,
        created_at: required_text(&row, "created_at")?,
        updated_at: required_text(&row, "updated_at")?,
    })
}

fn observation_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<BenchmarkObservationRecord, AppError> {
    Ok(BenchmarkObservationRecord {
        id: required_text(&row, "id")?,
        benchmark_id: required_text(&row, "benchmark_id")?,
        level: required_text(&row, "level")?,
        observed_on: required_text(&row, "observed_on")?,
        note: optional_text(&row, "note")?,
        source_kind: required_text(&row, "source_kind")?,
        import_item_id: optional_text(&row, "import_item_id")?,
        created_at: required_text(&row, "created_at")?,
    })
}

pub async fn list_recurring_activity_rules(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    include_archived: bool,
) -> Result<Vec<RecurringActivityRuleRecord>, AppError> {
    query_count::record("sustainable.rule_list");
    let archived_filter = if include_archived {
        ""
    } else {
        " AND archived_at IS NULL"
    };
    let sql = format!(
        "SELECT id, household_id, cadence, interval_value, start_local_date, end_local_date,
                anchor_local_date, kind, endpoint_account_id, endpoint_component, amount, currency,
                source_account_id, source_component, source_amount, source_currency,
                destination_account_id, destination_component, destination_amount, destination_currency,
                fee_amount, fee_currency, fee_kind, income_kind, related_instrument_id,
                liability_account_id, principal_amount, principal_currency, cash_account_id,
                cash_component, cash_amount, cash_currency, fx_rate, note, revision, archived_at,
                created_at, updated_at
         FROM recurring_activity_rules
         WHERE household_id = ?{archived_filter}
         ORDER BY archived_at IS NOT NULL ASC, start_local_date ASC, created_at ASC, id ASC"
    );
    sqlx::query(&sql)
        .bind(household_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("sustainable.rule_list_failed", error))?
        .into_iter()
        .map(rule_from_row)
        .collect()
}

pub async fn get_recurring_activity_rule(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    rule_id: &str,
) -> Result<Option<RecurringActivityRuleRecord>, AppError> {
    query_count::record("sustainable.rule_get");
    sqlx::query(
        "SELECT id, household_id, cadence, interval_value, start_local_date, end_local_date,
                anchor_local_date, kind, endpoint_account_id, endpoint_component, amount, currency,
                source_account_id, source_component, source_amount, source_currency,
                destination_account_id, destination_component, destination_amount, destination_currency,
                fee_amount, fee_currency, fee_kind, income_kind, related_instrument_id,
                liability_account_id, principal_amount, principal_currency, cash_account_id,
                cash_component, cash_amount, cash_currency, fx_rate, note, revision, archived_at,
                created_at, updated_at
         FROM recurring_activity_rules
         WHERE household_id = ? AND id = ?",
    )
    .bind(household_id)
    .bind(rule_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("sustainable.rule_get_failed", error))?
    .map(rule_from_row)
    .transpose()
}

pub async fn insert_recurring_activity_rule(
    tx: &mut Transaction<'_, Sqlite>,
    row: &RecurringActivityRuleRecord,
) -> Result<(), AppError> {
    query_count::record("sustainable.rule_insert");
    let payload = &row.payload;
    sqlx::query(
        "INSERT INTO recurring_activity_rules (
            id, household_id, cadence, interval_value, start_local_date, end_local_date,
            anchor_local_date, kind, endpoint_account_id, endpoint_component, amount, currency,
            source_account_id, source_component, source_amount, source_currency,
            destination_account_id, destination_component, destination_amount, destination_currency,
            fee_amount, fee_currency, fee_kind, income_kind, related_instrument_id,
            liability_account_id, principal_amount, principal_currency, cash_account_id,
            cash_component, cash_amount, cash_currency, fx_rate, note, revision, archived_at,
            created_at, updated_at
         ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
         )",
    )
    .bind(&row.id)
    .bind(&row.household_id)
    .bind(&row.cadence)
    .bind(row.interval_value)
    .bind(&row.start_local_date)
    .bind(&row.end_local_date)
    .bind(&row.anchor_local_date)
    .bind(&row.kind)
    .bind(&payload.endpoint_account_id)
    .bind(&payload.endpoint_component)
    .bind(&payload.amount)
    .bind(&payload.currency)
    .bind(&payload.source_account_id)
    .bind(&payload.source_component)
    .bind(&payload.source_amount)
    .bind(&payload.source_currency)
    .bind(&payload.destination_account_id)
    .bind(&payload.destination_component)
    .bind(&payload.destination_amount)
    .bind(&payload.destination_currency)
    .bind(&payload.fee_amount)
    .bind(&payload.fee_currency)
    .bind(&payload.fee_kind)
    .bind(&payload.income_kind)
    .bind(&payload.related_instrument_id)
    .bind(&payload.liability_account_id)
    .bind(&payload.principal_amount)
    .bind(&payload.principal_currency)
    .bind(&payload.cash_account_id)
    .bind(&payload.cash_component)
    .bind(&payload.cash_amount)
    .bind(&payload.cash_currency)
    .bind(&payload.fx_rate)
    .bind(&row.note)
    .bind(row.revision)
    .bind(&row.archived_at)
    .bind(&row.created_at)
    .bind(&row.updated_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("sustainable.rule_insert_failed", error))?;
    Ok(())
}

pub async fn list_pending_activities(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    status: Option<&str>,
    cursor: Option<&PendingActivityCursor>,
    limit: i64,
) -> Result<Vec<PendingActivityRecord>, AppError> {
    let limit = limit.clamp(1, 200);
    query_count::record("sustainable.pending_list");
    let (status_sql, cursor_sql) = (
        status.map_or("".to_owned(), |_| " AND status = ?".to_owned()),
        cursor.map_or("".to_owned(), |_| {
            " AND (scheduled_local_date > ? OR (scheduled_local_date = ? AND (created_at > ? OR (created_at = ? AND id > ?))))".to_owned()
        }),
    );
    let sql = format!(
        "SELECT id, household_id, recurring_rule_id, recurring_rule_revision,
                scheduled_local_date, creation_source, kind, endpoint_account_id,
                endpoint_component, amount, currency, source_account_id, source_component,
                source_amount, source_currency, destination_account_id, destination_component,
                destination_amount, destination_currency, fee_amount, fee_currency, fee_kind,
                income_kind, related_instrument_id, source_holding_id, source_instrument_id,
                destination_holding_id, destination_instrument_id, quantity, holding_id,
                instrument_id, unit_price, gross_amount, gross_currency, confirm_zero_unit_price,
                liability_account_id, principal_amount, principal_currency, cash_account_id,
                cash_component, cash_amount, cash_currency, fx_rate, note, status,
                posted_activity_id, skipped_at, created_at, updated_at
         FROM pending_activities
         WHERE household_id = ?{status_sql}{cursor_sql}
         ORDER BY scheduled_local_date ASC, created_at ASC, id ASC
         LIMIT ?"
    );
    let mut query = sqlx::query(&sql).bind(household_id);
    if let Some(status) = status {
        query = query.bind(status);
    }
    if let Some(cursor) = cursor {
        query = query
            .bind(&cursor.scheduled_local_date)
            .bind(&cursor.scheduled_local_date)
            .bind(&cursor.created_at)
            .bind(&cursor.created_at)
            .bind(&cursor.id);
    }
    query
        .bind(limit)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("sustainable.pending_list_failed", error))?
        .into_iter()
        .map(pending_from_row)
        .collect()
}

pub async fn get_pending_activity(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    pending_id: &str,
) -> Result<Option<PendingActivityRecord>, AppError> {
    query_count::record("sustainable.pending_get");
    sqlx::query(
        "SELECT id, household_id, recurring_rule_id, recurring_rule_revision,
                scheduled_local_date, creation_source, kind, endpoint_account_id,
                endpoint_component, amount, currency, source_account_id, source_component,
                source_amount, source_currency, destination_account_id, destination_component,
                destination_amount, destination_currency, fee_amount, fee_currency, fee_kind,
                income_kind, related_instrument_id, source_holding_id, source_instrument_id,
                destination_holding_id, destination_instrument_id, quantity, holding_id,
                instrument_id, unit_price, gross_amount, gross_currency, confirm_zero_unit_price,
                liability_account_id, principal_amount, principal_currency, cash_account_id,
                cash_component, cash_amount, cash_currency, fx_rate, note, status,
                posted_activity_id, skipped_at, created_at, updated_at
         FROM pending_activities
         WHERE household_id = ? AND id = ?",
    )
    .bind(household_id)
    .bind(pending_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("sustainable.pending_get_failed", error))?
    .map(pending_from_row)
    .transpose()
}

pub async fn insert_pending_activity(
    tx: &mut Transaction<'_, Sqlite>,
    row: &PendingActivityRecord,
) -> Result<(), AppError> {
    query_count::record("sustainable.pending_insert");
    let payload = &row.payload;
    sqlx::query(
        "INSERT INTO pending_activities (
            id, household_id, recurring_rule_id, recurring_rule_revision, scheduled_local_date,
            creation_source, kind, endpoint_account_id, endpoint_component, amount, currency,
            source_account_id, source_component, source_amount, source_currency,
            destination_account_id, destination_component, destination_amount, destination_currency,
            fee_amount, fee_currency, fee_kind, income_kind, related_instrument_id,
            source_holding_id, source_instrument_id, destination_holding_id, destination_instrument_id,
            quantity, holding_id, instrument_id, unit_price, gross_amount, gross_currency,
            confirm_zero_unit_price, liability_account_id, principal_amount, principal_currency,
            cash_account_id, cash_component, cash_amount, cash_currency, fx_rate, note, status,
            posted_activity_id, skipped_at, created_at, updated_at
         ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
         )",
    )
    .bind(&row.id)
    .bind(&row.household_id)
    .bind(&row.recurring_rule_id)
    .bind(row.recurring_rule_revision)
    .bind(&row.scheduled_local_date)
    .bind(&row.creation_source)
    .bind(&row.kind)
    .bind(&payload.endpoint_account_id)
    .bind(&payload.endpoint_component)
    .bind(&payload.amount)
    .bind(&payload.currency)
    .bind(&payload.source_account_id)
    .bind(&payload.source_component)
    .bind(&payload.source_amount)
    .bind(&payload.source_currency)
    .bind(&payload.destination_account_id)
    .bind(&payload.destination_component)
    .bind(&payload.destination_amount)
    .bind(&payload.destination_currency)
    .bind(&payload.fee_amount)
    .bind(&payload.fee_currency)
    .bind(&payload.fee_kind)
    .bind(&payload.income_kind)
    .bind(&payload.related_instrument_id)
    .bind(&payload.source_holding_id)
    .bind(&payload.source_instrument_id)
    .bind(&payload.destination_holding_id)
    .bind(&payload.destination_instrument_id)
    .bind(&payload.quantity)
    .bind(&payload.holding_id)
    .bind(&payload.instrument_id)
    .bind(&payload.unit_price)
    .bind(&payload.gross_amount)
    .bind(&payload.gross_currency)
    .bind(i64::from(payload.confirm_zero_unit_price))
    .bind(&payload.liability_account_id)
    .bind(&payload.principal_amount)
    .bind(&payload.principal_currency)
    .bind(&payload.cash_account_id)
    .bind(&payload.cash_component)
    .bind(&payload.cash_amount)
    .bind(&payload.cash_currency)
    .bind(&payload.fx_rate)
    .bind(&row.note)
    .bind(&row.status)
    .bind(&row.posted_activity_id)
    .bind(&row.skipped_at)
    .bind(&row.created_at)
    .bind(&row.updated_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("sustainable.pending_insert_failed", error))?;
    Ok(())
}

pub async fn list_freshness_policies(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    include_archived: bool,
) -> Result<Vec<FreshnessPolicyRecord>, AppError> {
    query_count::record("sustainable.policy_list");
    let archived_filter = if include_archived {
        ""
    } else {
        " AND archived_at IS NULL"
    };
    let sql = format!(
        "SELECT id, household_id, kind, target_account_id, target_instrument_id,
                target_currency_a, target_currency_b, review_interval_days, archived_at,
                created_at, updated_at
         FROM freshness_policies
         WHERE household_id = ?{archived_filter}
         ORDER BY kind ASC,
                  target_account_id IS NOT NULL ASC,
                  target_instrument_id IS NOT NULL ASC,
                  target_currency_a IS NOT NULL ASC,
                  target_account_id ASC, target_instrument_id ASC,
                  target_currency_a ASC, target_currency_b ASC, id ASC"
    );
    sqlx::query(&sql)
        .bind(household_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("sustainable.policy_list_failed", error))?
        .into_iter()
        .map(policy_from_row)
        .collect()
}

pub async fn insert_freshness_policy(
    tx: &mut Transaction<'_, Sqlite>,
    row: &FreshnessPolicyRecord,
) -> Result<(), AppError> {
    query_count::record("sustainable.policy_insert");
    sqlx::query(
        "INSERT INTO freshness_policies (
            id, household_id, kind, target_account_id, target_instrument_id,
            target_currency_a, target_currency_b, review_interval_days, archived_at,
            created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.household_id)
    .bind(&row.kind)
    .bind(&row.target_account_id)
    .bind(&row.target_instrument_id)
    .bind(&row.target_currency_a)
    .bind(&row.target_currency_b)
    .bind(row.review_interval_days)
    .bind(&row.archived_at)
    .bind(&row.created_at)
    .bind(&row.updated_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| {
        map_unique_or_write(
            "sustainable.policy_insert_failed",
            error,
            AppError::conflict("A freshness policy already exists for this target."),
        )
    })?;
    Ok(())
}

pub async fn list_maintenance_snoozes(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Vec<MaintenanceSnoozeRecord>, AppError> {
    query_count::record("sustainable.snooze_list");
    sqlx::query(
        "SELECT id, household_id, policy_kind, target_account_id, target_instrument_id,
                target_currency_a, target_currency_b, snoozed_until, created_at
         FROM maintenance_snoozes
         WHERE household_id = ?
         ORDER BY snoozed_until DESC, created_at DESC, id DESC",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("sustainable.snooze_list_failed", error))?
    .into_iter()
    .map(snooze_from_row)
    .collect()
}

pub async fn insert_maintenance_snooze(
    tx: &mut Transaction<'_, Sqlite>,
    row: &MaintenanceSnoozeRecord,
) -> Result<(), AppError> {
    query_count::record("sustainable.snooze_insert");
    sqlx::query(
        "INSERT INTO maintenance_snoozes (
            id, household_id, policy_kind, target_account_id, target_instrument_id,
            target_currency_a, target_currency_b, snoozed_until, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.household_id)
    .bind(&row.policy_kind)
    .bind(&row.target_account_id)
    .bind(&row.target_instrument_id)
    .bind(&row.target_currency_a)
    .bind(&row.target_currency_b)
    .bind(&row.snoozed_until)
    .bind(&row.created_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("sustainable.snooze_insert_failed", error))?;
    Ok(())
}

pub async fn list_import_batches(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    limit: i64,
) -> Result<Vec<ImportBatchRecord>, AppError> {
    query_count::record("sustainable.import_batch_list");
    sqlx::query(
        "SELECT id, household_id, template, file_sha256, source_namespace, row_count,
                committed_count, duplicate_count, rejected_count, status, created_at, completed_at
         FROM import_batches
         WHERE household_id = ?
         ORDER BY created_at DESC, id DESC
         LIMIT ?",
    )
    .bind(household_id)
    .bind(limit.clamp(1, 200))
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("sustainable.import_batch_list_failed", error))?
    .into_iter()
    .map(batch_from_row)
    .collect()
}

pub async fn insert_import_batch(
    tx: &mut Transaction<'_, Sqlite>,
    row: &ImportBatchRecord,
) -> Result<(), AppError> {
    query_count::record("sustainable.import_batch_insert");
    sqlx::query(
        "INSERT INTO import_batches (
            id, household_id, template, file_sha256, source_namespace, row_count,
            committed_count, duplicate_count, rejected_count, status, created_at, completed_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.household_id)
    .bind(&row.template)
    .bind(&row.file_sha256)
    .bind(&row.source_namespace)
    .bind(row.row_count)
    .bind(row.committed_count)
    .bind(row.duplicate_count)
    .bind(row.rejected_count)
    .bind(&row.status)
    .bind(&row.created_at)
    .bind(&row.completed_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("sustainable.import_batch_insert_failed", error))?;
    Ok(())
}

pub async fn list_import_items(
    tx: &mut Transaction<'_, Sqlite>,
    batch_id: &str,
) -> Result<Vec<ImportItemRecord>, AppError> {
    query_count::record("sustainable.import_item_list");
    sqlx::query(
        "SELECT id, batch_id, row_number, source_namespace, external_id, fingerprint,
                outcome, diagnostic_code, activity_id, instrument_quote_id, fx_quote_id,
                benchmark_observation_id, created_at
         FROM import_items
         WHERE batch_id = ?
         ORDER BY row_number ASC, id ASC",
    )
    .bind(batch_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("sustainable.import_item_list_failed", error))?
    .into_iter()
    .map(item_from_row)
    .collect()
}

pub async fn insert_import_item(
    tx: &mut Transaction<'_, Sqlite>,
    row: &ImportItemRecord,
) -> Result<(), AppError> {
    query_count::record("sustainable.import_item_insert");
    sqlx::query(
        "INSERT INTO import_items (
            id, batch_id, row_number, source_namespace, external_id, fingerprint,
            outcome, diagnostic_code, activity_id, instrument_quote_id, fx_quote_id,
            benchmark_observation_id, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.batch_id)
    .bind(row.row_number)
    .bind(&row.source_namespace)
    .bind(&row.external_id)
    .bind(&row.fingerprint)
    .bind(&row.outcome)
    .bind(&row.diagnostic_code)
    .bind(&row.activity_id)
    .bind(&row.instrument_quote_id)
    .bind(&row.fx_quote_id)
    .bind(&row.benchmark_observation_id)
    .bind(&row.created_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("sustainable.import_item_insert_failed", error))?;
    Ok(())
}

pub async fn find_import_identity(
    tx: &mut Transaction<'_, Sqlite>,
    source_namespace: &str,
    external_id: &str,
) -> Result<Vec<ImportItemRecord>, AppError> {
    query_count::record("sustainable.import_identity_lookup");
    sqlx::query(
        "SELECT id, batch_id, row_number, source_namespace, external_id, fingerprint,
                outcome, diagnostic_code, activity_id, instrument_quote_id, fx_quote_id,
                benchmark_observation_id, created_at
         FROM import_items
         WHERE source_namespace = ? AND external_id = ?
         ORDER BY created_at ASC, id ASC",
    )
    .bind(source_namespace)
    .bind(external_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("sustainable.import_identity_lookup_failed", error))?
    .into_iter()
    .map(item_from_row)
    .collect()
}

pub async fn list_benchmarks(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    include_archived: bool,
) -> Result<Vec<BenchmarkRecord>, AppError> {
    query_count::record("sustainable.benchmark_list");
    let archived_filter = if include_archived {
        ""
    } else {
        " AND archived_at IS NULL"
    };
    let sql = format!(
        "SELECT id, household_id, name, currency, series_kind, max_carry_days,
                archived_at, created_at, updated_at
         FROM benchmarks
         WHERE household_id = ?{archived_filter}
         ORDER BY archived_at IS NOT NULL ASC, name COLLATE NOCASE ASC, id ASC"
    );
    sqlx::query(&sql)
        .bind(household_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("sustainable.benchmark_list_failed", error))?
        .into_iter()
        .map(benchmark_from_row)
        .collect()
}

pub async fn insert_benchmark(
    tx: &mut Transaction<'_, Sqlite>,
    row: &BenchmarkRecord,
) -> Result<(), AppError> {
    query_count::record("sustainable.benchmark_insert");
    sqlx::query(
        "INSERT INTO benchmarks (
            id, household_id, name, currency, series_kind, max_carry_days,
            archived_at, created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.household_id)
    .bind(&row.name)
    .bind(&row.currency)
    .bind(&row.series_kind)
    .bind(row.max_carry_days)
    .bind(&row.archived_at)
    .bind(&row.created_at)
    .bind(&row.updated_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("sustainable.benchmark_insert_failed", error))?;
    Ok(())
}

pub async fn list_benchmark_observations(
    tx: &mut Transaction<'_, Sqlite>,
    benchmark_id: &str,
) -> Result<Vec<BenchmarkObservationRecord>, AppError> {
    query_count::record("sustainable.benchmark_observation_list");
    sqlx::query(
        "SELECT id, benchmark_id, level, observed_on, note, source_kind, import_item_id, created_at
         FROM benchmark_observations
         WHERE benchmark_id = ?
         ORDER BY observed_on DESC, created_at DESC, id DESC",
    )
    .bind(benchmark_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("sustainable.benchmark_observation_list_failed", error))?
    .into_iter()
    .map(observation_from_row)
    .collect()
}

pub async fn insert_benchmark_observation(
    tx: &mut Transaction<'_, Sqlite>,
    row: &BenchmarkObservationRecord,
) -> Result<(), AppError> {
    query_count::record("sustainable.benchmark_observation_insert");
    sqlx::query(
        "INSERT INTO benchmark_observations (
            id, benchmark_id, level, observed_on, note, source_kind, import_item_id, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.benchmark_id)
    .bind(&row.level)
    .bind(&row.observed_on)
    .bind(&row.note)
    .bind(&row.source_kind)
    .bind(&row.import_item_id)
    .bind(&row.created_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("sustainable.benchmark_observation_insert_failed", error))?;
    Ok(())
}

pub async fn get_benchmark_preference(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Option<BenchmarkPreferenceRecord>, AppError> {
    query_count::record("sustainable.benchmark_preference_get");
    sqlx::query(
        "SELECT household_id, benchmark_id, updated_at
         FROM household_benchmark_preferences
         WHERE household_id = ?",
    )
    .bind(household_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("sustainable.benchmark_preference_get_failed", error))?
    .map(|row| {
        Ok(BenchmarkPreferenceRecord {
            household_id: required_text(&row, "household_id")?,
            benchmark_id: required_text(&row, "benchmark_id")?,
            updated_at: required_text(&row, "updated_at")?,
        })
    })
    .transpose()
}

pub async fn set_benchmark_preference(
    tx: &mut Transaction<'_, Sqlite>,
    row: &BenchmarkPreferenceRecord,
) -> Result<(), AppError> {
    query_count::record("sustainable.benchmark_preference_set");
    sqlx::query(
        "INSERT INTO household_benchmark_preferences (household_id, benchmark_id, updated_at)
         VALUES (?, ?, ?)
         ON CONFLICT(household_id) DO UPDATE SET benchmark_id = excluded.benchmark_id, updated_at = excluded.updated_at",
    )
    .bind(&row.household_id)
    .bind(&row.benchmark_id)
    .bind(&row.updated_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("sustainable.benchmark_preference_set_failed", error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

    use super::*;
    use crate::{
        application::{
            onboarding_service::{CompleteOnboardingInput, OnboardingMemberInput},
            reference::finish_write_tx,
        },
        infrastructure::database_bootstrap::initialize_database,
        state::AppState,
    };

    fn test_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "nestworth-phase2-repository-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn cleanup(path: &PathBuf) {
        let _ = fs::remove_file(path);
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = path.as_os_str().to_os_string();
            sidecar.push(suffix);
            let _ = fs::remove_file(PathBuf::from(sidecar));
        }
    }

    #[test]
    fn repositories_preserve_ordering_and_append_only_provenance() {
        tauri::async_runtime::block_on(async {
            let path = test_path("ordering");
            cleanup(&path);
            let state = AppState::initialize(path.clone()).await;
            crate::application::onboarding_service::complete_onboarding(
                &state,
                CompleteOnboardingInput {
                    household_name: "Repository Household".to_owned(),
                    base_currency: "CNY".to_owned(),
                    members: vec![OnboardingMemberInput {
                        name: "Owner".to_owned(),
                    }],
                },
            )
            .await
            .expect("onboarding");

            let database = state.writable_db().expect("writable");
            let household_id: String = sqlx::query_scalar("SELECT id FROM households LIMIT 1")
                .fetch_one(database)
                .await
                .expect("household");
            sqlx::query(
                "INSERT INTO accounts (
                    id, household_id, name, primary_category, secondary_category,
                    tracking_mode, default_currency, created_at, updated_at
                 ) VALUES (?, ?, 'Repository Account', 'cash_equivalent', 'cash', 'balance', 'CNY', ?, ?)",
            )
            .bind("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
            .bind(&household_id)
            .bind("2026-08-20T00:00:00.000Z")
            .bind("2026-08-20T00:00:00.000Z")
            .execute(database)
            .await
            .expect("account");
            let mut tx = crate::application::reference::begin_write_tx(database)
                .await
                .expect("transaction");

            let policies = list_freshness_policies(&mut tx, &household_id, false)
                .await
                .expect("policies");
            assert_eq!(policies.len(), 4);
            assert_eq!(
                policies
                    .iter()
                    .map(|policy| policy.kind.as_str())
                    .collect::<Vec<_>>(),
                vec![
                    "account_cash",
                    "account_value",
                    "fx_quote",
                    "instrument_quote"
                ]
            );

            insert_recurring_activity_rule(
                &mut tx,
                &RecurringActivityRuleRecord {
                    id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd".to_owned(),
                    household_id: household_id.clone(),
                    cadence: "daily".to_owned(),
                    interval_value: 1,
                    start_local_date: "2026-08-20".to_owned(),
                    end_local_date: None,
                    anchor_local_date: "2026-08-20".to_owned(),
                    kind: "deposit".to_owned(),
                    payload: RulePayloadRecord {
                        endpoint_account_id: Some(
                            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".to_owned(),
                        ),
                        endpoint_component: Some("account_value".to_owned()),
                        amount: Some("1".to_owned()),
                        currency: Some("CNY".to_owned()),
                        ..RulePayloadRecord::default()
                    },
                    note: Some("rule".to_owned()),
                    revision: 1,
                    archived_at: None,
                    created_at: "2026-08-20T00:00:00.000Z".to_owned(),
                    updated_at: "2026-08-20T00:00:00.000Z".to_owned(),
                },
            )
            .await
            .expect("rule");
            assert_eq!(
                list_recurring_activity_rules(&mut tx, &household_id, false)
                    .await
                    .expect("rules")
                    .len(),
                1
            );
            insert_pending_activity(
                &mut tx,
                &PendingActivityRecord {
                    id: "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".to_owned(),
                    household_id: household_id.clone(),
                    recurring_rule_id: None,
                    recurring_rule_revision: None,
                    scheduled_local_date: "2026-08-20".to_owned(),
                    creation_source: "manual".to_owned(),
                    kind: "deposit".to_owned(),
                    payload: PendingPayloadRecord {
                        endpoint_account_id: Some(
                            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".to_owned(),
                        ),
                        endpoint_component: Some("account_value".to_owned()),
                        amount: Some("2".to_owned()),
                        currency: Some("CNY".to_owned()),
                        ..PendingPayloadRecord::default()
                    },
                    note: None,
                    status: "open".to_owned(),
                    posted_activity_id: None,
                    skipped_at: None,
                    created_at: "2026-08-20T00:00:00.000Z".to_owned(),
                    updated_at: "2026-08-20T00:00:00.000Z".to_owned(),
                },
            )
            .await
            .expect("pending");
            assert_eq!(
                list_pending_activities(&mut tx, &household_id, Some("open"), None, 10)
                    .await
                    .expect("pending list")
                    .len(),
                1
            );

            insert_benchmark(
                &mut tx,
                &BenchmarkRecord {
                    id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".to_owned(),
                    household_id: household_id.clone(),
                    name: "Fixture Index".to_owned(),
                    currency: "CNY".to_owned(),
                    series_kind: "price_return".to_owned(),
                    max_carry_days: 7,
                    archived_at: None,
                    created_at: "2026-08-20T00:00:00.000Z".to_owned(),
                    updated_at: "2026-08-20T00:00:00.000Z".to_owned(),
                },
            )
            .await
            .expect("benchmark");
            for (id, created_at, level) in [
                (
                    "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb2",
                    "2026-08-20T00:00:02.000Z",
                    "103",
                ),
                (
                    "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb1",
                    "2026-08-20T00:00:01.000Z",
                    "102",
                ),
            ] {
                insert_benchmark_observation(
                    &mut tx,
                    &BenchmarkObservationRecord {
                        id: id.to_owned(),
                        benchmark_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".to_owned(),
                        level: level.to_owned(),
                        observed_on: "2026-08-20".to_owned(),
                        note: None,
                        source_kind: "manual".to_owned(),
                        import_item_id: None,
                        created_at: created_at.to_owned(),
                    },
                )
                .await
                .expect("observation");
            }
            let observations =
                list_benchmark_observations(&mut tx, "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")
                    .await
                    .expect("observations");
            assert_eq!(observations[0].level, "103");
            assert_eq!(observations[1].level, "102");

            set_benchmark_preference(
                &mut tx,
                &BenchmarkPreferenceRecord {
                    household_id: household_id.clone(),
                    benchmark_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".to_owned(),
                    updated_at: "2026-08-20T00:00:03.000Z".to_owned(),
                },
            )
            .await
            .expect("preference");
            assert_eq!(
                get_benchmark_preference(&mut tx, &household_id)
                    .await
                    .expect("preference read")
                    .expect("preference")
                    .benchmark_id,
                "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
            );

            insert_import_batch(
                &mut tx,
                &ImportBatchRecord {
                    id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc".to_owned(),
                    household_id: household_id.clone(),
                    template: "# nestworth-csv:benchmark:v1".to_owned(),
                    file_sha256: "a".repeat(64),
                    source_namespace: Some("fixture".to_owned()),
                    row_count: 1,
                    committed_count: 0,
                    duplicate_count: 0,
                    rejected_count: 1,
                    status: "committed".to_owned(),
                    created_at: "2026-08-20T00:00:04.000Z".to_owned(),
                    completed_at: Some("2026-08-20T00:00:04.000Z".to_owned()),
                },
            )
            .await
            .expect("import batch");
            insert_import_item(
                &mut tx,
                &ImportItemRecord {
                    id: "cccccccc-cccc-4ccc-8ccc-ccccccccccc1".to_owned(),
                    batch_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc".to_owned(),
                    row_number: 1,
                    source_namespace: Some("fixture".to_owned()),
                    external_id: Some("row-1".to_owned()),
                    fingerprint: "b".repeat(64),
                    outcome: "rejected".to_owned(),
                    diagnostic_code: Some("INVALID_BENCHMARK".to_owned()),
                    activity_id: None,
                    instrument_quote_id: None,
                    fx_quote_id: None,
                    benchmark_observation_id: None,
                    created_at: "2026-08-20T00:00:04.000Z".to_owned(),
                },
            )
            .await
            .expect("import item");
            let batches = list_import_batches(&mut tx, &household_id, 10)
                .await
                .expect("batches");
            assert_eq!(batches.len(), 1);
            assert_eq!(
                list_import_items(&mut tx, &batches[0].id)
                    .await
                    .expect("items")
                    .len(),
                1
            );
            assert_eq!(
                find_import_identity(&mut tx, "fixture", "row-1")
                    .await
                    .expect("identity")
                    .len(),
                1
            );

            finish_write_tx(tx, Ok(())).await.expect("commit");
            let reopened = initialize_database(path.clone()).await;
            assert!(reopened.pool.is_some());
            reopened.pool.expect("reopened").close().await;
            cleanup(&path);
        });
    }
}
