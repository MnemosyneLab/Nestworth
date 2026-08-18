use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};

use super::reference::{
    begin_read_tx, begin_write_tx, finish_read_tx, finish_write_tx, map_read_error,
    map_write_error, next_sort_order, require_household_id_tx, require_household_tx,
    sort_order_i32, SortTable,
};
use super::valuation_service::{self, AccountValuationDto, ValuationSnapshot};
use crate::{
    domain::{
        percent_to_basis_points, Account, AccountGroupId, AccountId, AccountValue, CurrencyCode,
        HouseholdId, InstitutionId, MediaAssetId, MemberId, Money, NewAccount, Ownership,
        OwnershipShare, PersistedAccount, PrimaryCategory, SecondaryCategory, Timestamp,
        TrackingMode,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MoneyDto {
    pub amount: String,
    pub currency: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipShareInput {
    pub member_id: String,
    pub percent: Option<String>,
    pub share_bps: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountInput {
    pub name: String,
    pub primary_category: String,
    pub secondary_category: String,
    pub default_currency: String,
    pub institution_id: Option<String>,
    pub group_id: Option<String>,
    pub tracking_mode: Option<String>,
    pub note: Option<String>,
    pub include_in_net_worth: bool,
    pub include_in_investment: bool,
    pub include_in_liquid_assets: bool,
    pub opened_on: Option<String>,
    pub closed_on: Option<String>,
    pub owners: Vec<OwnershipShareInput>,
    pub initial_amount: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccountInput {
    pub id: String,
    pub name: String,
    pub primary_category: String,
    pub secondary_category: String,
    pub institution_id: Option<String>,
    pub group_id: Option<String>,
    pub tracking_mode: Option<String>,
    pub note: Option<String>,
    pub include_in_net_worth: bool,
    pub include_in_investment: bool,
    pub include_in_liquid_assets: bool,
    pub opened_on: Option<String>,
    pub closed_on: Option<String>,
    pub owners: Vec<OwnershipShareInput>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccountValueInput {
    pub id: String,
    pub amount: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountOwnerDto {
    pub member_id: String,
    pub member_name: String,
    pub share_bps: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountRecordDto {
    pub id: String,
    pub name: String,
    pub primary_category: String,
    pub secondary_category: String,
    pub tracking_mode: String,
    pub default_currency: String,
    pub institution_id: Option<String>,
    pub group_id: Option<String>,
    pub note: Option<String>,
    pub logo_asset_id: Option<String>,
    pub include_in_net_worth: bool,
    pub include_in_investment: bool,
    pub include_in_liquid_assets: bool,
    pub opened_on: Option<String>,
    pub closed_on: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub latest_value: Option<MoneyDto>,
    pub valuation: AccountValuationDto,
    pub owners: Vec<AccountOwnerDto>,
}

pub async fn list_accounts(
    state: &AppState,
    include_archived: bool,
) -> Result<Vec<AccountRecordDto>, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household_id = require_household_id_tx(&mut tx).await?;
        list_accounts_in_tx(&mut tx, &household_id, include_archived).await
    }
    .await;
    finish_read_tx(tx, result).await
}

pub(crate) async fn list_accounts_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    include_archived: bool,
) -> Result<Vec<AccountRecordDto>, AppError> {
    let rows = sqlx::query(list_accounts_sql(include_archived))
        .bind(household_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("account.list_failed", error))?;
    let mut accounts = Vec::with_capacity(rows.len());
    let mut by_id = HashMap::with_capacity(rows.len());
    for row in rows {
        let dto = account_from_list_row(row)?;
        by_id.insert(dto.id.clone(), accounts.len());
        accounts.push(dto);
    }
    if accounts.is_empty() {
        return Ok(accounts);
    }
    let owner_rows = sqlx::query(list_owners_sql(include_archived))
        .bind(household_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("account.owners_load_failed", error))?;
    for row in owner_rows {
        let account_id: String = row
            .try_get("account_id")
            .map_err(|_| AppError::DatabaseUnavailable)?;
        if let Some(index) = by_id.get(&account_id).copied() {
            accounts[index].owners.push(owner_from_row(row)?);
        }
    }
    let household = require_household_tx(tx).await?;
    let snapshot = ValuationSnapshot::load(tx, &household.id, &household.base_currency).await?;
    valuation_service::enrich_accounts(&snapshot, &mut accounts, &Timestamp::now())?;
    Ok(accounts)
}

fn list_accounts_sql(include_archived: bool) -> &'static str {
    if include_archived {
        "SELECT a.id, a.household_id, a.institution_id, a.group_id, a.name, a.primary_category, a.secondary_category, a.tracking_mode, a.default_currency, a.note, a.logo_asset_id, a.include_in_net_worth, a.include_in_investment, a.include_in_liquid_assets, a.opened_on, a.closed_on, a.sort_order, a.created_at, a.updated_at, a.archived_at, v.amount AS latest_amount, v.currency AS latest_currency
         FROM accounts a
         LEFT JOIN (
           SELECT account_id, amount, currency
           FROM (
             SELECT account_id, amount, currency,
                    ROW_NUMBER() OVER (PARTITION BY account_id ORDER BY effective_at DESC, created_at DESC, id DESC) AS rn
             FROM account_values
           ) ranked
           WHERE rn = 1
         ) v ON v.account_id = a.id
         WHERE a.household_id = ?
         ORDER BY a.sort_order ASC, a.name COLLATE NOCASE ASC, a.id ASC"
    } else {
        "SELECT a.id, a.household_id, a.institution_id, a.group_id, a.name, a.primary_category, a.secondary_category, a.tracking_mode, a.default_currency, a.note, a.logo_asset_id, a.include_in_net_worth, a.include_in_investment, a.include_in_liquid_assets, a.opened_on, a.closed_on, a.sort_order, a.created_at, a.updated_at, a.archived_at, v.amount AS latest_amount, v.currency AS latest_currency
         FROM accounts a
         LEFT JOIN (
           SELECT account_id, amount, currency
           FROM (
             SELECT account_id, amount, currency,
                    ROW_NUMBER() OVER (PARTITION BY account_id ORDER BY effective_at DESC, created_at DESC, id DESC) AS rn
             FROM account_values
           ) ranked
           WHERE rn = 1
         ) v ON v.account_id = a.id
         WHERE a.household_id = ? AND a.archived_at IS NULL
         ORDER BY a.sort_order ASC, a.name COLLATE NOCASE ASC, a.id ASC"
    }
}

fn list_owners_sql(include_archived: bool) -> &'static str {
    if include_archived {
        "SELECT o.account_id, o.member_id, m.name, o.share_bps
         FROM account_ownership o
         JOIN members m ON m.id = o.member_id
         JOIN accounts a ON a.id = o.account_id
         WHERE a.household_id = ?
         ORDER BY a.sort_order ASC, a.name COLLATE NOCASE ASC, a.id ASC, m.sort_order ASC, m.name COLLATE NOCASE ASC, m.id ASC"
    } else {
        "SELECT o.account_id, o.member_id, m.name, o.share_bps
         FROM account_ownership o
         JOIN members m ON m.id = o.member_id
         JOIN accounts a ON a.id = o.account_id
         WHERE a.household_id = ? AND a.archived_at IS NULL
         ORDER BY a.sort_order ASC, a.name COLLATE NOCASE ASC, a.id ASC, m.sort_order ASC, m.name COLLATE NOCASE ASC, m.id ASC"
    }
}

pub async fn get_account(state: &AppState, id: &str) -> Result<AccountRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household_id = require_household_id_tx(&mut tx).await?;
        load_account_detail(&mut tx, &household_id, id).await
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn create_account(
    state: &AppState,
    input: CreateAccountInput,
) -> Result<AccountRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = create_account_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn update_account(
    state: &AppState,
    input: UpdateAccountInput,
) -> Result<AccountRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = update_account_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn update_account_value(
    state: &AppState,
    input: UpdateAccountValueInput,
) -> Result<AccountRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = update_account_value_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn archive_account(state: &AppState, id: &str) -> Result<AccountRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = mutate_archive_in_tx(&mut tx, id, true).await;
    finish_write_tx(tx, result).await
}

pub async fn restore_account(state: &AppState, id: &str) -> Result<AccountRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = mutate_archive_in_tx(&mut tx, id, false).await;
    finish_write_tx(tx, result).await
}

async fn create_account_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: CreateAccountInput,
) -> Result<AccountRecordDto, AppError> {
    let household = require_household_tx(tx).await?;
    let sort_order = next_sort_order(tx, SortTable::Accounts, &household.id).await?;
    let mut new_account = new_account_from_input(
        &household.id,
        &input.name,
        &input.primary_category,
        &input.secondary_category,
        &input.default_currency,
        input.institution_id.as_deref(),
        input.group_id.as_deref(),
        input.tracking_mode.as_deref(),
        input.note.as_deref(),
        input.include_in_net_worth,
        input.include_in_investment,
        input.include_in_liquid_assets,
        input.opened_on.as_deref(),
        input.closed_on.as_deref(),
        sort_order,
    )?;
    new_account.default_currency = CurrencyCode::parse_supported(&input.default_currency)?;
    validate_references(
        tx,
        &household.id,
        new_account.institution_id,
        new_account.group_id,
        None,
        None,
    )
    .await?;
    let ownership = parse_ownership(tx, &household.id, &input.owners, None).await?;
    let account = Account::new(new_account, Timestamp::now())?;
    insert_account(tx, &household.id, &account).await?;
    replace_ownership(tx, &account.id().to_string(), &ownership).await?;
    match account.tracking_mode() {
        TrackingMode::Holdings => {
            if input.initial_amount.is_some() {
                return Err(AppError::validation(
                    "initialAmount",
                    "Holdings accounts cannot have an initial account value.",
                ));
            }
        }
        _ => {
            let amount = input.initial_amount.as_deref().ok_or_else(|| {
                AppError::validation("initialAmount", "An initial amount is required.")
            })?;
            let money = Money::parse(amount, account.default_currency())?;
            let value = AccountValue::initial(
                account.id(),
                account.tracking_mode(),
                money,
                Timestamp::now(),
            )?;
            insert_value(tx, &value).await?;
        }
    }
    tracing::info!(event = "account.create", account_id = %account.id(), "account created");
    load_account_detail(tx, &household.id, &account.id().to_string()).await
}

