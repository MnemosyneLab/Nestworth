use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};

use super::{
    instrument_service,
    reference::{
        begin_write_tx, finish_write_tx, map_read_error, map_unique_or_write, map_write_error,
        require_household_id_tx,
    },
};
use crate::{
    domain::{
        AccountId, Holding, HoldingId, InstrumentId, PersistedHolding, PrimaryCategory, Quantity,
        Timestamp, TrackingMode,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListHoldingsInput {
    pub account_id: String,
    pub include_archived: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateHoldingInput {
    pub account_id: String,
    pub instrument_id: String,
    pub quantity: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateHoldingInput {
    pub id: String,
    pub quantity: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HoldingRecordDto {
    pub id: String,
    pub account_id: String,
    pub instrument_id: String,
    pub instrument_name: String,
    pub instrument_symbol: Option<String>,
    pub quote_currency: String,
    pub quantity: String,
    pub note: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

pub async fn list_holdings(
    state: &AppState,
    input: ListHoldingsInput,
) -> Result<Vec<HoldingRecordDto>, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household_id = require_household_id_tx(&mut tx).await?;
        require_account(&mut tx, &household_id, &input.account_id).await?;
        list_holdings_for_account(&mut tx, &input.account_id, input.include_archived).await
    }
    .await;
    finish_write_tx(tx, result).await
}

pub(crate) async fn list_holdings_for_account(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    include_archived: bool,
) -> Result<Vec<HoldingRecordDto>, AppError> {
    sqlx::query(list_sql(include_archived))
        .bind(account_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("holding.list_failed", error))?
        .into_iter()
        .map(holding_from_row)
        .collect()
}

pub(crate) async fn list_active_holdings_for_household(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Vec<HoldingRecordDto>, AppError> {
    sqlx::query(
        "SELECT h.id, h.account_id, h.instrument_id, i.name AS instrument_name, i.symbol AS instrument_symbol, i.quote_currency, h.quantity, h.note, h.sort_order, h.created_at, h.updated_at, h.archived_at
         FROM holdings h
         JOIN instruments i ON i.id = h.instrument_id
         JOIN accounts a ON a.id = h.account_id
         WHERE a.household_id = ? AND h.archived_at IS NULL AND a.archived_at IS NULL
         ORDER BY a.sort_order ASC, a.name COLLATE NOCASE ASC, a.id ASC, h.sort_order ASC, i.name COLLATE NOCASE ASC, h.id ASC",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("holding.household_list_failed", error))?
    .into_iter()
    .map(holding_from_row)
    .collect()
}

fn list_sql(include_archived: bool) -> &'static str {
    if include_archived {
        "SELECT h.id, h.account_id, h.instrument_id, i.name AS instrument_name, i.symbol AS instrument_symbol, i.quote_currency, h.quantity, h.note, h.sort_order, h.created_at, h.updated_at, h.archived_at
         FROM holdings h
         JOIN instruments i ON i.id = h.instrument_id
         WHERE h.account_id = ?
         ORDER BY h.sort_order ASC, i.name COLLATE NOCASE ASC, h.id ASC"
    } else {
        "SELECT h.id, h.account_id, h.instrument_id, i.name AS instrument_name, i.symbol AS instrument_symbol, i.quote_currency, h.quantity, h.note, h.sort_order, h.created_at, h.updated_at, h.archived_at
         FROM holdings h
         JOIN instruments i ON i.id = h.instrument_id
         WHERE h.account_id = ? AND h.archived_at IS NULL
         ORDER BY h.sort_order ASC, i.name COLLATE NOCASE ASC, h.id ASC"
    }
}

pub async fn create_holding(
    state: &AppState,
    input: CreateHoldingInput,
) -> Result<HoldingRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = create_holding_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn update_holding(
    state: &AppState,
    input: UpdateHoldingInput,
) -> Result<HoldingRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = update_holding_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn archive_holding(state: &AppState, id: &str) -> Result<HoldingRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = mutate_archive_in_tx(&mut tx, id, true).await;
    finish_write_tx(tx, result).await
}

pub async fn restore_holding(state: &AppState, id: &str) -> Result<HoldingRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = mutate_archive_in_tx(&mut tx, id, false).await;
    finish_write_tx(tx, result).await
}

async fn create_holding_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: CreateHoldingInput,
) -> Result<HoldingRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let account = require_account(tx, &household_id, &input.account_id).await?;
    if account.0 != TrackingMode::Holdings || account.1 != PrimaryCategory::Investment {
        return Err(AppError::invalid_category(
            "Holdings can only be created on an investment holdings account.",
        ));
    }
    if account.2 {
        return Err(AppError::validation(
            "accountId",
            "Holdings cannot be added to an archived account.",
        ));
    }
    let instrument =
        instrument_service::load_instrument_domain(tx, &household_id, &input.instrument_id).await?;
    if instrument.is_archived() {
        return Err(AppError::validation(
            "instrumentId",
            "Archived instruments cannot be newly selected.",
        ));
    }
    let next_sort: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM holdings WHERE account_id = ?",
    )
    .bind(&input.account_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| map_write_error("holding.sort_failed", error))?;
    let holding = Holding::new(
        AccountId::parse(&input.account_id)?,
        InstrumentId::parse(&input.instrument_id)?,
        Quantity::parse(&input.quantity)?,
        input.note.as_deref(),
        next_sort,
        Timestamp::now(),
    )?;
    sqlx::query(
        "INSERT INTO holdings (id, account_id, instrument_id, quantity, note, sort_order, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(holding.id().to_string())
    .bind(holding.account_id().to_string())
    .bind(holding.instrument_id().to_string())
    .bind(holding.quantity().canonical())
    .bind(holding.note())
    .bind(holding.sort_order())
    .bind(holding.created_at().to_rfc3339())
    .bind(holding.updated_at().to_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(|error| map_unique_or_write("holding.create_failed", error, AppError::DuplicateHolding))?;
    tracing::info!(event = "holding.create", holding_id = %holding.id(), "holding created");
    load_holding(tx, &household_id, &holding.id().to_string()).await
}

async fn update_holding_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: UpdateHoldingInput,
) -> Result<HoldingRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let mut holding = load_holding_domain(tx, &household_id, &input.id).await?;
    holding.update_current_state(
        Quantity::parse(&input.quantity)?,
        input.note.as_deref(),
        Timestamp::now(),
    )?;
    sqlx::query("UPDATE holdings SET quantity = ?, note = ?, updated_at = ? WHERE id = ?")
        .bind(holding.quantity().canonical())
        .bind(holding.note())
        .bind(holding.updated_at().to_rfc3339())
        .bind(holding.id().to_string())
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("holding.update_failed", error))?;
    load_holding(tx, &household_id, &holding.id().to_string()).await
}

