//! Freshness-policy settings and the read-only Maintenance queue.
//!
//! Freshness is deliberately an application label. This module reads existing
//! observations and policy rows, but never changes the ledger, projections,
//! snapshots, dirty markers, or valuation semantics.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};

use super::{
    account_service, history_repositories,
    pending_service::{self, PendingActivityDto},
    query_count,
    reference::{
        begin_read_tx, begin_write_tx, finish_read_tx, finish_write_tx, require_household_tx,
    },
    sustainable_repositories::{
        self as repositories, FreshnessPolicyRecord, MaintenanceSnoozeRecord,
    },
    valuation_service::{self, ValuationSnapshot},
};
use crate::{
    domain::{
        AccountId, CalendarDate, CurrencyCode, FreshnessPolicyId, FreshnessPolicyKind, FxPair,
        HistoryTimezone, InstrumentId, MaintenanceSnoozeId, Timestamp, TrackingMode,
    },
    error::AppError,
    state::AppState,
};

const DUE_PENDING_LIMIT: i64 = 200;

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFreshnessPolicyInput {
    pub id: Option<String>,
    pub kind: String,
    pub target_account_id: Option<String>,
    pub target_instrument_id: Option<String>,
    pub target_currency_a: Option<String>,
    pub target_currency_b: Option<String>,
    pub review_interval_days: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SnoozeMaintenanceItemInput {
    pub policy_kind: String,
    pub target_account_id: Option<String>,
    pub target_instrument_id: Option<String>,
    pub target_currency_a: Option<String>,
    pub target_currency_b: Option<String>,
    pub snoozed_until: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FreshnessPolicyDto {
    pub id: String,
    pub kind: String,
    pub target_account_id: Option<String>,
    pub target_instrument_id: Option<String>,
    pub target_currency_a: Option<String>,
    pub target_currency_b: Option<String>,
    pub review_interval_days: Option<i32>,
    pub is_default: bool,
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceSnoozeDto {
    pub id: String,
    pub policy_kind: String,
    pub target_account_id: Option<String>,
    pub target_instrument_id: Option<String>,
    pub target_currency_a: Option<String>,
    pub target_currency_b: Option<String>,
    pub snoozed_until: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MaintenancePageDto {
    pub local_date: String,
    pub items: Vec<MaintenanceItemDto>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceItemDto {
    pub id: String,
    pub item_kind: String,
    pub policy_id: Option<String>,
    pub policy_kind: Option<String>,
    pub target_account_id: Option<String>,
    pub target_instrument_id: Option<String>,
    pub target_currency_a: Option<String>,
    pub target_currency_b: Option<String>,
    pub label: String,
    pub underlying_status: String,
    pub status: String,
    pub observed_on: Option<String>,
    pub due_on: Option<String>,
    pub snoozed_until: Option<String>,
    pub pending_activity: Option<PendingActivityDto>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PolicyTarget {
    account_id: Option<String>,
    instrument_id: Option<String>,
    currency_a: Option<String>,
    currency_b: Option<String>,
}

impl PolicyTarget {
    fn is_default(&self) -> bool {
        self.account_id.is_none()
            && self.instrument_id.is_none()
            && self.currency_a.is_none()
            && self.currency_b.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TargetKey {
    kind: FreshnessPolicyKind,
    account_id: Option<String>,
    instrument_id: Option<String>,
    currency_a: Option<String>,
    currency_b: Option<String>,
}

impl TargetKey {
    fn new(kind: FreshnessPolicyKind, target: &PolicyTarget) -> Self {
        Self {
            kind,
            account_id: target.account_id.clone(),
            instrument_id: target.instrument_id.clone(),
            currency_a: target.currency_a.clone(),
            currency_b: target.currency_b.clone(),
        }
    }
}

pub async fn list_freshness_policies(
    state: &AppState,
    include_archived: bool,
) -> Result<Vec<FreshnessPolicyDto>, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        repositories::list_freshness_policies(&mut tx, &household.id, include_archived)
            .await?
            .iter()
            .map(policy_dto)
            .collect()
    }
    .await;
    finish_read_tx(tx, result).await
}

pub async fn update_freshness_policy(
    state: &AppState,
    input: UpdateFreshnessPolicyInput,
) -> Result<FreshnessPolicyDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let kind = FreshnessPolicyKind::parse(&input.kind)?;
        let target = normalize_target(
            kind,
            input.target_account_id,
            input.target_instrument_id,
            input.target_currency_a,
            input.target_currency_b,
        )?;
        let interval = parse_interval(input.review_interval_days)?;
        let existing = if let Some(id) = input.id.as_deref() {
            FreshnessPolicyId::parse(id)?;
            repositories::list_freshness_policies(&mut tx, &household.id, true)
                .await?
                .into_iter()
                .find(|row| row.id == id)
                .ok_or_else(|| AppError::not_found("freshness policy", id))
                .map(Some)?
        } else {
            None
        };

        validate_target_reference_in_tx(&mut tx, &household.id, kind, &target, existing.is_some())
            .await?;

        let now = Timestamp::now().to_rfc3339();
        let mut row = existing.unwrap_or_else(|| FreshnessPolicyRecord {
            id: FreshnessPolicyId::new().to_string(),
            household_id: household.id.clone(),
            kind: kind.as_str().to_owned(),
            target_account_id: target.account_id.clone(),
            target_instrument_id: target.instrument_id.clone(),
            target_currency_a: target.currency_a.clone(),
            target_currency_b: target.currency_b.clone(),
            review_interval_days: interval,
            archived_at: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        });

        if row.kind != kind.as_str() || record_target(&row)? != target {
            return Err(AppError::conflict(
                "A freshness policy kind and target cannot be changed after creation.",
            ));
        }

        row.review_interval_days = interval;
        row.updated_at = now;
        if input.id.is_some() {
            repositories::update_freshness_policy(&mut tx, &row).await?;
        } else {
            repositories::insert_freshness_policy(&mut tx, &row).await?;
        }
        policy_dto(&row)
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn snooze_maintenance_item(
    state: &AppState,
    input: SnoozeMaintenanceItemInput,
) -> Result<MaintenanceSnoozeDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let kind = FreshnessPolicyKind::parse(&input.policy_kind)?;
        let target = normalize_target(
            kind,
            input.target_account_id,
            input.target_instrument_id,
            input.target_currency_a,
            input.target_currency_b,
        )?;
        if target.is_default() {
            return Err(AppError::validation(
                "target",
                "A snooze must identify a concrete maintenance target.",
            ));
        }
        CalendarDate::parse(&input.snoozed_until)?;
        validate_target_reference_in_tx(&mut tx, &household.id, kind, &target, false).await?;
        let policies = repositories::list_freshness_policies(&mut tx, &household.id, false).await?;
        let policy = resolve_policy(&policies, kind, &target).ok_or_else(|| {
            AppError::conflict("No active freshness policy exists for this target.")
        })?;
        if policy.review_interval_days.is_none() {
            return Err(AppError::conflict(
                "The freshness policy is disabled for this target.",
            ));
        }

        let row = MaintenanceSnoozeRecord {
            id: MaintenanceSnoozeId::new().to_string(),
            household_id: household.id,
            policy_kind: kind.as_str().to_owned(),
            target_account_id: target.account_id,
            target_instrument_id: target.instrument_id,
            target_currency_a: target.currency_a,
            target_currency_b: target.currency_b,
            snoozed_until: input.snoozed_until,
            created_at: Timestamp::now().to_rfc3339(),
        };
        repositories::insert_maintenance_snooze(&mut tx, &row).await?;
        snooze_dto(&row)
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn list_maintenance_items(state: &AppState) -> Result<MaintenancePageDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = list_maintenance_items_in_tx(&mut tx).await;
    finish_read_tx(tx, result).await
}

async fn list_maintenance_items_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<MaintenancePageDto, AppError> {
    let household = require_household_tx(tx).await?;
    let origin = history_repositories::get_origin_by_household(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    if !origin.timezone_confirmed {
        return Err(AppError::HistoryTimezoneConfirmationRequired);
    }
    let timezone = HistoryTimezone::parse(&origin.timezone)?;
    let now = Timestamp::now();
    let today = timezone.local_date(&now);
    let accounts = account_service::list_account_records_in_tx(tx, &household.id, false).await?;
    let snapshot = ValuationSnapshot::load(tx, &household.id, &household.base_currency).await?;
    let policies = repositories::list_freshness_policies(tx, &household.id, false).await?;
    let snoozes = repositories::list_maintenance_snoozes(tx, &household.id).await?;
    let active_snoozes = active_snoozes(&snoozes, today)?;
    let account_value_dates = latest_account_value_dates(tx, &household.id, &now).await?;
    let account_by_id = accounts
        .iter()
        .map(|account| (account.id.as_str(), account))
        .collect::<HashMap<_, _>>();
    let mut items = Vec::new();

    for account in &accounts {
        let kind = if account.tracking_mode == TrackingMode::Holdings.as_str() {
            FreshnessPolicyKind::AccountCash
        } else if account.tracking_mode == TrackingMode::Balance.as_str()
            || account.tracking_mode == TrackingMode::ManualValue.as_str()
        {
            FreshnessPolicyKind::AccountValue
        } else {
            continue;
        };
        let target = PolicyTarget {
            account_id: Some(account.id.clone()),
            instrument_id: None,
            currency_a: None,
            currency_b: None,
        };
        if kind == FreshnessPolicyKind::AccountValue {
            if let Some(policy) = enabled_policy(&policies, kind, &target) {
                append_freshness_item(
                    &mut items,
                    today,
                    &timezone,
                    policy,
                    &target,
                    account.name.clone(),
                    account_value_dates.get(&account.id).map(String::as_str),
                    &active_snoozes,
                    None,
                )?;
            }
        }
    }

    for cash in valuation_service::snapshot_cash(&snapshot) {
        let Some(account) = account_by_id.get(cash.account_id.as_str()) else {
            continue;
        };
        if account.tracking_mode != TrackingMode::Holdings.as_str() {
            continue;
        }
        let target = PolicyTarget {
            account_id: Some(cash.account_id.clone()),
            instrument_id: None,
            currency_a: None,
            currency_b: None,
        };
        let Some(policy) = enabled_policy(&policies, FreshnessPolicyKind::AccountCash, &target)
        else {
            continue;
        };
        append_freshness_item(
            &mut items,
            today,
            &timezone,
            policy,
            &target,
            format!("{} ({})", account.name, cash.currency),
            Some(&cash.effective_at),
            &active_snoozes,
            Some(cash.currency.clone()),
        )?;
    }

    let mut instrument_quote_dates = HashMap::new();
    for quote in valuation_service::snapshot_instrument_quotes(&snapshot) {
        if quote.source_kind != "manual" || !eligible_timestamp(&quote.quoted_at, &now)? {
            continue;
        }
        let key = quote.instrument_id.clone();
        let candidate = (
            quote.quoted_at.clone(),
            quote.created_at.clone(),
            quote.id.clone(),
        );
        if instrument_quote_dates
            .get(&key)
            .is_none_or(|current: &(String, String, String)| candidate > *current)
        {
            instrument_quote_dates.insert(key, candidate);
        }
    }
    for instrument in valuation_service::snapshot_instruments(&snapshot).values() {
        if instrument.archived_at.is_some() || instrument.quote_preference != "manual" {
            continue;
        }
        let target = PolicyTarget {
            account_id: None,
            instrument_id: Some(instrument.id.clone()),
            currency_a: None,
            currency_b: None,
        };
        let Some(policy) = enabled_policy(&policies, FreshnessPolicyKind::InstrumentQuote, &target)
        else {
            continue;
        };
        append_freshness_item(
            &mut items,
            today,
            &timezone,
            policy,
            &target,
            instrument.name.clone(),
            instrument_quote_dates
                .get(&instrument.id)
                .map(|value| value.0.as_str()),
            &active_snoozes,
            None,
        )?;
    }

    let required_pairs = valuation_service::required_fx_pairs(&snapshot, &accounts)?;
    let mut fx_quote_dates = HashMap::new();
    for quote in valuation_service::snapshot_fx_quotes(&snapshot) {
        if quote.source_kind != "manual" || !eligible_timestamp(&quote.quoted_at, &now)? {
            continue;
        }
        let pair = FxPair::new(
            CurrencyCode::parse(&quote.base_currency)?,
            CurrencyCode::parse(&quote.quote_currency)?,
        )?;
        let key = (
            pair.currency_a().as_str().to_owned(),
            pair.currency_b().as_str().to_owned(),
        );
        let candidate = (
            quote.quoted_at.clone(),
            quote.created_at.clone(),
            quote.id.clone(),
        );
        if fx_quote_dates
            .get(&key)
            .is_none_or(|current: &(String, String, String)| candidate > *current)
        {
            fx_quote_dates.insert(key, candidate);
        }
    }
    for pair in required_pairs {
        if valuation_service::snapshot_fx_preference(&snapshot, &pair).as_str() != "manual" {
            continue;
        }
        let target = PolicyTarget {
            account_id: None,
            instrument_id: None,
            currency_a: Some(pair.currency_a().as_str().to_owned()),
            currency_b: Some(pair.currency_b().as_str().to_owned()),
        };
        let Some(policy) = enabled_policy(&policies, FreshnessPolicyKind::FxQuote, &target) else {
            continue;
        };
        let key = (
            pair.currency_a().as_str().to_owned(),
            pair.currency_b().as_str().to_owned(),
        );
        append_freshness_item(
            &mut items,
            today,
            &timezone,
            policy,
            &target,
            format!("{}/{}", key.0, key.1),
            fx_quote_dates.get(&key).map(|value| value.0.as_str()),
            &active_snoozes,
            None,
        )?;
    }

    for pending in pending_service::list_due_pending_activities_in_tx(
        tx,
        &household.id,
        &today.to_ymd(),
        DUE_PENDING_LIMIT,
    )
    .await?
    {
        let status = if pending.scheduled_local_date == today.to_ymd() {
            "due"
        } else {
            "overdue"
        };
        items.push(MaintenanceItemDto {
            id: format!("pending_activity:{}", pending.id),
            item_kind: "pending_activity".to_owned(),
            policy_id: None,
            policy_kind: None,
            target_account_id: None,
            target_instrument_id: None,
            target_currency_a: None,
            target_currency_b: None,
            label: "Pending Activity".to_owned(),
            underlying_status: status.to_owned(),
            status: status.to_owned(),
            observed_on: None,
            due_on: Some(pending.scheduled_local_date.clone()),
            snoozed_until: None,
            pending_activity: Some(pending),
        });
    }

    items.sort_by_key(|item| {
        (
            status_rank(&item.status),
            item.item_kind.clone(),
            item.policy_kind.clone().unwrap_or_default(),
            item.label.clone(),
            item.id.clone(),
        )
    });
    Ok(MaintenancePageDto {
        local_date: today.to_ymd(),
        items,
    })
}

async fn latest_account_value_dates(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    now: &Timestamp,
) -> Result<HashMap<String, String>, AppError> {
    query_count::record("maintenance.account_value_latest");
    let rows = sqlx::query(
        "SELECT account_id, effective_at
         FROM (
             SELECT av.account_id, av.effective_at,
                    ROW_NUMBER() OVER (
                        PARTITION BY av.account_id
                        ORDER BY av.effective_at DESC, av.created_at DESC, av.id DESC
                    ) AS rn
             FROM account_values av
             JOIN accounts a ON a.id = av.account_id
             WHERE a.household_id = ?
               AND a.archived_at IS NULL
               AND av.effective_at <= ?
         ) ranked
         WHERE rn = 1",
    )
    .bind(household_id)
    .bind(now.to_rfc3339())
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| AppError::DatabaseUnavailable)?;
    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get("account_id")
                    .map_err(|_| AppError::DatabaseUnavailable)?,
                row.try_get("effective_at")
                    .map_err(|_| AppError::DatabaseUnavailable)?,
            ))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn append_freshness_item(
    items: &mut Vec<MaintenanceItemDto>,
    today: CalendarDate,
    timezone: &HistoryTimezone,
    policy: &FreshnessPolicyRecord,
    target: &PolicyTarget,
    label: String,
    observed_at: Option<&str>,
    active_snoozes: &HashMap<TargetKey, String>,
    component_currency: Option<String>,
) -> Result<(), AppError> {
    let interval = policy.review_interval_days.ok_or(AppError::Internal)?;
    let observed_on = observed_at
        .map(|value| local_observation_date(timezone, value))
        .transpose()?;
    let (underlying_status, due_on) = freshness_status(today, observed_on.as_deref(), interval)?;
    let kind = FreshnessPolicyKind::parse(&policy.kind)?;
    let key = TargetKey::new(kind, target);
    let snoozed_until = active_snoozes.get(&key).cloned();
    let status = if snoozed_until.is_some() {
        "snoozed".to_owned()
    } else {
        underlying_status.to_owned()
    };
    let id = match kind {
        FreshnessPolicyKind::AccountValue => format!(
            "account_value:{}",
            target.account_id.as_deref().unwrap_or_default()
        ),
        FreshnessPolicyKind::AccountCash => format!(
            "account_cash:{}:{}",
            target.account_id.as_deref().unwrap_or_default(),
            component_currency.as_deref().unwrap_or_default()
        ),
        FreshnessPolicyKind::InstrumentQuote => format!(
            "instrument_quote:{}",
            target.instrument_id.as_deref().unwrap_or_default()
        ),
        FreshnessPolicyKind::FxQuote => format!(
            "fx_quote:{}/{}",
            target.currency_a.as_deref().unwrap_or_default(),
            target.currency_b.as_deref().unwrap_or_default()
        ),
    };
    items.push(MaintenanceItemDto {
        id,
        item_kind: "freshness".to_owned(),
        policy_id: Some(policy.id.clone()),
        policy_kind: Some(kind.as_str().to_owned()),
        target_account_id: target.account_id.clone(),
        target_instrument_id: target.instrument_id.clone(),
        target_currency_a: target.currency_a.clone(),
        target_currency_b: target.currency_b.clone(),
        label,
        underlying_status: underlying_status.to_owned(),
        status,
        observed_on,
        due_on,
        snoozed_until,
        pending_activity: None,
    });
    Ok(())
}

fn freshness_status(
    today: CalendarDate,
    observed_on: Option<&str>,
    interval_days: i64,
) -> Result<(&'static str, Option<String>), AppError> {
    let Some(observed_on) = observed_on else {
        return Ok(("missing", None));
    };
    let observed = CalendarDate::parse(observed_on)?;
    let due = observed
        .checked_add_days(interval_days)
        .ok_or(AppError::DatabaseUnavailable)?;
    let status = if today < due {
        "current"
    } else if today == due {
        "due"
    } else {
        "overdue"
    };
    Ok((status, Some(due.to_ymd())))
}

fn local_observation_date(timezone: &HistoryTimezone, timestamp: &str) -> Result<String, AppError> {
    let timestamp = Timestamp::parse(timestamp).map_err(|_| AppError::DatabaseUnavailable)?;
    Ok(timezone.local_date(&timestamp).to_ymd())
}

fn eligible_timestamp(timestamp: &str, now: &Timestamp) -> Result<bool, AppError> {
    let parsed = Timestamp::parse(timestamp).map_err(|_| AppError::DatabaseUnavailable)?;
    Ok(parsed <= now.clone())
}

fn status_rank(status: &str) -> u8 {
    match status {
        "overdue" => 0,
        "due" => 1,
        "missing" => 2,
        "snoozed" => 3,
        "current" => 4,
        _ => 5,
    }
}

fn active_snoozes(
    rows: &[MaintenanceSnoozeRecord],
    today: CalendarDate,
) -> Result<HashMap<TargetKey, String>, AppError> {
    let mut latest: HashMap<TargetKey, (String, String)> = HashMap::new();
    for row in rows {
        let kind = FreshnessPolicyKind::parse(&row.policy_kind)?;
        let target = record_target_from_snooze(row)?;
        CalendarDate::parse(&row.snoozed_until)?;
        let key = TargetKey::new(kind, &target);
        let replace = latest.get(&key).is_none_or(|current| {
            (row.created_at.clone(), row.id.clone()) > (current.0.clone(), current.1.clone())
        });
        if replace {
            latest.insert(key, (row.created_at.clone(), row.snoozed_until.clone()));
        }
    }
    let today = today.to_ymd();
    Ok(latest
        .into_iter()
        .filter_map(|(key, (_, until))| (until > today).then_some((key, until)))
        .collect())
}

fn enabled_policy<'a>(
    policies: &'a [FreshnessPolicyRecord],
    kind: FreshnessPolicyKind,
    target: &PolicyTarget,
) -> Option<&'a FreshnessPolicyRecord> {
    resolve_policy(policies, kind, target).filter(|policy| policy.review_interval_days.is_some())
}

fn resolve_policy<'a>(
    policies: &'a [FreshnessPolicyRecord],
    kind: FreshnessPolicyKind,
    target: &PolicyTarget,
) -> Option<&'a FreshnessPolicyRecord> {
    let key = TargetKey::new(kind, target);
    policies
        .iter()
        .find(|row| record_key(row).ok().as_ref() == Some(&key))
        .or_else(|| {
            let default_key = TargetKey::new(
                kind,
                &PolicyTarget {
                    account_id: None,
                    instrument_id: None,
                    currency_a: None,
                    currency_b: None,
                },
            );
            policies
                .iter()
                .find(|row| record_key(row).ok().as_ref() == Some(&default_key))
        })
}