async fn update_account_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: UpdateAccountInput,
) -> Result<AccountRecordDto, AppError> {
    let household = require_household_tx(tx).await?;
    let account_id = AccountId::parse(&input.id)?;
    let current = load_account_detail(tx, &household.id, &account_id.to_string()).await?;
    let retained_members: Vec<String> = current
        .owners
        .iter()
        .map(|owner| owner.member_id.clone())
        .collect();
    let retained_institution = current.institution_id.clone();
    let retained_group = current.group_id.clone();
    let mut account = account_from_dto(&household.id, current)?;
    let update = new_account_from_input(
        &household.id,
        &input.name,
        &input.primary_category,
        &input.secondary_category,
        account.default_currency().as_str(),
        input.institution_id.as_deref(),
        input.group_id.as_deref(),
        input.tracking_mode.as_deref(),
        input.note.as_deref(),
        input.include_in_net_worth,
        input.include_in_investment,
        input.include_in_liquid_assets,
        input.opened_on.as_deref(),
        input.closed_on.as_deref(),
        account.sort_order(),
    )?;
    validate_references(
        tx,
        &household.id,
        update.institution_id,
        update.group_id,
        retained_institution.as_deref(),
        retained_group.as_deref(),
    )
    .await?;
    let ownership = parse_ownership(
        tx,
        &household.id,
        &input.owners,
        Some(retained_members.as_slice()),
    )
    .await?;
    account.update(update, Timestamp::now())?;
    let updated = sqlx::query(
        "UPDATE accounts
         SET name = ?, primary_category = ?, secondary_category = ?, tracking_mode = ?, institution_id = ?, group_id = ?, note = ?, include_in_net_worth = ?, include_in_investment = ?, include_in_liquid_assets = ?, opened_on = ?, closed_on = ?, updated_at = ?
         WHERE id = ? AND household_id = ?",
    )
    .bind(account.name())
    .bind(account.primary_category().as_str())
    .bind(account.secondary_category().as_str())
    .bind(account.tracking_mode().as_str())
    .bind(account.institution_id().map(|id| id.to_string()))
    .bind(account.group_id().map(|id| id.to_string()))
    .bind(account.note())
    .bind(i64::from(account.include_in_net_worth()))
    .bind(i64::from(account.include_in_investment()))
    .bind(i64::from(account.include_in_liquid_assets()))
    .bind(account.opened_on().map(crate::domain::CalendarDate::to_ymd))
    .bind(account.closed_on().map(crate::domain::CalendarDate::to_ymd))
    .bind(account.updated_at().to_rfc3339())
    .bind(account.id().to_string())
    .bind(&household.id)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("account.update_failed", error))?;
    if updated.rows_affected() != 1 {
        return Err(AppError::not_found("account", &account.id().to_string()));
    }
    replace_ownership(tx, &account.id().to_string(), &ownership).await?;
    load_account_detail(tx, &household.id, &account.id().to_string()).await
}

