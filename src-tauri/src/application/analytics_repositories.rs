use sqlx::{Row, Sqlite, Transaction};

use super::{
    history_repositories::OriginHoldingRecord,
    query_count,
    reference::{map_read_error, map_write_error},
};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostBasisDeclarationRecord {
    pub id: String,
    pub household_id: String,
    pub origin_holding_id: Option<String>,
    pub activity_leg_id: Option<String>,
    pub instrument_id: String,
    pub declared_cost: Option<String>,
    pub declared_currency: Option<String>,
    pub acquired_on: Option<String>,
    pub revokes: Option<String>,
    pub is_revocation: bool,
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityLegHouseholdRecord {
    pub id: String,
    pub activity_id: String,
    pub household_id: String,
    pub instrument_id: Option<String>,
}

fn flag(value: i64) -> bool {
    value != 0
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

fn declaration_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<CostBasisDeclarationRecord, AppError> {
    Ok(CostBasisDeclarationRecord {
        id: required_text(&row, "id")?,
        household_id: required_text(&row, "household_id")?,
        origin_holding_id: optional_text(&row, "origin_holding_id")?,
        activity_leg_id: optional_text(&row, "activity_leg_id")?,
        instrument_id: required_text(&row, "instrument_id")?,
        declared_cost: optional_text(&row, "declared_cost")?,
        declared_currency: optional_text(&row, "declared_currency")?,
        acquired_on: optional_text(&row, "acquired_on")?,
        revokes: optional_text(&row, "revokes")?,
        is_revocation: flag(required_i64(&row, "is_revocation")?),
        note: optional_text(&row, "note")?,
        created_at: required_text(&row, "created_at")?,
    })
}

pub async fn insert_declaration(
    tx: &mut Transaction<'_, Sqlite>,
    row: &CostBasisDeclarationRecord,
) -> Result<(), AppError> {
    query_count::record("cost_basis_declaration_insert");
    sqlx::query(
        "INSERT INTO cost_basis_declarations (
            id, household_id, origin_holding_id, activity_leg_id, instrument_id,
            declared_cost, declared_currency, acquired_on, revokes, is_revocation, note, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.household_id)
    .bind(&row.origin_holding_id)
    .bind(&row.activity_leg_id)
    .bind(&row.instrument_id)
    .bind(&row.declared_cost)
    .bind(&row.declared_currency)
    .bind(&row.acquired_on)
    .bind(&row.revokes)
    .bind(i64::from(row.is_revocation))
    .bind(&row.note)
    .bind(&row.created_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("cost_basis.declaration_insert_failed", error))?;
    Ok(())
}

pub async fn latest_declaration_for_lot(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    origin_holding_id: Option<&str>,
    activity_leg_id: Option<&str>,
) -> Result<Option<CostBasisDeclarationRecord>, AppError> {
    query_count::record("cost_basis_declaration_latest");
    let row = sqlx::query(
        "SELECT id, household_id, origin_holding_id, activity_leg_id, instrument_id,
                declared_cost, declared_currency, acquired_on, revokes, is_revocation, note, created_at
         FROM cost_basis_declarations
         WHERE household_id = ?
           AND (
                (? IS NOT NULL AND origin_holding_id = ?)
                OR (? IS NOT NULL AND activity_leg_id = ?)
           )
         ORDER BY created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(household_id)
    .bind(origin_holding_id)
    .bind(origin_holding_id)
    .bind(activity_leg_id)
    .bind(activity_leg_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("cost_basis.declaration_latest_failed", error))?;
    row.map(declaration_from_row).transpose()
}

pub async fn list_declarations_for_lot(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    origin_holding_id: Option<&str>,
    activity_leg_id: Option<&str>,
) -> Result<Vec<CostBasisDeclarationRecord>, AppError> {
    query_count::record("cost_basis_declaration_list_lot");
    sqlx::query(
        "SELECT id, household_id, origin_holding_id, activity_leg_id, instrument_id,
                declared_cost, declared_currency, acquired_on, revokes, is_revocation, note, created_at
         FROM cost_basis_declarations
         WHERE household_id = ?
           AND (
                (? IS NOT NULL AND origin_holding_id = ?)
                OR (? IS NOT NULL AND activity_leg_id = ?)
           )
         ORDER BY created_at DESC, id DESC",
    )
    .bind(household_id)
    .bind(origin_holding_id)
    .bind(origin_holding_id)
    .bind(activity_leg_id)
    .bind(activity_leg_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("cost_basis.declaration_list_lot_failed", error))?
    .into_iter()
    .map(declaration_from_row)
    .collect()
}

pub async fn list_declarations_for_household(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Vec<CostBasisDeclarationRecord>, AppError> {
    query_count::record("cost_basis_declaration_list_household");
    sqlx::query(
        "SELECT id, household_id, origin_holding_id, activity_leg_id, instrument_id,
                declared_cost, declared_currency, acquired_on, revokes, is_revocation, note, created_at
         FROM cost_basis_declarations
         WHERE household_id = ?
         ORDER BY created_at DESC, id DESC",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("cost_basis.declaration_list_failed", error))?
    .into_iter()
    .map(declaration_from_row)
    .collect()
}

pub async fn count_declarations(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<i64, AppError> {
    query_count::record("cost_basis_declaration_count");
    sqlx::query_scalar("SELECT COUNT(*) FROM cost_basis_declarations WHERE household_id = ?")
        .bind(household_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| map_read_error("cost_basis.declaration_count_failed", error))
}

pub async fn get_origin_holding_for_household(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    holding_id: &str,
) -> Result<Option<OriginHoldingRecord>, AppError> {
    query_count::record("cost_basis_origin_holding");
    let row = sqlx::query(
        "SELECT h.origin_id, h.holding_id, h.account_id, h.instrument_id, h.quantity, h.active
         FROM history_origin_holdings h
         JOIN history_origins o ON o.id = h.origin_id
         WHERE o.household_id = ? AND h.holding_id = ?",
    )
    .bind(household_id)
    .bind(holding_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("cost_basis.origin_holding_load_failed", error))?;
    row.map(|row| {
        Ok(OriginHoldingRecord {
            origin_id: required_text(&row, "origin_id")?,
            holding_id: required_text(&row, "holding_id")?,
            account_id: required_text(&row, "account_id")?,
            instrument_id: required_text(&row, "instrument_id")?,
            quantity: required_text(&row, "quantity")?,
            active: flag(required_i64(&row, "active")?),
        })
    })
    .transpose()
}

pub async fn get_activity_leg_for_household(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    leg_id: &str,
) -> Result<Option<ActivityLegHouseholdRecord>, AppError> {
    query_count::record("cost_basis_activity_leg");
    let row = sqlx::query(
        "SELECT l.id, l.activity_id, a.household_id, l.instrument_id
         FROM activity_legs l
         JOIN activities a ON a.id = l.activity_id
         WHERE l.id = ? AND a.household_id = ?",
    )
    .bind(leg_id)
    .bind(household_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("cost_basis.activity_leg_load_failed", error))?;
    row.map(|row| {
        Ok(ActivityLegHouseholdRecord {
            id: required_text(&row, "id")?,
            activity_id: required_text(&row, "activity_id")?,
            household_id: required_text(&row, "household_id")?,
            instrument_id: optional_text(&row, "instrument_id")?,
        })
    })
    .transpose()
}