async fn mutate_archive_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    archive: bool,
) -> Result<HoldingRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let current = load_holding(tx, &household_id, id).await?;
    if archive && current.archived_at.is_some() {
        return Ok(current);
    }
    if !archive && current.archived_at.is_none() {
        return Ok(current);
    }
    let mut holding = holding_from_dto(current)?;
    if archive {
        holding.archive(Timestamp::now());
        sqlx::query("UPDATE holdings SET archived_at = ?, updated_at = ? WHERE id = ? AND archived_at IS NULL")
            .bind(holding.archived_at().map(Timestamp::to_rfc3339))
            .bind(holding.updated_at().to_rfc3339())
            .bind(holding.id().to_string())
            .execute(&mut **tx)
            .await
            .map_err(|error| map_write_error("holding.archive_failed", error))?;
    } else {
        holding.restore(Timestamp::now());
        sqlx::query("UPDATE holdings SET archived_at = NULL, updated_at = ? WHERE id = ? AND archived_at IS NOT NULL")
            .bind(holding.updated_at().to_rfc3339())
            .bind(holding.id().to_string())
            .execute(&mut **tx)
            .await
            .map_err(|error| {
                map_unique_or_write("holding.restore_failed", error, AppError::DuplicateHolding)
            })?;
    }
    load_holding(tx, &household_id, &holding.id().to_string()).await
}

async fn require_account(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    account_id: &str,
) -> Result<(TrackingMode, PrimaryCategory, bool), AppError> {
    let row = sqlx::query(
        "SELECT tracking_mode, primary_category, archived_at FROM accounts WHERE id = ? AND household_id = ?",
    )
    .bind(account_id)
    .bind(household_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("holding.account_lookup_failed", error))?
    .ok_or_else(|| AppError::not_found("account", account_id))?;
    let tracking: String = row
        .try_get("tracking_mode")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    let primary: String = row
        .try_get("primary_category")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    let archived_at: Option<String> = row
        .try_get("archived_at")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    Ok((
        TrackingMode::parse(&tracking)?,
        PrimaryCategory::parse(&primary)?,
        archived_at.is_some(),
    ))
}