async fn update_account_value_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: UpdateAccountValueInput,
) -> Result<AccountRecordDto, AppError> {
    let household = require_household_tx(tx).await?;
    let account_id = AccountId::parse(&input.id)?;
    let account = load_account_domain(tx, &household.id, &account_id.to_string()).await?;
    let money = Money::parse(&input.amount, account.default_currency())?;
    let value = AccountValue::initial(
        account.id(),
        account.tracking_mode(),
        money,
        Timestamp::now(),
    )?;
    insert_value(tx, &value).await?;
    sqlx::query("UPDATE accounts SET updated_at = ? WHERE id = ? AND household_id = ?")
        .bind(Timestamp::now().to_rfc3339())
        .bind(account.id().to_string())
        .bind(&household.id)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("account.value_touch_failed", error))?;
    load_account_detail(tx, &household.id, &account.id().to_string()).await
}

async fn mutate_archive_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    archive: bool,
) -> Result<AccountRecordDto, AppError> {
    let household = require_household_tx(tx).await?;
    let account_id = AccountId::parse(id)?;
    let current = load_account_detail(tx, &household.id, &account_id.to_string()).await?;
    if archive && current.archived_at.is_some() {
        return Ok(current);
    }
    if !archive && current.archived_at.is_none() {
        return Ok(current);
    }
    let mut account = account_from_dto(&household.id, current)?;
    if archive {
        account.archive(Timestamp::now());
        let updated = sqlx::query(
            "UPDATE accounts SET archived_at = ?, updated_at = ? WHERE id = ? AND household_id = ? AND archived_at IS NULL",
        )
        .bind(account.archived_at().map(Timestamp::to_rfc3339))
        .bind(account.updated_at().to_rfc3339())
        .bind(account.id().to_string())
        .bind(&household.id)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("account.archive_failed", error))?;
        if updated.rows_affected() != 1 {
            let latest = load_account_detail(tx, &household.id, &account.id().to_string()).await?;
            if latest.archived_at.is_some() {
                return Ok(latest);
            }
            return Err(AppError::not_found("account", &account.id().to_string()));
        }
    } else {
        account.restore(Timestamp::now());
        let updated = sqlx::query(
            "UPDATE accounts SET archived_at = NULL, updated_at = ? WHERE id = ? AND household_id = ? AND archived_at IS NOT NULL",
        )
        .bind(account.updated_at().to_rfc3339())
        .bind(account.id().to_string())
        .bind(&household.id)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("account.restore_failed", error))?;
        if updated.rows_affected() != 1 {
            let latest = load_account_detail(tx, &household.id, &account.id().to_string()).await?;
            if latest.archived_at.is_none() {
                return Ok(latest);
            }
            return Err(AppError::not_found("account", &account.id().to_string()));
        }
    }
    load_account_detail(tx, &household.id, &account.id().to_string()).await
}

#[allow(clippy::too_many_arguments)]
fn new_account_from_input(
    household_id: &str,
    name: &str,
    primary_category: &str,
    secondary_category: &str,
    default_currency: &str,
    institution_id: Option<&str>,
    group_id: Option<&str>,
    tracking_mode: Option<&str>,
    note: Option<&str>,
    include_in_net_worth: bool,
    include_in_investment: bool,
    include_in_liquid_assets: bool,
    opened_on: Option<&str>,
    closed_on: Option<&str>,
    sort_order: i64,
) -> Result<NewAccount, AppError> {
    let primary = PrimaryCategory::parse(primary_category)?;
    let secondary = SecondaryCategory::parse_for(primary, secondary_category)?;
    let mut account = NewAccount::required(
        HouseholdId::parse(household_id).map_err(|_| AppError::Internal)?,
        name.to_owned(),
        primary,
        secondary,
        CurrencyCode::parse(default_currency)?,
    );
    account.institution_id = parse_optional_institution(institution_id)?;
    account.group_id = parse_optional_group(group_id)?;
    account.tracking_mode = tracking_mode.map(TrackingMode::parse).transpose()?;
    account.note = note.map(ToOwned::to_owned);
    account.include_in_net_worth = include_in_net_worth;
    account.include_in_investment = include_in_investment;
    account.include_in_liquid_assets = include_in_liquid_assets;
    account.opened_on = opened_on
        .map(crate::domain::CalendarDate::parse)
        .transpose()?;
    account.closed_on = closed_on
        .map(crate::domain::CalendarDate::parse)
        .transpose()?;
    account.sort_order = sort_order;
    Ok(account)
}