fn parse_interval(interval: Option<i32>) -> Result<Option<i64>, AppError> {
    interval.map(i64::from).map_or(Ok(None), |value| {
        if (1..=3650).contains(&value) {
            Ok(Some(value))
        } else {
            Err(AppError::validation(
                "reviewIntervalDays",
                "The review interval must be between 1 and 3650 days, or disabled.",
            ))
        }
    })
}

fn normalize_target(
    kind: FreshnessPolicyKind,
    account_id: Option<String>,
    instrument_id: Option<String>,
    currency_a: Option<String>,
    currency_b: Option<String>,
) -> Result<PolicyTarget, AppError> {
    let has_account = account_id.is_some();
    let has_instrument = instrument_id.is_some();
    let has_currency = currency_a.is_some() || currency_b.is_some();
    let target_count =
        usize::from(has_account) + usize::from(has_instrument) + usize::from(has_currency);
    if target_count > 1 || (has_currency && (currency_a.is_none() || currency_b.is_none())) {
        return Err(AppError::validation(
            "target",
            "A freshness policy target must contain exactly one compatible target shape.",
        ));
    }
    if target_count == 0 {
        return Ok(PolicyTarget {
            account_id: None,
            instrument_id: None,
            currency_a: None,
            currency_b: None,
        });
    }
    match kind {
        FreshnessPolicyKind::AccountValue | FreshnessPolicyKind::AccountCash => {
            if !has_account || has_instrument || has_currency {
                return Err(AppError::validation(
                    "target",
                    "This policy kind requires an Account target.",
                ));
            }
            AccountId::parse(account_id.as_deref().unwrap_or_default())?;
            Ok(PolicyTarget {
                account_id,
                instrument_id: None,
                currency_a: None,
                currency_b: None,
            })
        }
        FreshnessPolicyKind::InstrumentQuote => {
            if !has_instrument || has_account || has_currency {
                return Err(AppError::validation(
                    "target",
                    "Instrument quote policies require an Instrument target.",
                ));
            }
            InstrumentId::parse(instrument_id.as_deref().unwrap_or_default())?;
            Ok(PolicyTarget {
                account_id: None,
                instrument_id,
                currency_a: None,
                currency_b: None,
            })
        }
        FreshnessPolicyKind::FxQuote => {
            if has_account || has_instrument {
                return Err(AppError::validation(
                    "target",
                    "FX quote policies require a normalized currency pair.",
                ));
            }
            let left = CurrencyCode::parse(currency_a.as_deref().unwrap_or_default())?;
            let right = CurrencyCode::parse(currency_b.as_deref().unwrap_or_default())?;
            let pair = FxPair::new(left, right)?;
            Ok(PolicyTarget {
                account_id: None,
                instrument_id: None,
                currency_a: Some(pair.currency_a().as_str().to_owned()),
                currency_b: Some(pair.currency_b().as_str().to_owned()),
            })
        }
    }
}

