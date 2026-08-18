use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};

use super::{
    account_service::MoneyDto,
    activity_service::{self, latest_cash_money, PostCommand},
    history_origin::ensure_activity_writes_allowed,
    reference::{begin_write_tx, finish_write_tx, map_read_error, require_household_id_tx},
};
use crate::{
    domain::{
        checked_sub, AccountId, CurrencyCode, MonetaryComponent, MonetaryEndpoint, Money,
        PrimaryCategory, TrackingMode,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListAccountCashInput {
    pub account_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppendAccountCashInput {
    pub account_id: String,
    pub amount: String,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountCashRecordDto {
    pub id: String,
    pub account_id: String,
    pub amount: String,
    pub currency: String,
    pub effective_at: String,
    pub created_at: String,
}

pub async fn list_account_cash(
    state: &AppState,
    input: ListAccountCashInput,
) -> Result<Vec<AccountCashRecordDto>, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household_id = require_household_id_tx(&mut tx).await?;
        require_holdings_account(&mut tx, &household_id, &input.account_id).await?;
        list_latest_cash(&mut tx, &input.account_id).await
    }
    .await;
    finish_write_tx(tx, result).await
}

pub(crate) async fn list_latest_cash(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
) -> Result<Vec<AccountCashRecordDto>, AppError> {
    sqlx::query(
        "SELECT id, account_id, amount, currency, effective_at, created_at
         FROM (
           SELECT id, account_id, amount, currency, effective_at, created_at,
                  ROW_NUMBER() OVER (PARTITION BY account_id, currency ORDER BY effective_at DESC, created_at DESC, id DESC) AS rn
           FROM account_cash_values
           WHERE account_id = ?
         ) ranked
         WHERE rn = 1
         ORDER BY currency ASC, id ASC",
    )
    .bind(account_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("cash.list_failed", error))?
    .into_iter()
    .map(cash_from_row)
    .collect()
}

pub(crate) async fn list_latest_cash_for_household(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Vec<AccountCashRecordDto>, AppError> {
    sqlx::query(
        "SELECT c.id, c.account_id, c.amount, c.currency, c.effective_at, c.created_at
         FROM (
           SELECT id, account_id, amount, currency, effective_at, created_at,
                  ROW_NUMBER() OVER (PARTITION BY account_id, currency ORDER BY effective_at DESC, created_at DESC, id DESC) AS rn
           FROM account_cash_values
         ) c
         JOIN accounts a ON a.id = c.account_id
         WHERE a.household_id = ? AND a.archived_at IS NULL AND c.rn = 1
         ORDER BY a.sort_order ASC, a.name COLLATE NOCASE ASC, a.id ASC, c.currency ASC, c.id ASC",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("cash.household_list_failed", error))?
    .into_iter()
    .map(cash_from_row)
    .collect()
}

pub async fn append_account_cash(
    state: &AppState,
    input: AppendAccountCashInput,
) -> Result<AccountCashRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = append_account_cash_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

async fn append_account_cash_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: AppendAccountCashInput,
) -> Result<AccountCashRecordDto, AppError> {
    let origin = ensure_activity_writes_allowed(tx).await?;
    require_holdings_account(tx, &origin.household_id, &input.account_id).await?;
    let currency = CurrencyCode::parse_supported(&input.currency)?;
    let target = Money::parse(&input.amount, currency)?;
    let current = latest_cash_money(tx, &input.account_id, currency).await?;
    // Compatibility: append_account_cash amount is the absolute resulting cash
    // (v0.1.2 semantics). After History Origin, the delta against the latest
    // cash for that currency (missing = 0) is posted as Deposit (increase) or
    // Withdrawal (decrease). Equal amounts are rejected as no-change. Phase 7
    // will post tagged Deposit/Withdrawal amounts explicitly.
    let endpoint = MonetaryEndpoint {
        account_id: AccountId::parse(&input.account_id)?,
        component: MonetaryComponent::HoldingsCash,
    };
    let command = if target.amount() > current.amount() {
        let delta = checked_sub(target.amount(), current.amount())?;
        PostCommand::Deposit {
            endpoint,
            amount: Money::from_canonical(delta, currency)?,
        }
    } else if target.amount() < current.amount() {
        let delta = checked_sub(current.amount(), target.amount())?;
        PostCommand::Withdrawal {
            endpoint,
            amount: Money::from_canonical(delta, currency)?,
        }
    } else {
        return Err(AppError::invalid_activity("The target is unchanged."));
    };
    let posted = activity_service::post_in_tx(tx, command, None).await?;
    tracing::info!(
        event = "account_cash.append",
        activity_id = %posted.id(),
        "cash observation appended"
    );
    load_cash_by_activity(tx, &posted.id().to_string()).await
}

async fn load_cash_by_activity(
    tx: &mut Transaction<'_, Sqlite>,
    activity_id: &str,
) -> Result<AccountCashRecordDto, AppError> {
    let row = sqlx::query(
        "SELECT id, account_id, amount, currency, effective_at, created_at
         FROM account_cash_values
         WHERE activity_id = ?
         ORDER BY created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(activity_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("cash.activity_load_failed", error))?
    .ok_or(AppError::Internal)?;
    cash_from_row(row)
}

async fn require_holdings_account(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    account_id: &str,
) -> Result<(), AppError> {
    let row = sqlx::query(
        "SELECT tracking_mode, primary_category, archived_at FROM accounts WHERE id = ? AND household_id = ?",
    )
    .bind(account_id)
    .bind(household_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("cash.account_lookup_failed", error))?
    .ok_or_else(|| AppError::not_found("account", account_id))?;
    let tracking = TrackingMode::parse(
        &row.try_get::<String, _>("tracking_mode")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    )?;
    let primary = PrimaryCategory::parse(
        &row.try_get::<String, _>("primary_category")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    )?;
    let archived_at: Option<String> = row
        .try_get("archived_at")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    if tracking != TrackingMode::Holdings || primary != PrimaryCategory::Investment {
        return Err(AppError::invalid_category(
            "Cash by currency is only available on holdings accounts.",
        ));
    }
    if archived_at.is_some() {
        return Err(AppError::validation(
            "accountId",
            "Cash cannot be updated on an archived account.",
        ));
    }
    Ok(())
}

fn cash_from_row(row: sqlx::sqlite::SqliteRow) -> Result<AccountCashRecordDto, AppError> {
    Ok(AccountCashRecordDto {
        id: row
            .try_get("id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        account_id: row
            .try_get("account_id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        amount: row
            .try_get("amount")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        currency: row
            .try_get("currency")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        effective_at: row
            .try_get("effective_at")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        created_at: row
            .try_get("created_at")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    })
}

pub fn cash_money(dto: &AccountCashRecordDto) -> Result<MoneyDto, AppError> {
    Ok(MoneyDto {
        amount: dto.amount.clone(),
        currency: dto.currency.clone(),
    })
}