async fn validate_references(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    institution_id: Option<InstitutionId>,
    group_id: Option<AccountGroupId>,
    retained_institution_id: Option<&str>,
    retained_group_id: Option<&str>,
) -> Result<(), AppError> {
    if let Some(institution_id) = institution_id {
        require_assignable_reference(
            tx,
            "institutions",
            "institution",
            "institutionId",
            household_id,
            &institution_id.to_string(),
            retained_institution_id,
        )
        .await?;
    }
    if let Some(group_id) = group_id {
        require_assignable_reference(
            tx,
            "account_groups",
            "group",
            "groupId",
            household_id,
            &group_id.to_string(),
            retained_group_id,
        )
        .await?;
    }
    Ok(())
}

async fn require_assignable_reference(
    tx: &mut Transaction<'_, Sqlite>,
    table: &'static str,
    entity: &'static str,
    field: &'static str,
    household_id: &str,
    id: &str,
    retained_id: Option<&str>,
) -> Result<(), AppError> {
    let sql = match table {
        "institutions" => "SELECT archived_at FROM institutions WHERE id = ? AND household_id = ?",
        "account_groups" => {
            "SELECT archived_at FROM account_groups WHERE id = ? AND household_id = ?"
        }
        _ => return Err(AppError::Internal),
    };
    let archived_at: Option<Option<String>> = sqlx::query_scalar(sql)
        .bind(id)
        .bind(household_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| map_read_error("account.reference_lookup_failed", error))?;
    let Some(archived_at) = archived_at else {
        return Err(AppError::not_found(entity, id));
    };
    if archived_at.is_some() && retained_id != Some(id) {
        return Err(AppError::validation(
            field,
            "Archived references cannot be assigned to an account.",
        ));
    }
    Ok(())
}

async fn parse_ownership(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    owners: &[OwnershipShareInput],
    retained_member_ids: Option<&[String]>,
) -> Result<Ownership, AppError> {
    let mut shares = Vec::with_capacity(owners.len());
    for owner in owners {
        let member_id = MemberId::parse(&owner.member_id)?;
        let archived_at: Option<Option<String>> =
            sqlx::query_scalar("SELECT archived_at FROM members WHERE id = ? AND household_id = ?")
                .bind(member_id.to_string())
                .bind(household_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(|error| map_read_error("account.owner_lookup_failed", error))?;
        let Some(archived_at) = archived_at else {
            return Err(AppError::not_found("member", &member_id.to_string()));
        };
        if archived_at.is_some() {
            let retained = retained_member_ids
                .is_some_and(|ids| ids.iter().any(|id| id == &member_id.to_string()));
            if !retained {
                return Err(AppError::validation(
                    "owners",
                    "Archived members cannot be assigned to an account.",
                ));
            }
        }
        let share_bps = match (owner.share_bps, owner.percent.as_deref()) {
            (Some(share_bps), _) => share_bps,
            (None, Some(percent)) => percent_to_basis_points(percent)?,
            (None, None) => {
                return Err(AppError::validation(
                    "owners",
                    "Each owner must include a percent or shareBps value.",
                ))
            }
        };
        shares.push(OwnershipShare::new(member_id, share_bps)?);
    }
    Ownership::parse(shares)
}

async fn insert_account(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    account: &Account,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO accounts
         (id, household_id, institution_id, group_id, name, primary_category, secondary_category, tracking_mode, default_currency, note, logo_asset_id, include_in_net_worth, include_in_investment, include_in_liquid_assets, opened_on, closed_on, sort_order, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(account.id().to_string())
    .bind(household_id)
    .bind(account.institution_id().map(|id| id.to_string()))
    .bind(account.group_id().map(|id| id.to_string()))
    .bind(account.name())
    .bind(account.primary_category().as_str())
    .bind(account.secondary_category().as_str())
    .bind(account.tracking_mode().as_str())
    .bind(account.default_currency().as_str())
    .bind(account.note())
    .bind(account.logo_asset_id().map(|id| id.to_string()))
    .bind(i64::from(account.include_in_net_worth()))
    .bind(i64::from(account.include_in_investment()))
    .bind(i64::from(account.include_in_liquid_assets()))
    .bind(account.opened_on().map(crate::domain::CalendarDate::to_ymd))
    .bind(account.closed_on().map(crate::domain::CalendarDate::to_ymd))
    .bind(account.sort_order())
    .bind(account.created_at().to_rfc3339())
    .bind(account.updated_at().to_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("account.create_failed", error))?;
    Ok(())
}

async fn replace_ownership(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    ownership: &Ownership,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM account_ownership WHERE account_id = ?")
        .bind(account_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("account.ownership_clear_failed", error))?;
    for share in ownership.shares() {
        sqlx::query(
            "INSERT INTO account_ownership (account_id, member_id, share_bps) VALUES (?, ?, ?)",
        )
        .bind(account_id)
        .bind(share.member_id().to_string())
        .bind(share.share_bps())
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("account.ownership_insert_failed", error))?;
    }
    Ok(())
}

async fn insert_value(
    tx: &mut Transaction<'_, Sqlite>,
    value: &AccountValue,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO account_values (id, account_id, value_kind, amount, currency, effective_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(value.id().to_string())
    .bind(value.account_id().to_string())
    .bind(value.value_kind().as_str())
    .bind(value.money().canonical_amount())
    .bind(value.money().currency().as_str())
    .bind(value.effective_at().to_rfc3339())
    .bind(value.created_at().to_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("account.value_insert_failed", error))?;
    Ok(())
}

pub(crate) async fn load_account_detail(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    id: &str,
) -> Result<AccountRecordDto, AppError> {
    let row = sqlx::query(
        "SELECT id, household_id, institution_id, group_id, name, primary_category, secondary_category, tracking_mode, default_currency, note, logo_asset_id, include_in_net_worth, include_in_investment, include_in_liquid_assets, opened_on, closed_on, sort_order, created_at, updated_at, archived_at
         FROM accounts WHERE household_id = ? AND id = ?",
    )
    .bind(household_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("account.load_failed", error))?
    .ok_or_else(|| AppError::not_found("account", id))?;
    let mut dto = account_from_row(row)?;
    dto.latest_value = load_latest_value(tx, id).await?;
    dto.owners = load_owners(tx, id).await?;
    let household = require_household_tx(tx).await?;
    let snapshot = ValuationSnapshot::load(tx, &household.id, &household.base_currency).await?;
    valuation_service::enrich_accounts(
        &snapshot,
        std::slice::from_mut(&mut dto),
        &Timestamp::now(),
    )?;
    Ok(dto)
}

pub(crate) async fn load_account_domain(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    id: &str,
) -> Result<Account, AppError> {
    let dto = load_account_detail(tx, household_id, id).await?;
    account_from_dto(household_id, dto)
}

async fn load_latest_value(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
) -> Result<Option<MoneyDto>, AppError> {
    let row = sqlx::query(
        "SELECT amount, currency
         FROM account_values
         WHERE account_id = ?
         ORDER BY effective_at DESC, created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(account_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("account.value_load_failed", error))?;
    row.map(money_from_row).transpose()
}

async fn load_owners(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
) -> Result<Vec<AccountOwnerDto>, AppError> {
    sqlx::query(
        "SELECT o.member_id, m.name, o.share_bps
         FROM account_ownership o
         JOIN members m ON m.id = o.member_id
         WHERE o.account_id = ?
         ORDER BY m.sort_order ASC, m.name COLLATE NOCASE ASC, m.id ASC",
    )
    .bind(account_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("account.owners_load_failed", error))?
    .into_iter()
    .map(owner_from_row)
    .collect()
}

fn parse_optional_institution(value: Option<&str>) -> Result<Option<InstitutionId>, AppError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => InstitutionId::parse(value).map(Some),
        None => Ok(None),
    }
}

fn parse_optional_group(value: Option<&str>) -> Result<Option<AccountGroupId>, AppError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => AccountGroupId::parse(value).map(Some),
        None => Ok(None),
    }
}