async fn validate_target_reference_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    kind: FreshnessPolicyKind,
    target: &PolicyTarget,
    allow_archived: bool,
) -> Result<(), AppError> {
    if target.is_default() {
        return Ok(());
    }
    match kind {
        FreshnessPolicyKind::AccountValue | FreshnessPolicyKind::AccountCash => {
            let id = target.account_id.as_deref().unwrap_or_default();
            let row = sqlx::query(
                "SELECT household_id, tracking_mode, archived_at
                 FROM accounts WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| AppError::DatabaseUnavailable)?
            .ok_or_else(|| AppError::not_found("account", id))?;
            let row_household: String = row
                .try_get("household_id")
                .map_err(|_| AppError::DatabaseUnavailable)?;
            if row_household != household_id {
                return Err(AppError::not_found("account", id));
            }
            let tracking_mode: String = row
                .try_get("tracking_mode")
                .map_err(|_| AppError::DatabaseUnavailable)?;
            let expected = match kind {
                FreshnessPolicyKind::AccountValue => [
                    TrackingMode::Balance.as_str(),
                    TrackingMode::ManualValue.as_str(),
                ],
                FreshnessPolicyKind::AccountCash => [TrackingMode::Holdings.as_str(), ""],
                _ => unreachable!(),
            };
            if tracking_mode != expected[0] && tracking_mode != expected[1] {
                return Err(AppError::validation(
                    "policyKind",
                    "The target Account tracking mode is incompatible with this policy kind.",
                ));
            }
            let archived_at: Option<String> = row
                .try_get("archived_at")
                .map_err(|_| AppError::DatabaseUnavailable)?;
            if archived_at.is_some() && !allow_archived {
                return Err(AppError::conflict(
                    "An archived Account cannot receive a new freshness policy or snooze.",
                ));
            }
        }
        FreshnessPolicyKind::InstrumentQuote => {
            let id = target.instrument_id.as_deref().unwrap_or_default();
            let row = sqlx::query("SELECT household_id, archived_at FROM instruments WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(|_| AppError::DatabaseUnavailable)?
                .ok_or_else(|| AppError::not_found("instrument", id))?;
            let row_household: String = row
                .try_get("household_id")
                .map_err(|_| AppError::DatabaseUnavailable)?;
            if row_household != household_id {
                return Err(AppError::not_found("instrument", id));
            }
            let archived_at: Option<String> = row
                .try_get("archived_at")
                .map_err(|_| AppError::DatabaseUnavailable)?;
            if archived_at.is_some() && !allow_archived {
                return Err(AppError::conflict(
                    "An archived Instrument cannot receive a new freshness policy or snooze.",
                ));
            }
        }
        FreshnessPolicyKind::FxQuote => {}
    }
    Ok(())
}

fn record_target(row: &FreshnessPolicyRecord) -> Result<PolicyTarget, AppError> {
    normalize_target(
        FreshnessPolicyKind::parse(&row.kind)?,
        row.target_account_id.clone(),
        row.target_instrument_id.clone(),
        row.target_currency_a.clone(),
        row.target_currency_b.clone(),
    )
}

fn record_target_from_snooze(row: &MaintenanceSnoozeRecord) -> Result<PolicyTarget, AppError> {
    normalize_target(
        FreshnessPolicyKind::parse(&row.policy_kind)?,
        row.target_account_id.clone(),
        row.target_instrument_id.clone(),
        row.target_currency_a.clone(),
        row.target_currency_b.clone(),
    )
}

fn record_key(row: &FreshnessPolicyRecord) -> Result<TargetKey, AppError> {
    let kind = FreshnessPolicyKind::parse(&row.kind)?;
    Ok(TargetKey::new(kind, &record_target(row)?))
}

fn policy_dto(row: &FreshnessPolicyRecord) -> Result<FreshnessPolicyDto, AppError> {
    FreshnessPolicyKind::parse(&row.kind)?;
    let interval = row
        .review_interval_days
        .map(|value| i32::try_from(value).map_err(|_| AppError::DatabaseUnavailable))
        .transpose()?;
    Ok(FreshnessPolicyDto {
        id: row.id.clone(),
        kind: row.kind.clone(),
        target_account_id: row.target_account_id.clone(),
        target_instrument_id: row.target_instrument_id.clone(),
        target_currency_a: row.target_currency_a.clone(),
        target_currency_b: row.target_currency_b.clone(),
        review_interval_days: interval,
        is_default: row.target_account_id.is_none()
            && row.target_instrument_id.is_none()
            && row.target_currency_a.is_none()
            && row.target_currency_b.is_none(),
        archived_at: row.archived_at.clone(),
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    })
}

fn snooze_dto(row: &MaintenanceSnoozeRecord) -> Result<MaintenanceSnoozeDto, AppError> {
    FreshnessPolicyKind::parse(&row.policy_kind)?;
    Ok(MaintenanceSnoozeDto {
        id: row.id.clone(),
        policy_kind: row.policy_kind.clone(),
        target_account_id: row.target_account_id.clone(),
        target_instrument_id: row.target_instrument_id.clone(),
        target_currency_a: row.target_currency_a.clone(),
        target_currency_b: row.target_currency_b.clone(),
        snoozed_until: row.snoozed_until.clone(),
        created_at: row.created_at.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        active_snoozes, freshness_status, list_freshness_policies, list_maintenance_items,
        normalize_target, parse_interval, snooze_maintenance_item, update_freshness_policy,
        SnoozeMaintenanceItemInput, UpdateFreshnessPolicyInput,
    };
    use crate::{
        application::{
            account_service::{
                archive_account, create_account, restore_account, update_account_value,
                CreateAccountInput, OwnershipShareInput, UpdateAccountValueInput,
            },
            instrument_service::{create_instrument, CreateInstrumentInput},
            member_service::list_members,
            query_count,
            quote_service::{set_fx_quote_preference, SetFxQuotePreferenceInput},
            sustainable_repositories::MaintenanceSnoozeRecord,
        },
        domain::{CalendarDate, FreshnessPolicyKind, HistoryTimezone, Timestamp},
        test_support::{cleanup, onboarded_state},
    };

    fn date(value: &str) -> CalendarDate {
        CalendarDate::parse(value).expect("date")
    }

    #[test]
    fn freshness_status_respects_exact_due_and_overdue_boundaries() {
        assert_eq!(
            freshness_status(date("2026-02-28"), Some("2026-01-31"), 28).expect("due"),
            ("due", Some("2026-02-28".to_owned()))
        );
        assert_eq!(
            freshness_status(date("2028-02-29"), Some("2028-01-31"), 29).expect("leap"),
            ("due", Some("2028-02-29".to_owned()))
        );
        assert_eq!(
            freshness_status(date("2026-02-27"), Some("2026-01-31"), 28).expect("current"),
            ("current", Some("2026-02-28".to_owned()))
        );
        assert_eq!(
            freshness_status(date("2026-02-28"), None, 28).expect("missing"),
            ("missing", None)
        );
    }

    #[test]
    fn target_normalization_rejects_wrong_shape_and_normalizes_fx_pair() {
        assert!(normalize_target(
            FreshnessPolicyKind::AccountValue,
            None,
            Some("11111111-1111-4111-8111-111111111111".to_owned()),
            None,
            None,
        )
        .is_err());
        let target = normalize_target(
            FreshnessPolicyKind::FxQuote,
            None,
            None,
            Some("USD".to_owned()),
            Some("CNY".to_owned()),
        )
        .expect("fx target");
        assert_eq!(target.currency_a.as_deref(), Some("CNY"));
        assert_eq!(target.currency_b.as_deref(), Some("USD"));
    }

    #[test]
    fn interval_is_bounded_or_disabled() {
        assert_eq!(parse_interval(None).expect("disabled"), None);
        assert_eq!(parse_interval(Some(3650)).expect("upper"), Some(3650));
        assert!(parse_interval(Some(0)).is_err());
        assert!(parse_interval(Some(3651)).is_err());
    }

    async fn bank_account_with_currency(
        state: &crate::state::AppState,
        name: &str,
        currency: &str,
    ) -> crate::application::account_service::AccountRecordDto {
        let member_id = list_members(state, false)
            .await
            .expect("members")
            .first()
            .expect("member")
            .id
            .clone();
        create_account(
            state,
            CreateAccountInput {
                name: name.to_owned(),
                primary_category: "cash_equivalent".to_owned(),
                secondary_category: "bank_account".to_owned(),
                default_currency: currency.to_owned(),
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
                initial_amount: Some("100".to_owned()),
            },
        )
        .await
        .expect("account")
    }

    async fn bank_account(
        state: &crate::state::AppState,
        name: &str,
    ) -> crate::application::account_service::AccountRecordDto {
        bank_account_with_currency(state, name, "CNY").await
    }

    async fn financial_counts(state: &crate::state::AppState) -> (i64, i64, i64, Option<String>) {
        let pool = state.writable_db().expect("db");
        let activities: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
            .fetch_one(pool)
            .await
            .expect("activities");
        let values: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM account_values")
            .fetch_one(pool)
            .await
            .expect("values");
        let cash: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM account_cash_values")
            .fetch_one(pool)
            .await
            .expect("cash");
        let dirty: Option<String> =
            sqlx::query_scalar("SELECT dirty_from FROM history_snapshot_state LIMIT 1")
                .fetch_one(pool)
                .await
                .expect("dirty");
        (activities, values, cash, dirty)
    }

    #[test]
    fn defaults_overrides_disabled_policy_snooze_and_archive_are_deterministic() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("maintenance-policy-boundaries").await;
            let account = bank_account(&state, "Reviewable Cash").await;
            let policies = list_freshness_policies(&state, false)
                .await
                .expect("policies");
            assert_eq!(policies.len(), 4);
            let default = policies
                .iter()
                .find(|policy| policy.kind == "account_value" && policy.is_default)
                .expect("account default")
                .clone();
            assert_eq!(default.review_interval_days, Some(30));

            let before = financial_counts(&state).await;
            update_freshness_policy(
                &state,
                UpdateFreshnessPolicyInput {
                    id: Some(default.id),
                    kind: "account_value".to_owned(),
                    target_account_id: None,
                    target_instrument_id: None,
                    target_currency_a: None,
                    target_currency_b: None,
                    review_interval_days: None,
                },
            )
            .await
            .expect("disable default");

            let override_policy = update_freshness_policy(
                &state,
                UpdateFreshnessPolicyInput {
                    id: None,
                    kind: "account_value".to_owned(),
                    target_account_id: Some(account.id.clone()),
                    target_instrument_id: None,
                    target_currency_a: None,
                    target_currency_b: None,
                    review_interval_days: Some(1),
                },
            )
            .await
            .expect("override");
            assert!(!override_policy.is_default);
            let queue = list_maintenance_items(&state).await.expect("queue");
            let account_item = queue
                .items
                .iter()
                .find(|item| item.target_account_id.as_deref() == Some(account.id.as_str()))
                .expect("override item");
            assert_eq!(
                account_item.policy_id.as_deref(),
                Some(override_policy.id.as_str())
            );
            assert_eq!(account_item.underlying_status, "current");

            let duplicate = update_freshness_policy(
                &state,
                UpdateFreshnessPolicyInput {
                    id: None,
                    kind: "account_value".to_owned(),
                    target_account_id: Some(account.id.clone()),
                    target_instrument_id: None,
                    target_currency_a: None,
                    target_currency_b: None,
                    review_interval_days: Some(2),
                },
            )
            .await
            .expect_err("duplicate override");
            assert!(matches!(duplicate, crate::error::AppError::Conflict { .. }));

            let snoozed_until = CalendarDate::parse(&queue.local_date)
                .expect("today")
                .checked_add_days(1)
                .expect("tomorrow")
                .to_ymd();
            snooze_maintenance_item(
                &state,
                SnoozeMaintenanceItemInput {
                    policy_kind: "account_value".to_owned(),
                    target_account_id: Some(account.id.clone()),
                    target_instrument_id: None,
                    target_currency_a: None,
                    target_currency_b: None,
                    snoozed_until,
                },
            )
            .await
            .expect("snooze");
            let snoozed_queue = list_maintenance_items(&state).await.expect("snoozed queue");
            let snoozed = snoozed_queue
                .items
                .iter()
                .find(|item| item.target_account_id.as_deref() == Some(account.id.as_str()))
                .expect("snoozed item");
            assert_eq!(snoozed.status, "snoozed");
            assert_eq!(snoozed.underlying_status, "current");

            let after = financial_counts(&state).await;
            assert_eq!(before, after);
            let snooze_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM maintenance_snoozes")
                .fetch_one(state.writable_db().expect("db"))
                .await
                .expect("snooze count");
            assert_eq!(snooze_count, 1);

            let values_before_observation: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM account_values WHERE account_id = ?")
                    .bind(&account.id)
                    .fetch_one(state.writable_db().expect("db"))
                    .await
                    .expect("value count");
            update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: account.id.clone(),
                    amount: "125".to_owned(),
                },
            )
            .await
            .expect("new observation");
            let refreshed = list_maintenance_items(&state)
                .await
                .expect("refreshed queue");
            let refreshed_item = refreshed
                .items
                .iter()
                .find(|item| item.target_account_id.as_deref() == Some(account.id.as_str()))
                .expect("refreshed item");
            assert_eq!(refreshed_item.underlying_status, "current");
            assert_eq!(refreshed_item.status, "snoozed");
            let values_after_observation: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM account_values WHERE account_id = ?")
                    .bind(&account.id)
                    .fetch_one(state.writable_db().expect("db"))
                    .await
                    .expect("value count");
            assert_eq!(values_after_observation, values_before_observation + 1);
            let retained_snooze_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM maintenance_snoozes")
                    .fetch_one(state.writable_db().expect("db"))
                    .await
                    .expect("retained snooze count");
            assert_eq!(retained_snooze_count, 1);

            let wrong_kind = update_freshness_policy(
                &state,
                UpdateFreshnessPolicyInput {
                    id: None,
                    kind: "account_cash".to_owned(),
                    target_account_id: Some(account.id.clone()),
                    target_instrument_id: None,
                    target_currency_a: None,
                    target_currency_b: None,
                    review_interval_days: Some(1),
                },
            )
            .await
            .expect_err("wrong tracking kind");
            assert!(matches!(
                wrong_kind,
                crate::error::AppError::Validation { .. }
            ));

            archive_account(&state, &account.id).await.expect("archive");
            let archived_new = update_freshness_policy(
                &state,
                UpdateFreshnessPolicyInput {
                    id: None,
                    kind: "account_value".to_owned(),
                    target_account_id: Some(account.id.clone()),
                    target_instrument_id: None,
                    target_currency_a: None,
                    target_currency_b: None,
                    review_interval_days: Some(1),
                },
            )
            .await
            .expect_err("archived target");
            assert!(matches!(
                archived_new,
                crate::error::AppError::Conflict { .. }
            ));
            let archived_queue = list_maintenance_items(&state)
                .await
                .expect("archived queue");
            assert!(!archived_queue
                .items
                .iter()
                .any(|item| item.target_account_id.as_deref() == Some(account.id.as_str())));
            restore_account(&state, &account.id).await.expect("restore");
            let restored_queue = list_maintenance_items(&state)
                .await
                .expect("restored queue");
            assert!(restored_queue
                .items
                .iter()
                .any(|item| item.target_account_id.as_deref() == Some(account.id.as_str())));

            cleanup(&path);
        });
    }

    #[test]
    fn manual_preference_controls_instrument_and_required_fx_maintenance() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("maintenance-manual-preference").await;
            let usd = bank_account_with_currency(&state, "USD Cash", "USD").await;
            let manual = create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "Manual Asset".to_owned(),
                    symbol: Some("MAN".to_owned()),
                    instrument_type: "stock".to_owned(),
                    quote_currency: "CNY".to_owned(),
                    market_code: None,
                    country_code: None,
                    isin: None,
                    provider_key: None,
                    provider_symbol: None,
                    quote_preference: Some("manual".to_owned()),
                    note: None,
                },
            )
            .await
            .expect("manual instrument");
            let provider = create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "Provider Asset".to_owned(),
                    symbol: Some("PRO".to_owned()),
                    instrument_type: "stock".to_owned(),
                    quote_currency: "CNY".to_owned(),
                    market_code: None,
                    country_code: None,
                    isin: None,
                    provider_key: Some("fake".to_owned()),
                    provider_symbol: Some("PRO".to_owned()),
                    quote_preference: Some("provider".to_owned()),
                    note: None,
                },
            )
            .await
            .expect("provider instrument");

            let queue = list_maintenance_items(&state).await.expect("queue");
            let manual_item = queue
                .items
                .iter()
                .find(|item| item.target_instrument_id.as_deref() == Some(manual.id.as_str()))
                .expect("manual quote item");
            assert_eq!(manual_item.policy_kind.as_deref(), Some("instrument_quote"));
            assert_eq!(manual_item.underlying_status, "missing");
            assert!(!queue
                .items
                .iter()
                .any(|item| item.target_instrument_id.as_deref() == Some(provider.id.as_str())));
            let fx_item = queue
                .items
                .iter()
                .find(|item| {
                    item.policy_kind.as_deref() == Some("fx_quote")
                        && item.target_currency_a.as_deref() == Some("CNY")
                        && item.target_currency_b.as_deref() == Some("USD")
                })
                .expect("manual fx item");
            assert_eq!(fx_item.underlying_status, "missing");

            set_fx_quote_preference(
                &state,
                SetFxQuotePreferenceInput {
                    currency_a: "USD".to_owned(),
                    currency_b: "CNY".to_owned(),
                    quote_preference: "provider".to_owned(),
                },
            )
            .await
            .expect("provider preference");
            let provider_queue = list_maintenance_items(&state)
                .await
                .expect("provider queue");
            assert!(!provider_queue.items.iter().any(|item| {
                item.policy_kind.as_deref() == Some("fx_quote")
                    && item.target_currency_a.as_deref() == Some("CNY")
                    && item.target_currency_b.as_deref() == Some("USD")
            }));
            assert!(!provider_queue
                .items
                .iter()
                .any(|item| item.target_instrument_id.as_deref() == Some(provider.id.as_str())));
            assert!(provider.id != usd.id);
            cleanup(&path);
        });
    }

    #[test]
    fn snooze_expiry_and_history_timezone_use_calendar_dates() {
        let timezone = HistoryTimezone::parse("America/New_York").expect("timezone");
        let dst_day = Timestamp::parse("2026-03-08T07:00:00Z").expect("timestamp");
        assert_eq!(timezone.local_date(&dst_day).to_ymd(), "2026-03-08");

        let rows = vec![MaintenanceSnoozeRecord {
            id: "11111111-1111-4111-8111-111111111111".to_owned(),
            household_id: "22222222-2222-4222-8222-222222222222".to_owned(),
            policy_kind: "account_value".to_owned(),
            target_account_id: Some("33333333-3333-4333-8333-333333333333".to_owned()),
            target_instrument_id: None,
            target_currency_a: None,
            target_currency_b: None,
            snoozed_until: "2026-08-21".to_owned(),
            created_at: "2026-08-20T00:00:00.000Z".to_owned(),
        }];
        assert_eq!(
            active_snoozes(&rows, CalendarDate::parse("2026-08-20").expect("today"))
                .expect("active")
                .len(),
            1
        );
        assert!(
            active_snoozes(&rows, CalendarDate::parse("2026-08-21").expect("expiry"))
                .expect("expired")
                .is_empty()
        );
    }

    #[test]
    fn maintenance_reads_each_observation_family_in_bounded_batches() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("maintenance-query-count").await;
            let _ = bank_account(&state, "Batch Cash").await;
            let (result, families) =
                query_count::capture_async(|| async { list_maintenance_items(&state).await }).await;
            result.expect("maintenance queue");
            for family in [
                "maintenance.account_value_latest",
                "sustainable.policy_list",
                "sustainable.snooze_list",
                "sustainable.pending_due_list",
            ] {
                assert_eq!(families.iter().filter(|value| **value == family).count(), 1);
            }
            cleanup(&path);
        });
    }
}
