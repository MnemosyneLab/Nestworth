use sqlx::{Row, Sqlite, Transaction};

use super::reference::{map_read_error, map_write_error};
use crate::{
    domain::{
        Activity, ActivityId, ActivityKind, ActivityLeg, CalendarDate, ComponentKind, Direction,
        FeeKind, FxRate, HoldingId, HouseholdId, IncomeKind, InstrumentId, LegComponent, LegRole,
        Money, Quantity,
    },
    error::AppError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryOriginRecord {
    pub id: String,
    pub household_id: String,
    pub timezone: String,
    pub timezone_confirmed: bool,
    pub origin_at: String,
    pub origin_local_date: String,
    pub source: String,
    pub schema_version: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginAccountValueRecord {
    pub origin_id: String,
    pub account_id: String,
    pub amount: String,
    pub currency: String,
    pub value_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginCashValueRecord {
    pub origin_id: String,
    pub account_id: String,
    pub amount: String,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginHoldingRecord {
    pub origin_id: String,
    pub holding_id: String,
    pub account_id: String,
    pub instrument_id: String,
    pub quantity: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginAccountStateRecord {
    pub origin_id: String,
    pub account_id: String,
    pub primary_category: String,
    pub secondary_category: String,
    pub tracking_mode: String,
    pub include_in_net_worth: bool,
    pub include_in_investment: bool,
    pub include_in_liquid_assets: bool,
    pub archived_at: Option<String>,
    pub institution_id: Option<String>,
    pub group_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginOwnershipRecord {
    pub origin_id: String,
    pub account_id: String,
    pub member_id: String,
    pub share_bps: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityListCursor {
    pub effective_at: String,
    pub created_at: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldingQuantityRecord {
    pub id: String,
    pub holding_id: String,
    pub quantity: String,
    pub effective_at: String,
    pub created_at: String,
    pub activity_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountStateObservationRecord {
    pub id: String,
    pub account_id: String,
    pub primary_category: String,
    pub secondary_category: String,
    pub tracking_mode: String,
    pub include_in_net_worth: bool,
    pub include_in_investment: bool,
    pub include_in_liquid_assets: bool,
    pub archived_at: Option<String>,
    pub institution_id: Option<String>,
    pub group_id: Option<String>,
    pub effective_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountStateOwnershipRecord {
    pub observation_id: String,
    pub member_id: String,
    pub share_bps: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldingStateObservationRecord {
    pub id: String,
    pub holding_id: String,
    pub active: bool,
    pub archived_at: Option<String>,
    pub effective_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentPreferenceObservationRecord {
    pub id: String,
    pub instrument_id: String,
    pub quote_preference: String,
    pub effective_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FxPreferenceObservationRecord {
    pub id: String,
    pub household_id: String,
    pub currency_a: String,
    pub currency_b: String,
    pub source_kind: String,
    pub effective_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotStateRecord {
    pub household_id: String,
    pub dirty_from: Option<String>,
    pub last_completed_on: Option<String>,
    pub rebuild_status: String,
    pub rebuild_cursor_on: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyValuationSnapshotRecord {
    pub id: String,
    pub household_id: String,
    pub snapshot_on: String,
    pub cutoff_at: String,
    pub revision: i64,
    pub supersedes_snapshot_id: Option<String>,
    pub assets_amount: String,
    pub liabilities_amount: String,
    pub net_worth_amount: String,
    pub currency: String,
    pub is_complete: bool,
    pub valued_component_count: i64,
    pub total_component_count: i64,
    pub coverage_bps: i64,
    pub generation_reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyValuationSnapshotItemRecord {
    pub id: String,
    pub snapshot_id: String,
    pub account_id: String,
    pub holding_id: Option<String>,
    pub instrument_id: Option<String>,
    pub component_kind: String,
    pub native_amount: Option<String>,
    pub native_currency: Option<String>,
    pub base_amount: Option<String>,
    pub instrument_quote_id: Option<String>,
    pub fx_quote_id: Option<String>,
    pub account_state_observation_id: Option<String>,
    pub origin_id: Option<String>,
    pub activity_id: Option<String>,
    pub is_complete: bool,
    pub missing_reason: Option<String>,
    pub sort_order: i64,
}

fn flag(value: i64) -> bool {
    value != 0
}

pub async fn get_origin_by_household(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Option<HistoryOriginRecord>, AppError> {
    let row = sqlx::query(
        "SELECT id, household_id, timezone, timezone_confirmed, origin_at, origin_local_date, source, schema_version, created_at
         FROM history_origins WHERE household_id = ?",
    )
    .bind(household_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_load_failed", error))?;
    row.map(origin_from_row).transpose()
}

pub async fn insert_origin(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO history_origins (
            id, household_id, timezone, timezone_confirmed, origin_at, origin_local_date, source, schema_version, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&origin.id)
    .bind(&origin.household_id)
    .bind(&origin.timezone)
    .bind(i64::from(origin.timezone_confirmed))
    .bind(&origin.origin_at)
    .bind(&origin.origin_local_date)
    .bind(&origin.source)
    .bind(origin.schema_version)
    .bind(&origin.created_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.origin_insert_failed", error))?;
    Ok(())
}

pub async fn update_origin_timezone(
    tx: &mut Transaction<'_, Sqlite>,
    origin_id: &str,
    timezone: &str,
    origin_local_date: &str,
) -> Result<(), AppError> {
    let updated = sqlx::query(
        "UPDATE history_origins
         SET timezone = ?, timezone_confirmed = 1, origin_local_date = ?
         WHERE id = ? AND timezone_confirmed = 0",
    )
    .bind(timezone)
    .bind(origin_local_date)
    .bind(origin_id)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.origin_timezone_update_failed", error))?;
    if updated.rows_affected() != 1 {
        return Err(AppError::conflict(
            "The history timezone can no longer be changed.",
        ));
    }
    Ok(())
}

pub async fn insert_origin_account_value(
    tx: &mut Transaction<'_, Sqlite>,
    row: &OriginAccountValueRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO history_origin_account_values (origin_id, account_id, amount, currency, value_kind)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&row.origin_id)
    .bind(&row.account_id)
    .bind(&row.amount)
    .bind(&row.currency)
    .bind(&row.value_kind)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.origin_account_value_insert_failed", error))?;
    Ok(())
}

pub async fn insert_origin_cash_value(
    tx: &mut Transaction<'_, Sqlite>,
    row: &OriginCashValueRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO history_origin_cash_values (origin_id, account_id, amount, currency)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&row.origin_id)
    .bind(&row.account_id)
    .bind(&row.amount)
    .bind(&row.currency)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.origin_cash_value_insert_failed", error))?;
    Ok(())
}

pub async fn insert_origin_holding(
    tx: &mut Transaction<'_, Sqlite>,
    row: &OriginHoldingRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO history_origin_holdings (origin_id, holding_id, account_id, instrument_id, quantity, active)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.origin_id)
    .bind(&row.holding_id)
    .bind(&row.account_id)
    .bind(&row.instrument_id)
    .bind(&row.quantity)
    .bind(i64::from(row.active))
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.origin_holding_insert_failed", error))?;
    Ok(())
}

pub async fn insert_origin_account_state(
    tx: &mut Transaction<'_, Sqlite>,
    row: &OriginAccountStateRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO history_origin_account_states (
            origin_id, account_id, primary_category, secondary_category, tracking_mode,
            include_in_net_worth, include_in_investment, include_in_liquid_assets,
            archived_at, institution_id, group_id
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.origin_id)
    .bind(&row.account_id)
    .bind(&row.primary_category)
    .bind(&row.secondary_category)
    .bind(&row.tracking_mode)
    .bind(i64::from(row.include_in_net_worth))
    .bind(i64::from(row.include_in_investment))
    .bind(i64::from(row.include_in_liquid_assets))
    .bind(&row.archived_at)
    .bind(&row.institution_id)
    .bind(&row.group_id)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.origin_account_state_insert_failed", error))?;
    Ok(())
}

pub async fn insert_origin_ownership(
    tx: &mut Transaction<'_, Sqlite>,
    row: &OriginOwnershipRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO history_origin_ownership (origin_id, account_id, member_id, share_bps)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&row.origin_id)
    .bind(&row.account_id)
    .bind(&row.member_id)
    .bind(row.share_bps)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.origin_ownership_insert_failed", error))?;
    Ok(())
}

pub async fn list_origin_account_values(
    tx: &mut Transaction<'_, Sqlite>,
    origin_id: &str,
) -> Result<Vec<OriginAccountValueRecord>, AppError> {
    sqlx::query(
        "SELECT origin_id, account_id, amount, currency, value_kind
         FROM history_origin_account_values WHERE origin_id = ? ORDER BY account_id",
    )
    .bind(origin_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_account_values_load_failed", error))?
    .into_iter()
    .map(|row| {
        Ok(OriginAccountValueRecord {
            origin_id: required_text(&row, "origin_id")?,
            account_id: required_text(&row, "account_id")?,
            amount: required_text(&row, "amount")?,
            currency: required_text(&row, "currency")?,
            value_kind: required_text(&row, "value_kind")?,
        })
    })
    .collect()
}

pub async fn list_origin_cash_values(
    tx: &mut Transaction<'_, Sqlite>,
    origin_id: &str,
) -> Result<Vec<OriginCashValueRecord>, AppError> {
    sqlx::query(
        "SELECT origin_id, account_id, amount, currency
         FROM history_origin_cash_values WHERE origin_id = ? ORDER BY account_id, currency",
    )
    .bind(origin_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_cash_values_load_failed", error))?
    .into_iter()
    .map(|row| {
        Ok(OriginCashValueRecord {
            origin_id: required_text(&row, "origin_id")?,
            account_id: required_text(&row, "account_id")?,
            amount: required_text(&row, "amount")?,
            currency: required_text(&row, "currency")?,
        })
    })
    .collect()
}

pub async fn list_origin_holdings(
    tx: &mut Transaction<'_, Sqlite>,
    origin_id: &str,
) -> Result<Vec<OriginHoldingRecord>, AppError> {
    sqlx::query(
        "SELECT origin_id, holding_id, account_id, instrument_id, quantity, active
         FROM history_origin_holdings WHERE origin_id = ? ORDER BY holding_id",
    )
    .bind(origin_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_holdings_load_failed", error))?
    .into_iter()
    .map(|row| {
        Ok(OriginHoldingRecord {
            origin_id: required_text(&row, "origin_id")?,
            holding_id: required_text(&row, "holding_id")?,
            account_id: required_text(&row, "account_id")?,
            instrument_id: required_text(&row, "instrument_id")?,
            quantity: required_text(&row, "quantity")?,
            active: flag(required_i64(&row, "active")?),
        })
    })
    .collect()
}

pub async fn list_origin_account_states(
    tx: &mut Transaction<'_, Sqlite>,
    origin_id: &str,
) -> Result<Vec<OriginAccountStateRecord>, AppError> {
    sqlx::query(
        "SELECT origin_id, account_id, primary_category, secondary_category, tracking_mode,
                include_in_net_worth, include_in_investment, include_in_liquid_assets,
                archived_at, institution_id, group_id
         FROM history_origin_account_states WHERE origin_id = ? ORDER BY account_id",
    )
    .bind(origin_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_account_states_load_failed", error))?
    .into_iter()
    .map(origin_account_state_from_row)
    .collect()
}

pub async fn list_origin_ownership(
    tx: &mut Transaction<'_, Sqlite>,
    origin_id: &str,
) -> Result<Vec<OriginOwnershipRecord>, AppError> {
    sqlx::query(
        "SELECT origin_id, account_id, member_id, share_bps
         FROM history_origin_ownership WHERE origin_id = ? ORDER BY account_id, member_id",
    )
    .bind(origin_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.origin_ownership_load_failed", error))?
    .into_iter()
    .map(|row| {
        Ok(OriginOwnershipRecord {
            origin_id: required_text(&row, "origin_id")?,
            account_id: required_text(&row, "account_id")?,
            member_id: required_text(&row, "member_id")?,
            share_bps: required_i64(&row, "share_bps")?,
        })
    })
    .collect()
}

pub async fn snapshot_state_exists(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<bool, AppError> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM history_snapshot_state WHERE household_id = ?")
            .bind(household_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| map_read_error("history.snapshot_state_exists_failed", error))?;
    Ok(count > 0)
}

pub async fn insert_activity(
    tx: &mut Transaction<'_, Sqlite>,
    activity: &Activity,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO activities (
            id, household_id, kind, effective_at, effective_local_date, created_at, note,
            reverses, corrects, correction_group, income_kind, fee_kind, related_instrument_id
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(activity.id().to_string())
    .bind(activity.household_id().to_string())
    .bind(activity.kind().as_str())
    .bind(activity.effective_at().to_rfc3339())
    .bind(activity.effective_local_date().to_ymd())
    .bind(activity.created_at().to_rfc3339())
    .bind(activity.note())
    .bind(activity.reverses().map(|id| id.to_string()))
    .bind(activity.corrects().map(|id| id.to_string()))
    .bind(activity.correction_group().map(|id| id.to_string()))
    .bind(activity.income_kind().map(IncomeKind::as_str))
    .bind(activity.fee_kind().map(FeeKind::as_str))
    .bind(activity.related_instrument_id().map(|id| id.to_string()))
    .execute(&mut **tx)
    .await
    .map_err(map_activity_insert_error)?;

    for leg in activity.legs() {
        let (amount, currency, holding_id, instrument_id, quantity) = match leg.component() {
            LegComponent::AccountValue { amount } | LegComponent::HoldingsCash { amount } => (
                Some(amount.canonical_amount()),
                Some(amount.currency().as_str().to_owned()),
                None,
                None,
                None,
            ),
            LegComponent::HoldingQuantity {
                instrument_id,
                holding_id,
                quantity,
            } => (
                None,
                None,
                Some(holding_id.to_string()),
                Some(instrument_id.to_string()),
                Some(quantity.canonical()),
            ),
        };
        sqlx::query(
            "INSERT INTO activity_legs (
                id, activity_id, account_id, role, direction, component_kind,
                amount, currency, holding_id, instrument_id, quantity, fx_rate, sort_order
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(leg.id().to_string())
        .bind(leg.activity_id().to_string())
        .bind(leg.account_id().to_string())
        .bind(leg.role().as_str())
        .bind(leg.direction().as_str())
        .bind(leg.component_kind().as_str())
        .bind(amount)
        .bind(currency)
        .bind(holding_id)
        .bind(instrument_id)
        .bind(quantity)
        .bind(leg.fx_rate().map(|rate| rate.canonical()))
        .bind(leg.sort_order())
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("history.activity_leg_insert_failed", error))?;
    }
    Ok(())
}

pub async fn get_activity(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> Result<Option<Activity>, AppError> {
    let Some(header) = sqlx::query(
        "SELECT id, household_id, kind, effective_at, effective_local_date, created_at, note,
                reverses, corrects, correction_group, income_kind, fee_kind, related_instrument_id
         FROM activities WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.activity_load_failed", error))?
    else {
        return Ok(None);
    };
    let legs = load_legs_for_activity(tx, id).await?;
    Ok(Some(activity_from_row(header, legs)?))
}

pub async fn get_activity_by_reverses(
    tx: &mut Transaction<'_, Sqlite>,
    original_id: &str,
) -> Result<Option<Activity>, AppError> {
    let Some(header) = sqlx::query(
        "SELECT id, household_id, kind, effective_at, effective_local_date, created_at, note,
                reverses, corrects, correction_group, income_kind, fee_kind, related_instrument_id
         FROM activities WHERE reverses = ?",
    )
    .bind(original_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.activity_reversal_load_failed", error))?
    else {
        return Ok(None);
    };
    let id = required_text(&header, "id")?;
    let legs = load_legs_for_activity(tx, &id).await?;
    Ok(Some(activity_from_row(header, legs)?))
}

pub async fn get_activity_by_corrects(
    tx: &mut Transaction<'_, Sqlite>,
    original_id: &str,
) -> Result<Option<Activity>, AppError> {
    let Some(header) = sqlx::query(
        "SELECT id, household_id, kind, effective_at, effective_local_date, created_at, note,
                reverses, corrects, correction_group, income_kind, fee_kind, related_instrument_id
         FROM activities WHERE corrects = ?
         ORDER BY created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(original_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.activity_replacement_load_failed", error))?
    else {
        return Ok(None);
    };
    let id = required_text(&header, "id")?;
    let legs = load_legs_for_activity(tx, &id).await?;
    Ok(Some(activity_from_row(header, legs)?))
}

pub async fn list_all_activities_asc(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Vec<Activity>, AppError> {
    let rows = sqlx::query(
        "SELECT id, household_id, kind, effective_at, effective_local_date, created_at, note,
                reverses, corrects, correction_group, income_kind, fee_kind, related_instrument_id
         FROM activities
         WHERE household_id = ?
         ORDER BY effective_at ASC, created_at ASC, id ASC",
    )
    .bind(household_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.activity_replay_list_failed", error))?;

    let mut activities = Vec::with_capacity(rows.len());
    for header in rows {
        let id = required_text(&header, "id")?;
        let legs = load_legs_for_activity(tx, &id).await?;
        activities.push(activity_from_row(header, legs)?);
    }
    Ok(activities)
}

pub async fn list_activities_desc(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    cursor: Option<&ActivityListCursor>,
    limit: i64,
) -> Result<Vec<Activity>, AppError> {
    let rows = if let Some(cursor) = cursor {
        sqlx::query(
            "SELECT id, household_id, kind, effective_at, effective_local_date, created_at, note,
                    reverses, corrects, correction_group, income_kind, fee_kind, related_instrument_id
             FROM activities
             WHERE household_id = ?
               AND (
                    effective_at < ?
                    OR (effective_at = ? AND created_at < ?)
                    OR (effective_at = ? AND created_at = ? AND id < ?)
               )
             ORDER BY effective_at DESC, created_at DESC, id DESC
             LIMIT ?",
        )
        .bind(household_id)
        .bind(&cursor.effective_at)
        .bind(&cursor.effective_at)
        .bind(&cursor.created_at)
        .bind(&cursor.effective_at)
        .bind(&cursor.created_at)
        .bind(&cursor.id)
        .bind(limit)
        .fetch_all(&mut **tx)
        .await
    } else {
        sqlx::query(
            "SELECT id, household_id, kind, effective_at, effective_local_date, created_at, note,
                    reverses, corrects, correction_group, income_kind, fee_kind, related_instrument_id
             FROM activities
             WHERE household_id = ?
             ORDER BY effective_at DESC, created_at DESC, id DESC
             LIMIT ?",
        )
        .bind(household_id)
        .bind(limit)
        .fetch_all(&mut **tx)
        .await
    }
    .map_err(|error| map_read_error("history.activity_list_failed", error))?;

    let mut activities = Vec::with_capacity(rows.len());
    for header in rows {
        let id = required_text(&header, "id")?;
        let legs = load_legs_for_activity(tx, &id).await?;
        activities.push(activity_from_row(header, legs)?);
    }
    Ok(activities)
}

pub async fn count_activities(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<i64, AppError> {
    sqlx::query_scalar("SELECT COUNT(*) FROM activities WHERE household_id = ?")
        .bind(household_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| map_read_error("history.activity_count_failed", error))
}

pub async fn count_snapshots(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<i64, AppError> {
    sqlx::query_scalar("SELECT COUNT(*) FROM daily_valuation_snapshots WHERE household_id = ?")
        .bind(household_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| map_read_error("history.snapshot_count_failed", error))
}

pub async fn insert_holding_quantity(
    tx: &mut Transaction<'_, Sqlite>,
    row: &HoldingQuantityRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO holding_quantity_values (id, holding_id, quantity, effective_at, created_at, activity_id)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.holding_id)
    .bind(&row.quantity)
    .bind(&row.effective_at)
    .bind(&row.created_at)
    .bind(&row.activity_id)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.holding_quantity_insert_failed", error))?;
    Ok(())
}

pub async fn latest_holding_quantity_at(
    tx: &mut Transaction<'_, Sqlite>,
    holding_id: &str,
    cutoff_at: &str,
) -> Result<Option<HoldingQuantityRecord>, AppError> {
    let row = sqlx::query(
        "SELECT id, holding_id, quantity, effective_at, created_at, activity_id
         FROM holding_quantity_values
         WHERE holding_id = ? AND effective_at <= ?
         ORDER BY effective_at DESC, created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(holding_id)
    .bind(cutoff_at)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.holding_quantity_load_failed", error))?;
    row.map(|row| {
        Ok(HoldingQuantityRecord {
            id: required_text(&row, "id")?,
            holding_id: required_text(&row, "holding_id")?,
            quantity: required_text(&row, "quantity")?,
            effective_at: required_text(&row, "effective_at")?,
            created_at: required_text(&row, "created_at")?,
            activity_id: optional_text(&row, "activity_id")?,
        })
    })
    .transpose()
}

pub async fn insert_account_state_observation(
    tx: &mut Transaction<'_, Sqlite>,
    row: &AccountStateObservationRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO account_state_observations (
            id, account_id, primary_category, secondary_category, tracking_mode,
            include_in_net_worth, include_in_investment, include_in_liquid_assets,
            archived_at, institution_id, group_id, effective_at, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.account_id)
    .bind(&row.primary_category)
    .bind(&row.secondary_category)
    .bind(&row.tracking_mode)
    .bind(i64::from(row.include_in_net_worth))
    .bind(i64::from(row.include_in_investment))
    .bind(i64::from(row.include_in_liquid_assets))
    .bind(&row.archived_at)
    .bind(&row.institution_id)
    .bind(&row.group_id)
    .bind(&row.effective_at)
    .bind(&row.created_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.account_state_insert_failed", error))?;
    Ok(())
}

pub async fn insert_account_state_ownership(
    tx: &mut Transaction<'_, Sqlite>,
    row: &AccountStateOwnershipRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO account_state_ownership (observation_id, member_id, share_bps)
         VALUES (?, ?, ?)",
    )
    .bind(&row.observation_id)
    .bind(&row.member_id)
    .bind(row.share_bps)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.account_state_ownership_insert_failed", error))?;
    Ok(())
}

pub async fn latest_account_state_at(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    cutoff_at: &str,
) -> Result<Option<AccountStateObservationRecord>, AppError> {
    let row = sqlx::query(
        "SELECT id, account_id, primary_category, secondary_category, tracking_mode,
                include_in_net_worth, include_in_investment, include_in_liquid_assets,
                archived_at, institution_id, group_id, effective_at, created_at
         FROM account_state_observations
         WHERE account_id = ? AND effective_at <= ?
         ORDER BY effective_at DESC, created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(account_id)
    .bind(cutoff_at)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.account_state_load_failed", error))?;
    row.map(account_state_from_row).transpose()
}

pub async fn list_account_state_ownership(
    tx: &mut Transaction<'_, Sqlite>,
    observation_id: &str,
) -> Result<Vec<AccountStateOwnershipRecord>, AppError> {
    sqlx::query(
        "SELECT observation_id, member_id, share_bps
         FROM account_state_ownership WHERE observation_id = ? ORDER BY member_id",
    )
    .bind(observation_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.account_state_ownership_load_failed", error))?
    .into_iter()
    .map(|row| {
        Ok(AccountStateOwnershipRecord {
            observation_id: required_text(&row, "observation_id")?,
            member_id: required_text(&row, "member_id")?,
            share_bps: required_i64(&row, "share_bps")?,
        })
    })
    .collect()
}

pub async fn insert_holding_state_observation(
    tx: &mut Transaction<'_, Sqlite>,
    row: &HoldingStateObservationRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO holding_state_observations (id, holding_id, active, archived_at, effective_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.holding_id)
    .bind(i64::from(row.active))
    .bind(&row.archived_at)
    .bind(&row.effective_at)
    .bind(&row.created_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.holding_state_insert_failed", error))?;
    Ok(())
}

pub async fn latest_holding_state_at(
    tx: &mut Transaction<'_, Sqlite>,
    holding_id: &str,
    cutoff_at: &str,
) -> Result<Option<HoldingStateObservationRecord>, AppError> {
    let row = sqlx::query(
        "SELECT id, holding_id, active, archived_at, effective_at, created_at
         FROM holding_state_observations
         WHERE holding_id = ? AND effective_at <= ?
         ORDER BY effective_at DESC, created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(holding_id)
    .bind(cutoff_at)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.holding_state_load_failed", error))?;
    row.map(|row| {
        Ok(HoldingStateObservationRecord {
            id: required_text(&row, "id")?,
            holding_id: required_text(&row, "holding_id")?,
            active: flag(required_i64(&row, "active")?),
            archived_at: optional_text(&row, "archived_at")?,
            effective_at: required_text(&row, "effective_at")?,
            created_at: required_text(&row, "created_at")?,
        })
    })
    .transpose()
}

pub async fn insert_instrument_preference_observation(
    tx: &mut Transaction<'_, Sqlite>,
    row: &InstrumentPreferenceObservationRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO instrument_preference_observations (id, instrument_id, quote_preference, effective_at, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.instrument_id)
    .bind(&row.quote_preference)
    .bind(&row.effective_at)
    .bind(&row.created_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.instrument_preference_insert_failed", error))?;
    Ok(())
}

pub async fn latest_instrument_preference_at(
    tx: &mut Transaction<'_, Sqlite>,
    instrument_id: &str,
    cutoff_at: &str,
) -> Result<Option<InstrumentPreferenceObservationRecord>, AppError> {
    let row = sqlx::query(
        "SELECT id, instrument_id, quote_preference, effective_at, created_at
         FROM instrument_preference_observations
         WHERE instrument_id = ? AND effective_at <= ?
         ORDER BY effective_at DESC, created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(instrument_id)
    .bind(cutoff_at)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.instrument_preference_load_failed", error))?;
    row.map(|row| {
        Ok(InstrumentPreferenceObservationRecord {
            id: required_text(&row, "id")?,
            instrument_id: required_text(&row, "instrument_id")?,
            quote_preference: required_text(&row, "quote_preference")?,
            effective_at: required_text(&row, "effective_at")?,
            created_at: required_text(&row, "created_at")?,
        })
    })
    .transpose()
}

pub async fn insert_fx_preference_observation(
    tx: &mut Transaction<'_, Sqlite>,
    row: &FxPreferenceObservationRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO fx_preference_observations (
            id, household_id, currency_a, currency_b, source_kind, effective_at, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.household_id)
    .bind(&row.currency_a)
    .bind(&row.currency_b)
    .bind(&row.source_kind)
    .bind(&row.effective_at)
    .bind(&row.created_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.fx_preference_insert_failed", error))?;
    Ok(())
}

pub async fn latest_fx_preference_at(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    currency_a: &str,
    currency_b: &str,
    cutoff_at: &str,
) -> Result<Option<FxPreferenceObservationRecord>, AppError> {
    let row = sqlx::query(
        "SELECT id, household_id, currency_a, currency_b, source_kind, effective_at, created_at
         FROM fx_preference_observations
         WHERE household_id = ? AND currency_a = ? AND currency_b = ? AND effective_at <= ?
         ORDER BY effective_at DESC, created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(household_id)
    .bind(currency_a)
    .bind(currency_b)
    .bind(cutoff_at)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.fx_preference_load_failed", error))?;
    row.map(|row| {
        Ok(FxPreferenceObservationRecord {
            id: required_text(&row, "id")?,
            household_id: required_text(&row, "household_id")?,
            currency_a: required_text(&row, "currency_a")?,
            currency_b: required_text(&row, "currency_b")?,
            source_kind: required_text(&row, "source_kind")?,
            effective_at: required_text(&row, "effective_at")?,
            created_at: required_text(&row, "created_at")?,
        })
    })
    .transpose()
}

pub async fn upsert_snapshot_state(
    tx: &mut Transaction<'_, Sqlite>,
    row: &SnapshotStateRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO history_snapshot_state (
            household_id, dirty_from, last_completed_on, rebuild_status, rebuild_cursor_on, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(household_id) DO UPDATE SET
            dirty_from = excluded.dirty_from,
            last_completed_on = excluded.last_completed_on,
            rebuild_status = excluded.rebuild_status,
            rebuild_cursor_on = excluded.rebuild_cursor_on,
            updated_at = excluded.updated_at",
    )
    .bind(&row.household_id)
    .bind(&row.dirty_from)
    .bind(&row.last_completed_on)
    .bind(&row.rebuild_status)
    .bind(&row.rebuild_cursor_on)
    .bind(&row.updated_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.snapshot_state_upsert_failed", error))?;
    Ok(())
}

pub async fn get_snapshot_state(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<Option<SnapshotStateRecord>, AppError> {
    let row = sqlx::query(
        "SELECT household_id, dirty_from, last_completed_on, rebuild_status, rebuild_cursor_on, updated_at
         FROM history_snapshot_state WHERE household_id = ?",
    )
    .bind(household_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.snapshot_state_load_failed", error))?;
    row.map(|row| {
        Ok(SnapshotStateRecord {
            household_id: required_text(&row, "household_id")?,
            dirty_from: optional_text(&row, "dirty_from")?,
            last_completed_on: optional_text(&row, "last_completed_on")?,
            rebuild_status: required_text(&row, "rebuild_status")?,
            rebuild_cursor_on: optional_text(&row, "rebuild_cursor_on")?,
            updated_at: required_text(&row, "updated_at")?,
        })
    })
    .transpose()
}

pub async fn mark_snapshots_dirty_from(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    local_date: &str,
    updated_at: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE history_snapshot_state
         SET dirty_from = CASE
                WHEN dirty_from IS NULL OR dirty_from > ? THEN ?
                ELSE dirty_from
             END,
             updated_at = ?
         WHERE household_id = ?",
    )
    .bind(local_date)
    .bind(local_date)
    .bind(updated_at)
    .bind(household_id)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.snapshot_dirty_update_failed", error))?;
    Ok(())
}

pub async fn insert_daily_valuation_snapshot(
    tx: &mut Transaction<'_, Sqlite>,
    row: &DailyValuationSnapshotRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO daily_valuation_snapshots (
            id, household_id, snapshot_on, cutoff_at, revision, supersedes_snapshot_id,
            assets_amount, liabilities_amount, net_worth_amount, currency,
            is_complete, valued_component_count, total_component_count, coverage_bps,
            generation_reason, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.household_id)
    .bind(&row.snapshot_on)
    .bind(&row.cutoff_at)
    .bind(row.revision)
    .bind(&row.supersedes_snapshot_id)
    .bind(&row.assets_amount)
    .bind(&row.liabilities_amount)
    .bind(&row.net_worth_amount)
    .bind(&row.currency)
    .bind(i64::from(row.is_complete))
    .bind(row.valued_component_count)
    .bind(row.total_component_count)
    .bind(row.coverage_bps)
    .bind(&row.generation_reason)
    .bind(&row.created_at)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.snapshot_insert_failed", error))?;
    Ok(())
}

pub async fn insert_daily_valuation_snapshot_item(
    tx: &mut Transaction<'_, Sqlite>,
    row: &DailyValuationSnapshotItemRecord,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO daily_valuation_snapshot_items (
            id, snapshot_id, account_id, holding_id, instrument_id, component_kind,
            native_amount, native_currency, base_amount, instrument_quote_id, fx_quote_id,
            account_state_observation_id, origin_id, activity_id, is_complete, missing_reason, sort_order
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.snapshot_id)
    .bind(&row.account_id)
    .bind(&row.holding_id)
    .bind(&row.instrument_id)
    .bind(&row.component_kind)
    .bind(&row.native_amount)
    .bind(&row.native_currency)
    .bind(&row.base_amount)
    .bind(&row.instrument_quote_id)
    .bind(&row.fx_quote_id)
    .bind(&row.account_state_observation_id)
    .bind(&row.origin_id)
    .bind(&row.activity_id)
    .bind(i64::from(row.is_complete))
    .bind(&row.missing_reason)
    .bind(row.sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("history.snapshot_item_insert_failed", error))?;
    Ok(())
}

pub async fn latest_snapshot_for_date(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    snapshot_on: &str,
) -> Result<Option<DailyValuationSnapshotRecord>, AppError> {
    let row = sqlx::query(
        "SELECT id, household_id, snapshot_on, cutoff_at, revision, supersedes_snapshot_id,
                assets_amount, liabilities_amount, net_worth_amount, currency,
                is_complete, valued_component_count, total_component_count, coverage_bps,
                generation_reason, created_at
         FROM daily_valuation_snapshots
         WHERE household_id = ? AND snapshot_on = ?
         ORDER BY revision DESC, created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(household_id)
    .bind(snapshot_on)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.snapshot_load_failed", error))?;
    row.map(snapshot_from_row).transpose()
}

pub async fn list_snapshot_items(
    tx: &mut Transaction<'_, Sqlite>,
    snapshot_id: &str,
) -> Result<Vec<DailyValuationSnapshotItemRecord>, AppError> {
    sqlx::query(
        "SELECT id, snapshot_id, account_id, holding_id, instrument_id, component_kind,
                native_amount, native_currency, base_amount, instrument_quote_id, fx_quote_id,
                account_state_observation_id, origin_id, activity_id, is_complete, missing_reason, sort_order
         FROM daily_valuation_snapshot_items
         WHERE snapshot_id = ?
         ORDER BY sort_order ASC, account_id ASC, id ASC",
    )
    .bind(snapshot_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.snapshot_items_load_failed", error))?
    .into_iter()
    .map(snapshot_item_from_row)
    .collect()
}

async fn load_legs_for_activity(
    tx: &mut Transaction<'_, Sqlite>,
    activity_id: &str,
) -> Result<Vec<ActivityLeg>, AppError> {
    sqlx::query(
        "SELECT id, activity_id, account_id, role, direction, component_kind,
                amount, currency, holding_id, instrument_id, quantity, fx_rate, sort_order
         FROM activity_legs
         WHERE activity_id = ?
         ORDER BY sort_order ASC, id ASC",
    )
    .bind(activity_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("history.activity_legs_load_failed", error))?
    .into_iter()
    .map(leg_from_row)
    .collect()
}

fn origin_from_row(row: sqlx::sqlite::SqliteRow) -> Result<HistoryOriginRecord, AppError> {
    Ok(HistoryOriginRecord {
        id: required_text(&row, "id")?,
        household_id: required_text(&row, "household_id")?,
        timezone: required_text(&row, "timezone")?,
        timezone_confirmed: flag(required_i64(&row, "timezone_confirmed")?),
        origin_at: required_text(&row, "origin_at")?,
        origin_local_date: required_text(&row, "origin_local_date")?,
        source: required_text(&row, "source")?,
        schema_version: required_i64(&row, "schema_version")?,
        created_at: required_text(&row, "created_at")?,
    })
}

fn origin_account_state_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<OriginAccountStateRecord, AppError> {
    Ok(OriginAccountStateRecord {
        origin_id: required_text(&row, "origin_id")?,
        account_id: required_text(&row, "account_id")?,
        primary_category: required_text(&row, "primary_category")?,
        secondary_category: required_text(&row, "secondary_category")?,
        tracking_mode: required_text(&row, "tracking_mode")?,
        include_in_net_worth: flag(required_i64(&row, "include_in_net_worth")?),
        include_in_investment: flag(required_i64(&row, "include_in_investment")?),
        include_in_liquid_assets: flag(required_i64(&row, "include_in_liquid_assets")?),
        archived_at: optional_text(&row, "archived_at")?,
        institution_id: optional_text(&row, "institution_id")?,
        group_id: optional_text(&row, "group_id")?,
    })
}

fn account_state_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<AccountStateObservationRecord, AppError> {
    Ok(AccountStateObservationRecord {
        id: required_text(&row, "id")?,
        account_id: required_text(&row, "account_id")?,
        primary_category: required_text(&row, "primary_category")?,
        secondary_category: required_text(&row, "secondary_category")?,
        tracking_mode: required_text(&row, "tracking_mode")?,
        include_in_net_worth: flag(required_i64(&row, "include_in_net_worth")?),
        include_in_investment: flag(required_i64(&row, "include_in_investment")?),
        include_in_liquid_assets: flag(required_i64(&row, "include_in_liquid_assets")?),
        archived_at: optional_text(&row, "archived_at")?,
        institution_id: optional_text(&row, "institution_id")?,
        group_id: optional_text(&row, "group_id")?,
        effective_at: required_text(&row, "effective_at")?,
        created_at: required_text(&row, "created_at")?,
    })
}

fn snapshot_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<DailyValuationSnapshotRecord, AppError> {
    Ok(DailyValuationSnapshotRecord {
        id: required_text(&row, "id")?,
        household_id: required_text(&row, "household_id")?,
        snapshot_on: required_text(&row, "snapshot_on")?,
        cutoff_at: required_text(&row, "cutoff_at")?,
        revision: required_i64(&row, "revision")?,
        supersedes_snapshot_id: optional_text(&row, "supersedes_snapshot_id")?,
        assets_amount: required_text(&row, "assets_amount")?,
        liabilities_amount: required_text(&row, "liabilities_amount")?,
        net_worth_amount: required_text(&row, "net_worth_amount")?,
        currency: required_text(&row, "currency")?,
        is_complete: flag(required_i64(&row, "is_complete")?),
        valued_component_count: required_i64(&row, "valued_component_count")?,
        total_component_count: required_i64(&row, "total_component_count")?,
        coverage_bps: required_i64(&row, "coverage_bps")?,
        generation_reason: required_text(&row, "generation_reason")?,
        created_at: required_text(&row, "created_at")?,
    })
}

fn snapshot_item_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<DailyValuationSnapshotItemRecord, AppError> {
    Ok(DailyValuationSnapshotItemRecord {
        id: required_text(&row, "id")?,
        snapshot_id: required_text(&row, "snapshot_id")?,
        account_id: required_text(&row, "account_id")?,
        holding_id: optional_text(&row, "holding_id")?,
        instrument_id: optional_text(&row, "instrument_id")?,
        component_kind: required_text(&row, "component_kind")?,
        native_amount: optional_text(&row, "native_amount")?,
        native_currency: optional_text(&row, "native_currency")?,
        base_amount: optional_text(&row, "base_amount")?,
        instrument_quote_id: optional_text(&row, "instrument_quote_id")?,
        fx_quote_id: optional_text(&row, "fx_quote_id")?,
        account_state_observation_id: optional_text(&row, "account_state_observation_id")?,
        origin_id: optional_text(&row, "origin_id")?,
        activity_id: optional_text(&row, "activity_id")?,
        is_complete: flag(required_i64(&row, "is_complete")?),
        missing_reason: optional_text(&row, "missing_reason")?,
        sort_order: required_i64(&row, "sort_order")?,
    })
}

fn activity_from_row(
    row: sqlx::sqlite::SqliteRow,
    legs: Vec<ActivityLeg>,
) -> Result<Activity, AppError> {
    let correction_group = optional_text(&row, "correction_group")?
        .map(|value| uuid::Uuid::parse_str(&value).map_err(|_| AppError::DatabaseUnavailable))
        .transpose()?;
    let related = optional_text(&row, "related_instrument_id")?;
    Activity::from_persisted(
        ActivityId::parse(&required_text(&row, "id")?)?,
        HouseholdId::parse(&required_text(&row, "household_id")?)?,
        ActivityKind::parse(&required_text(&row, "kind")?)?,
        crate::domain::Timestamp::parse(&required_text(&row, "effective_at")?)?,
        CalendarDate::parse(&required_text(&row, "effective_local_date")?)?,
        crate::domain::Timestamp::parse(&required_text(&row, "created_at")?)?,
        optional_text(&row, "note")?,
        optional_text(&row, "reverses")?
            .map(|id| ActivityId::parse(&id))
            .transpose()?,
        optional_text(&row, "corrects")?
            .map(|id| ActivityId::parse(&id))
            .transpose()?,
        correction_group,
        optional_text(&row, "income_kind")?
            .map(|value| IncomeKind::parse(&value))
            .transpose()?,
        optional_text(&row, "fee_kind")?
            .map(|value| FeeKind::parse(&value))
            .transpose()?,
        related.as_deref().map(InstrumentId::parse).transpose()?,
        legs,
    )
}

fn leg_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ActivityLeg, AppError> {
    let component_kind = ComponentKind::parse(&required_text(&row, "component_kind")?)?;
    let component = match component_kind {
        ComponentKind::AccountValue | ComponentKind::HoldingsCash => {
            let amount = required_text(&row, "amount")?;
            let currency = crate::domain::CurrencyCode::parse(&required_text(&row, "currency")?)?;
            let money = Money::parse(&amount, currency)?;
            match component_kind {
                ComponentKind::AccountValue => LegComponent::AccountValue { amount: money },
                ComponentKind::HoldingsCash => LegComponent::HoldingsCash { amount: money },
                ComponentKind::HoldingQuantity => unreachable!(),
            }
        }
        ComponentKind::HoldingQuantity => LegComponent::HoldingQuantity {
            instrument_id: InstrumentId::parse(&required_text(&row, "instrument_id")?)?,
            holding_id: HoldingId::parse(&required_text(&row, "holding_id")?)?,
            quantity: Quantity::parse(&required_text(&row, "quantity")?)?,
        },
    };
    let fx_rate = optional_text(&row, "fx_rate")?
        .map(|value| FxRate::parse(&value))
        .transpose()?;
    ActivityLeg::from_persisted(
        crate::domain::ActivityLegId::parse(&required_text(&row, "id")?)?,
        ActivityId::parse(&required_text(&row, "activity_id")?)?,
        crate::domain::AccountId::parse(&required_text(&row, "account_id")?)?,
        LegRole::parse(&required_text(&row, "role")?)?,
        Direction::parse(&required_text(&row, "direction")?)?,
        component,
        fx_rate,
        required_i64(&row, "sort_order")?,
    )
}

fn map_activity_insert_error(error: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(database) = &error {
        if database.is_unique_violation() {
            let message = database.message();
            if message.contains("idx_activities_reverses") || message.contains("reverses") {
                return AppError::ActivityAlreadyReversed;
            }
        }
    }
    map_write_error("history.activity_insert_failed", error)
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