async fn load_holding(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    id: &str,
) -> Result<HoldingRecordDto, AppError> {
    let row = sqlx::query(
        "SELECT h.id, h.account_id, h.instrument_id, i.name AS instrument_name, i.symbol AS instrument_symbol, i.quote_currency, h.quantity, h.note, h.sort_order, h.created_at, h.updated_at, h.archived_at
         FROM holdings h
         JOIN instruments i ON i.id = h.instrument_id
         JOIN accounts a ON a.id = h.account_id
         WHERE h.id = ? AND a.household_id = ?",
    )
    .bind(id)
    .bind(household_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("holding.load_failed", error))?
    .ok_or_else(|| AppError::not_found("holding", id))?;
    holding_from_row(row)
}

async fn load_holding_domain(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    id: &str,
) -> Result<Holding, AppError> {
    holding_from_dto(load_holding(tx, household_id, id).await?)
}

fn holding_from_dto(dto: HoldingRecordDto) -> Result<Holding, AppError> {
    Ok(Holding::from_persisted(PersistedHolding {
        id: HoldingId::parse(&dto.id)?,
        account_id: AccountId::parse(&dto.account_id)?,
        instrument_id: InstrumentId::parse(&dto.instrument_id)?,
        quantity: Quantity::parse(&dto.quantity)?,
        note: dto.note,
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

fn holding_from_row(row: sqlx::sqlite::SqliteRow) -> Result<HoldingRecordDto, AppError> {
    Ok(HoldingRecordDto {
        id: row
            .try_get("id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        account_id: row
            .try_get("account_id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        instrument_id: row
            .try_get("instrument_id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        instrument_name: row
            .try_get("instrument_name")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        instrument_symbol: row
            .try_get("instrument_symbol")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        quote_currency: row
            .try_get("quote_currency")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        quantity: row
            .try_get("quantity")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        note: row
            .try_get("note")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        sort_order: super::reference::sort_order_i32(
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
        archive_holding, create_holding, list_holdings, CreateHoldingInput, ListHoldingsInput,
    };
    use crate::{
        application::{
            account_service::{create_account, CreateAccountInput, OwnershipShareInput},
            instrument_service::{create_instrument, CreateInstrumentInput},
            member_service::list_members,
        },
        error::AppError,
        test_support::{cleanup, onboarded_state, UNKNOWN_UUID},
    };

    async fn seed_holdings_account(state: &crate::state::AppState) -> (String, String) {
        let members = list_members(state, false).await.expect("members");
        let account = create_account(
            state,
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
                    member_id: members[0].id.clone(),
                    percent: Some("100".to_owned()),
                    share_bps: None,
                }],
                initial_amount: None,
            },
        )
        .await
        .expect("account");
        let instrument = create_instrument(
            state,
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
        (account.id, instrument.id)
    }

    #[test]
    fn creates_unique_active_holdings_and_rejects_duplicates() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("holdings-crud").await;
            let (account_id, instrument_id) = seed_holdings_account(&state).await;
            let created = create_holding(
                &state,
                CreateHoldingInput {
                    account_id: account_id.clone(),
                    instrument_id: instrument_id.clone(),
                    quantity: "3".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("create");
            assert_eq!(created.quantity, "3");
            let error = create_holding(
                &state,
                CreateHoldingInput {
                    account_id: account_id.clone(),
                    instrument_id,
                    quantity: "1".to_owned(),
                    note: None,
                },
            )
            .await
            .expect_err("duplicate");
            assert!(matches!(error, AppError::DuplicateHolding));
            archive_holding(&state, &created.id).await.expect("archive");
            assert!(list_holdings(
                &state,
                ListHoldingsInput {
                    account_id,
                    include_archived: false,
                },
            )
            .await
            .expect("list")
            .is_empty());
            cleanup(&path);
        });
    }

    #[test]
    fn rejects_holdings_on_balance_accounts_and_unknown_instruments() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("holdings-validate").await;
            let members = list_members(&state, false).await.expect("members");
            let bank = create_account(
                &state,
                CreateAccountInput {
                    name: "Cash".to_owned(),
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
                        member_id: members[0].id.clone(),
                        percent: Some("100".to_owned()),
                        share_bps: None,
                    }],
                    initial_amount: Some("1".to_owned()),
                },
            )
            .await
            .expect("bank");
            let error = create_holding(
                &state,
                CreateHoldingInput {
                    account_id: bank.id,
                    instrument_id: UNKNOWN_UUID.to_owned(),
                    quantity: "1".to_owned(),
                    note: None,
                },
            )
            .await
            .expect_err("mode");
            assert!(matches!(error, AppError::InvalidCategory { .. }));
            cleanup(&path);
        });
    }
}
