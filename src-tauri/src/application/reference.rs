use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};

use crate::{error::AppError, infrastructure::database::SqlitePool};

#[derive(Debug, Clone)]
pub struct HouseholdRef {
    pub id: String,
    pub base_currency: String,
}

#[derive(Debug, Clone, Copy)]
pub enum SortTable {
    Members,
    Institutions,
    AccountGroups,
    Accounts,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListFilterInput {
    pub include_archived: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IdInput {
    pub id: String,
}

pub async fn require_household_id(database: &SqlitePool) -> Result<String, AppError> {
    sqlx::query_scalar::<_, String>("SELECT id FROM households ORDER BY created_at, id LIMIT 1")
        .fetch_optional(database)
        .await
        .map_err(|error| map_read_error("reference.household_load_failed", error))?
        .ok_or_else(|| AppError::not_found("household", "current"))
}

pub async fn require_household_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<HouseholdRef, AppError> {
    let row =
        sqlx::query("SELECT id, base_currency FROM households ORDER BY created_at, id LIMIT 1")
            .fetch_optional(&mut **tx)
            .await
            .map_err(|error| map_read_error("reference.household_load_failed", error))?
            .ok_or_else(|| AppError::not_found("household", "current"))?;
    Ok(HouseholdRef {
        id: row
            .try_get("id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        base_currency: row
            .try_get("base_currency")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    })
}

pub async fn require_household_id_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<String, AppError> {
    Ok(require_household_tx(tx).await?.id)
}

pub async fn begin_write_tx(
    database: &SqlitePool,
) -> Result<Transaction<'static, Sqlite>, AppError> {
    database
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| map_write_error("reference.begin_failed", error))
}

pub async fn finish_write_tx<T>(
    tx: Transaction<'static, Sqlite>,
    result: Result<T, AppError>,
) -> Result<T, AppError> {
    match result {
        Ok(value) => {
            tx.commit()
                .await
                .map_err(|error| map_write_error("reference.commit_failed", error))?;
            Ok(value)
        }
        Err(error) => {
            if let Err(rollback_error) = tx.rollback().await {
                tracing::error!(
                    event = "reference.rollback_failed",
                    error = ?rollback_error,
                    "failed to roll back reference mutation"
                );
            }
            Err(error)
        }
    }
}

pub async fn next_sort_order(
    tx: &mut Transaction<'_, Sqlite>,
    table: SortTable,
    household_id: &str,
) -> Result<i64, AppError> {
    let sql = match table {
        SortTable::Members => {
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM members WHERE household_id = ?"
        }
        SortTable::Institutions => {
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM institutions WHERE household_id = ?"
        }
        SortTable::AccountGroups => {
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM account_groups WHERE household_id = ?"
        }
        SortTable::Accounts => {
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM accounts WHERE household_id = ?"
        }
    };
    sqlx::query_scalar(sql)
        .bind(household_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| map_write_error("reference.sort_order_failed", error))
}

pub fn map_read_error(event: &'static str, error: sqlx::Error) -> AppError {
    tracing::error!(event, error = ?error, "database read failed");
    AppError::from(error)
}

pub fn map_write_error(event: &'static str, error: sqlx::Error) -> AppError {
    tracing::error!(event, error = ?error, "database write failed");
    AppError::from(error)
}

pub fn sort_order_i32(value: i64) -> Result<i32, AppError> {
    i32::try_from(value).map_err(|_| AppError::Internal)
}
