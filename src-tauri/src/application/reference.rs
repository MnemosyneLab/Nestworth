use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Sqlite, Transaction};

use crate::{error::AppError, infrastructure::database::SqlitePool};

#[derive(Debug, Clone, Copy)]
pub enum SortTable {
    Members,
    Institutions,
    AccountGroups,
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