fn money_from_row(row: sqlx::sqlite::SqliteRow) -> Result<MoneyDto, AppError> {
    Ok(MoneyDto {
        amount: row
            .try_get("amount")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        currency: row
            .try_get("currency")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    })
}

fn owner_from_row(row: sqlx::sqlite::SqliteRow) -> Result<AccountOwnerDto, AppError> {
    Ok(AccountOwnerDto {
        member_id: row
            .try_get("member_id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        member_name: row
            .try_get("name")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        share_bps: row
            .try_get("share_bps")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    })
}

fn flag(value: i64) -> bool {
    value != 0
}

fn account_from_list_row(row: sqlx::sqlite::SqliteRow) -> Result<AccountRecordDto, AppError> {
    let latest_amount: Option<String> = row
        .try_get("latest_amount")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    let latest_currency: Option<String> = row
        .try_get("latest_currency")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    let latest_value = match (latest_amount, latest_currency) {
        (Some(amount), Some(currency)) => Some(MoneyDto { amount, currency }),
        (None, None) => None,
        _ => return Err(AppError::DatabaseUnavailable),
    };
    let mut dto = account_from_row(row)?;
    dto.latest_value = latest_value;
    Ok(dto)
}

fn account_from_row(row: sqlx::sqlite::SqliteRow) -> Result<AccountRecordDto, AppError> {
    Ok(AccountRecordDto {
        id: row
            .try_get("id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        name: row
            .try_get("name")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        primary_category: row
            .try_get("primary_category")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        secondary_category: row
            .try_get("secondary_category")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        tracking_mode: row
            .try_get("tracking_mode")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        default_currency: row
            .try_get("default_currency")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        institution_id: row
            .try_get("institution_id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        group_id: row
            .try_get("group_id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        note: row
            .try_get("note")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        logo_asset_id: row
            .try_get("logo_asset_id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        include_in_net_worth: flag(
            row.try_get("include_in_net_worth")
                .map_err(|_| AppError::DatabaseUnavailable)?,
        ),
        include_in_investment: flag(
            row.try_get("include_in_investment")
                .map_err(|_| AppError::DatabaseUnavailable)?,
        ),
        include_in_liquid_assets: flag(
            row.try_get("include_in_liquid_assets")
                .map_err(|_| AppError::DatabaseUnavailable)?,
        ),
        opened_on: row
            .try_get("opened_on")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        closed_on: row
            .try_get("closed_on")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        sort_order: sort_order_i32(
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
        latest_value: None,
        valuation: valuation_service::empty_account_valuation(),
        owners: Vec::new(),
    })
}

fn account_from_dto(household_id: &str, dto: AccountRecordDto) -> Result<Account, AppError> {
    Ok(Account::from_persisted(PersistedAccount {
        id: AccountId::parse(&dto.id)?,
        household_id: HouseholdId::parse(household_id).map_err(|_| AppError::Internal)?,
        institution_id: dto
            .institution_id
            .as_deref()
            .map(InstitutionId::parse)
            .transpose()?,
        group_id: dto
            .group_id
            .as_deref()
            .map(AccountGroupId::parse)
            .transpose()?,
        name: dto.name,
        primary_category: PrimaryCategory::parse(&dto.primary_category)?,
        secondary_category: SecondaryCategory::parse(&dto.secondary_category)?,
        tracking_mode: TrackingMode::parse(&dto.tracking_mode)?,
        default_currency: CurrencyCode::parse(&dto.default_currency)?,
        note: dto.note,
        logo_asset_id: dto
            .logo_asset_id
            .as_deref()
            .map(MediaAssetId::parse)
            .transpose()?,
        include_in_net_worth: dto.include_in_net_worth,
        include_in_investment: dto.include_in_investment,
        include_in_liquid_assets: dto.include_in_liquid_assets,
        opened_on: dto
            .opened_on
            .as_deref()
            .map(crate::domain::CalendarDate::parse)
            .transpose()?,
        closed_on: dto
            .closed_on
            .as_deref()
            .map(crate::domain::CalendarDate::parse)
            .transpose()?,
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

#[cfg(test)]
mod tests {
    use super::{
        archive_account, create_account, get_account, list_accounts, restore_account,
        update_account, update_account_value, CreateAccountInput, OwnershipShareInput,
        UpdateAccountInput, UpdateAccountValueInput,
    };
    use crate::{
        application::{
            group_service::{archive_group, create_group, CreateGroupInput},
            institution_service::{
                archive_institution, create_institution, CreateInstitutionInput,
            },
            member_service::{archive_member, list_members},
        },
        error::AppError,
        test_support::{blocked_future_state, cleanup, file_hash, onboarded_state, UNKNOWN_UUID},
    };

    fn owner(member_id: &str, percent: &str) -> OwnershipShareInput {
        OwnershipShareInput {
            member_id: member_id.to_owned(),
            percent: Some(percent.to_owned()),
            share_bps: None,
        }
    }

    fn bank_input(
        name: &str,
        member_id: &str,
        institution_id: Option<String>,
        amount: &str,
    ) -> CreateAccountInput {
        CreateAccountInput {
            name: name.to_owned(),
            primary_category: "cash_equivalent".to_owned(),
            secondary_category: "bank_account".to_owned(),
            default_currency: "CNY".to_owned(),
            institution_id,
            group_id: None,
            tracking_mode: None,
            note: None,
            include_in_net_worth: true,
            include_in_investment: false,
            include_in_liquid_assets: true,
            opened_on: None,
            closed_on: None,
            owners: vec![owner(member_id, "100")],
            initial_amount: Some(amount.to_owned()),
        }
    }

    async fn seed_bank(
        state: &crate::state::AppState,
    ) -> (
        String,
        String,
        crate::application::institution_service::InstitutionRecordDto,
    ) {
        let members = list_members(state, false).await.expect("members");
        let institution = create_institution(
            state,
            CreateInstitutionInput {
                name: "DBS".to_owned(),
                institution_type: Some("bank".to_owned()),
                country_code: Some("SG".to_owned()),
                website: None,
                note: None,
            },
        )
        .await
        .expect("institution");
        (members[0].id.clone(), members[1].id.clone(), institution)
    }

    #[test]
    fn creates_lists_updates_values_archives_and_restores() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("accounts-crud").await;
            let (walt, spouse, institution) = seed_bank(&state).await;
            let created = create_account(
                &state,
                bank_input(
                    " DBS Savings ",
                    &walt,
                    Some(institution.id.clone()),
                    "100000",
                ),
            )
            .await
            .expect("create should succeed");
            assert_eq!(created.name, "DBS Savings");
            assert_eq!(created.secondary_category, "bank_account");
            assert_eq!(created.tracking_mode, "balance");
            assert_eq!(created.latest_value.as_ref().unwrap().amount, "100000");
            assert_eq!(created.owners.len(), 1);
            assert_eq!(created.owners[0].share_bps, 10_000);

            let listed = list_accounts(&state, false).await.expect("list");
            assert_eq!(listed.len(), 1);

            let updated = update_account(
                &state,
                UpdateAccountInput {
                    id: created.id.clone(),
                    name: "DBS Joint".to_owned(),
                    primary_category: "cash_equivalent".to_owned(),
                    secondary_category: "bank_account".to_owned(),
                    institution_id: Some(institution.id.clone()),
                    group_id: None,
                    tracking_mode: None,
                    note: None,
                    include_in_net_worth: true,
                    include_in_investment: false,
                    include_in_liquid_assets: true,
                    opened_on: None,
                    closed_on: None,
                    owners: vec![owner(&walt, "50"), owner(&spouse, "50")],
                },
            )
            .await
            .expect("update should succeed");
            assert_eq!(updated.name, "DBS Joint");
            assert_eq!(updated.owners.len(), 2);

            let valued = update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: created.id.clone(),
                    amount: "110000".to_owned(),
                },
            )
            .await
            .expect("value update");
            assert_eq!(valued.latest_value.unwrap().amount, "110000");

            let archived = archive_account(&state, &created.id).await.expect("archive");
            assert!(archived.archived_at.is_some());
            assert!(list_accounts(&state, false)
                .await
                .expect("active")
                .is_empty());
            restore_account(&state, &created.id).await.expect("restore");
            assert_eq!(
                list_accounts(&state, false).await.expect("restored").len(),
                1
            );
            cleanup(&path);
        });
    }

    #[test]
    fn create_is_atomic_when_ownership_is_invalid() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("accounts-ownership").await;
            let (walt, spouse, institution) = seed_bank(&state).await;
            let mut input = bank_input("DBS Savings", &walt, Some(institution.id), "100000");
            input.owners = vec![owner(&walt, "60"), owner(&spouse, "50")];
            let error = create_account(&state, input)
                .await
                .expect_err("110% ownership must fail");
            assert!(matches!(
                error,
                AppError::OwnershipTotalInvalid { actual_bps: 11_000 }
            ));
            assert!(list_accounts(&state, true).await.expect("list").is_empty());
            cleanup(&path);
        });
    }

    #[test]
    fn rejects_unknown_institution_and_currency() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("accounts-validate").await;
            let (walt, _, institution) = seed_bank(&state).await;
            let mut input =
                bank_input("DBS Savings", &walt, Some(institution.id.clone()), "100000");
            input.default_currency = "SGD".to_owned();
            let created = create_account(&state, input)
                .await
                .expect("foreign currency is allowed");
            assert_eq!(created.default_currency, "SGD");
            assert!(!created.valuation.complete);

            let mut unsupported_currency = bank_input(
                "Unsupported Currency",
                &walt,
                Some(institution.id.clone()),
                "100000",
            );
            unsupported_currency.default_currency = "ZZZ".to_owned();
            let error = create_account(&state, unsupported_currency)
                .await
                .expect_err("uncatalogued currency");
            assert!(matches!(
                error,
                AppError::Validation { field, .. } if field == "currency"
            ));

            let input = bank_input(
                "DBS Savings",
                &walt,
                Some(UNKNOWN_UUID.to_owned()),
                "100000",
            );
            let error = create_account(&state, input)
                .await
                .expect_err("unknown institution");
            assert!(matches!(error, AppError::NotFound { entity, .. } if entity == "institution"));
            cleanup(&path);
        });
    }

    #[test]
    fn latest_value_uses_effective_created_and_id_order() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("accounts-latest").await;
            let (walt, _, institution) = seed_bank(&state).await;
            let created = create_account(
                &state,
                bank_input("DBS Savings", &walt, Some(institution.id), "100000"),
            )
            .await
            .expect("create");
            update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: created.id.clone(),
                    amount: "125000.50".to_owned(),
                },
            )
            .await
            .expect("second value");
            let detail = get_account(&state, &created.id).await.expect("detail");
            assert_eq!(detail.latest_value.unwrap().amount, "125000.5");
            cleanup(&path);
        });
    }

    #[test]
    fn unknown_account_mutations_are_not_found() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("accounts-missing").await;
            let error = get_account(&state, UNKNOWN_UUID)
                .await
                .expect_err("missing");
            assert!(matches!(error, AppError::NotFound { entity, .. } if entity == "account"));
            let error = archive_account(&state, UNKNOWN_UUID)
                .await
                .expect_err("missing archive");
            assert!(matches!(error, AppError::NotFound { entity, .. } if entity == "account"));
            cleanup(&path);
        });
    }

    #[test]
    fn blocked_future_database_rejects_account_writes() {
        tauri::async_runtime::block_on(async {
            let (state, path, before_hash) = blocked_future_state("accounts").await;
            let input = bank_input("DBS Savings", UNKNOWN_UUID, None, "100000");
            for error in [
                create_account(&state, input.clone())
                    .await
                    .expect_err("create"),
                update_account(
                    &state,
                    UpdateAccountInput {
                        id: UNKNOWN_UUID.to_owned(),
                        name: "DBS".to_owned(),
                        primary_category: "cash_equivalent".to_owned(),
                        secondary_category: "bank_account".to_owned(),
                        institution_id: None,
                        group_id: None,
                        tracking_mode: None,
                        note: None,
                        include_in_net_worth: true,
                        include_in_investment: false,
                        include_in_liquid_assets: true,
                        opened_on: None,
                        closed_on: None,
                        owners: vec![owner(UNKNOWN_UUID, "100")],
                    },
                )
                .await
                .expect_err("update"),
                update_account_value(
                    &state,
                    UpdateAccountValueInput {
                        id: UNKNOWN_UUID.to_owned(),
                        amount: "1".to_owned(),
                    },
                )
                .await
                .expect_err("value"),
                archive_account(&state, UNKNOWN_UUID)
                    .await
                    .expect_err("archive"),
                restore_account(&state, UNKNOWN_UUID)
                    .await
                    .expect_err("restore"),
            ] {
                assert!(matches!(
                    error,
                    AppError::UnsupportedNewerDatabase {
                        found: 999,
                        supported: 2
                    }
                ));
            }
            assert_eq!(file_hash(&path), before_hash);
            cleanup(&path);
        });
    }

    async fn table_count(state: &crate::state::AppState, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(state.writable_db().expect("writable"))
            .await
            .expect("count")
    }

    fn cash_input(name: &str, member_id: &str, amount: &str) -> CreateAccountInput {
        let mut input = bank_input(name, member_id, None, amount);
        input.secondary_category = "cash".to_owned();
        input
    }

    fn account_update(
        id: &str,
        name: &str,
        institution_id: Option<String>,
        group_id: Option<String>,
        owners: Vec<OwnershipShareInput>,
        tracking_mode: Option<&str>,
    ) -> UpdateAccountInput {
        UpdateAccountInput {
            id: id.to_owned(),
            name: name.to_owned(),
            primary_category: "cash_equivalent".to_owned(),
            secondary_category: "bank_account".to_owned(),
            institution_id,
            group_id,
            tracking_mode: tracking_mode.map(ToOwned::to_owned),
            note: None,
            include_in_net_worth: true,
            include_in_investment: false,
            include_in_liquid_assets: true,
            opened_on: None,
            closed_on: None,
            owners,
        }
    }

    #[test]
    fn list_accounts_keeps_latest_values_and_owners_aligned() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("accounts-list-align").await;
            let (walt, spouse, institution) = seed_bank(&state).await;
            let dbs = create_account(
                &state,
                bank_input("DBS Savings", &walt, Some(institution.id.clone()), "100000"),
            )
            .await
            .expect("dbs");
            create_account(&state, cash_input("WeChat", &spouse, "10000"))
                .await
                .expect("wechat");
            update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: dbs.id.clone(),
                    amount: "110000".to_owned(),
                },
            )
            .await
            .expect("value");
            let listed = list_accounts(&state, false).await.expect("list");
            assert_eq!(listed.len(), 2);
            assert_eq!(listed[0].name, "DBS Savings");
            assert_eq!(listed[0].latest_value.as_ref().unwrap().amount, "110000");
            assert_eq!(listed[0].owners.len(), 1);
            assert_eq!(listed[0].owners[0].member_id, walt);
            assert_eq!(listed[1].name, "WeChat");
            assert_eq!(listed[1].latest_value.as_ref().unwrap().amount, "10000");
            assert_eq!(listed[1].owners[0].member_id, spouse);
            cleanup(&path);
        });
    }

    #[test]
    fn create_rejects_archived_references_without_writes() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("accounts-archived-create").await;
            let (walt, spouse, institution) = seed_bank(&state).await;
            archive_institution(&state, &institution.id)
                .await
                .expect("archive institution");
            archive_member(&state, &spouse)
                .await
                .expect("archive member");
            let group = create_group(
                &state,
                CreateGroupInput {
                    name: "Emergency".to_owned(),
                    icon_key: None,
                    color: None,
                    description: None,
                },
            )
            .await
            .expect("group");
            archive_group(&state, &group.id)
                .await
                .expect("archive group");

            let error = create_account(
                &state,
                bank_input("DBS", &walt, Some(institution.id.clone()), "100000"),
            )
            .await
            .expect_err("archived institution");
            assert!(
                matches!(error, AppError::Validation { field, .. } if field == "institutionId")
            );

            let mut member_input = cash_input("WeChat", &spouse, "10000");
            let error = create_account(&state, member_input.clone())
                .await
                .expect_err("archived member");
            assert!(matches!(error, AppError::Validation { field, .. } if field == "owners"));

            member_input.owners = vec![owner(&walt, "100")];
            member_input.group_id = Some(group.id);
            let error = create_account(&state, member_input)
                .await
                .expect_err("archived group");
            assert!(matches!(error, AppError::Validation { field, .. } if field == "groupId"));

            assert!(list_accounts(&state, true).await.expect("list").is_empty());
            assert_eq!(
                table_count(&state, "SELECT COUNT(*) FROM account_values").await,
                0
            );
            assert_eq!(
                table_count(&state, "SELECT COUNT(*) FROM account_ownership").await,
                0
            );
            cleanup(&path);
        });
    }

    #[test]
    fn update_can_keep_but_not_switch_to_archived_references() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("accounts-archived-update").await;
            let (walt, spouse, first) = seed_bank(&state).await;
            let second = create_institution(
                &state,
                CreateInstitutionInput {
                    name: "OCBC".to_owned(),
                    institution_type: Some("bank".to_owned()),
                    country_code: Some("SG".to_owned()),
                    website: None,
                    note: None,
                },
            )
            .await
            .expect("second institution");
            let created = create_account(
                &state,
                bank_input("DBS Savings", &walt, Some(first.id.clone()), "100000"),
            )
            .await
            .expect("create");
            archive_institution(&state, &first.id)
                .await
                .expect("archive current");
            archive_institution(&state, &second.id)
                .await
                .expect("archive other");

            let kept = update_account(
                &state,
                account_update(
                    &created.id,
                    "DBS Kept",
                    Some(first.id.clone()),
                    None,
                    vec![owner(&walt, "100")],
                    Some("balance"),
                ),
            )
            .await
            .expect("retain archived institution");
            assert_eq!(kept.name, "DBS Kept");
            assert_eq!(kept.institution_id.as_deref(), Some(first.id.as_str()));

            let before = get_account(&state, &created.id).await.expect("before");
            let error = update_account(
                &state,
                account_update(
                    &created.id,
                    "DBS Switch",
                    Some(second.id.clone()),
                    None,
                    vec![owner(&walt, "100")],
                    Some("balance"),
                ),
            )
            .await
            .expect_err("switch to other archived institution");
            assert!(
                matches!(error, AppError::Validation { field, .. } if field == "institutionId")
            );
            let after = get_account(&state, &created.id).await.expect("after");
            assert_eq!(after, before);

            archive_member(&state, &spouse)
                .await
                .expect("archive spouse");
            let error = update_account(
                &state,
                account_update(
                    &created.id,
                    "DBS Kept",
                    Some(first.id.clone()),
                    None,
                    vec![owner(&walt, "50"), owner(&spouse, "50")],
                    Some("balance"),
                ),
            )
            .await
            .expect_err("new archived owner");
            assert!(matches!(error, AppError::Validation { field, .. } if field == "owners"));
            let after_owner = get_account(&state, &created.id)
                .await
                .expect("owners unchanged");
            assert_eq!(after_owner.owners.len(), 1);
            assert_eq!(after_owner.owners[0].member_id, walt);
            cleanup(&path);
        });
    }

    #[test]
    fn update_retains_archived_owner_already_on_the_account() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("accounts-archived-owner").await;
            let (walt, spouse, institution) = seed_bank(&state).await;
            let created = create_account(
                &state,
                bank_input("DBS Savings", &walt, Some(institution.id.clone()), "100000"),
            )
            .await
            .expect("create");
            let joint = update_account(
                &state,
                account_update(
                    &created.id,
                    "DBS Joint",
                    Some(institution.id.clone()),
                    None,
                    vec![owner(&walt, "50"), owner(&spouse, "50")],
                    Some("balance"),
                ),
            )
            .await
            .expect("joint");
            assert_eq!(joint.owners.len(), 2);
            archive_member(&state, &spouse)
                .await
                .expect("archive spouse");
            let kept = update_account(
                &state,
                account_update(
                    &created.id,
                    "DBS Joint Kept",
                    Some(institution.id.clone()),
                    None,
                    vec![owner(&walt, "50"), owner(&spouse, "50")],
                    Some("balance"),
                ),
            )
            .await
            .expect("retain archived owner");
            assert_eq!(kept.name, "DBS Joint Kept");
            assert_eq!(kept.owners.len(), 2);
            cleanup(&path);
        });
    }

    #[test]
    fn rejects_tracking_mode_change_without_mutating_values() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("accounts-tracking-lock").await;
            let (walt, _, institution) = seed_bank(&state).await;
            let created = create_account(
                &state,
                bank_input("DBS Savings", &walt, Some(institution.id.clone()), "100000"),
            )
            .await
            .expect("create");
            let before = get_account(&state, &created.id).await.expect("before");
            let error = update_account(
                &state,
                account_update(
                    &created.id,
                    "DBS Savings",
                    Some(institution.id.clone()),
                    None,
                    vec![owner(&walt, "100")],
                    Some("manual_value"),
                ),
            )
            .await
            .expect_err("tracking mode locked");
            assert!(matches!(error, AppError::Validation { field, .. } if field == "trackingMode"));
            let after = get_account(&state, &created.id).await.expect("after");
            assert_eq!(after, before);
            assert_eq!(
                table_count(&state, "SELECT COUNT(*) FROM account_values").await,
                1
            );
            cleanup(&path);
        });
    }
}
