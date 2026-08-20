//! Pending Activity proposals and recurring rule application boundaries.
//!
//! Pending rows are deliberately kept outside the ledger. This module parses
//! typed payloads, persists proposals, and delegates preview/post to the
//! existing ActivityService only at the explicit preview/post boundary.

use std::collections::{HashMap, HashSet};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    activity_service::{self, ActivityTimeSpec},
    history_query_service::{self, ActivityDetailDto, ActivityPreviewDto},
    history_repositories::{self, HistoryOriginRecord},
    reference::{
        begin_read_tx, begin_write_tx, finish_read_tx, finish_write_tx, require_household_tx,
    },
    sustainable_repositories as repositories,
};
use crate::{
    domain::{
        AccountId, AmbiguousOffset, CalendarDate, CurrencyCode, FeeKind, FxRate, HistoryTimezone,
        HoldingId, IncomeKind, InstrumentId, MonetaryComponent, MonetaryEndpoint, Money,
        PendingActivityKind, PendingActivityPayload, Quantity, QuantityEndpoint,
        RecurringActivityPayload, Schedule, ScheduleCadence, ScheduleInterval, Timestamp,
        UnitPrice,
    },
    error::AppError,
    state::AppState,
};

const DEFAULT_PAGE_SIZE: i64 = 50;
const MAX_PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum PendingActivityPayloadInput {
    #[serde(rename_all = "camelCase")]
    Deposit {
        account_id: String,
        component: String,
        amount: String,
        currency: String,
    },
    #[serde(rename_all = "camelCase")]
    Withdrawal {
        account_id: String,
        component: String,
        amount: String,
        currency: String,
    },
    #[serde(rename_all = "camelCase")]
    Transfer {
        source_account_id: String,
        source_component: String,
        source_amount: String,
        source_currency: String,
        destination_account_id: String,
        destination_component: String,
        destination_amount: String,
        destination_currency: String,
        fee_amount: Option<String>,
        fee_kind: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    PositionTransfer {
        source_holding_id: String,
        destination_holding_id: String,
        quantity: String,
    },
    #[serde(rename_all = "camelCase")]
    Buy {
        holding_id: String,
        instrument_id: String,
        quantity: String,
        unit_price: String,
        gross_amount: String,
        settlement_currency: String,
        fee_amount: Option<String>,
        #[serde(default)]
        confirm_zero_unit_price: bool,
    },
    #[serde(rename_all = "camelCase")]
    Sell {
        holding_id: String,
        instrument_id: String,
        quantity: String,
        unit_price: String,
        gross_amount: String,
        settlement_currency: String,
        fee_amount: Option<String>,
        #[serde(default)]
        confirm_zero_unit_price: bool,
    },
    #[serde(rename_all = "camelCase")]
    Income {
        account_id: String,
        component: String,
        amount: String,
        currency: String,
        income_kind: String,
        instrument_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Fee {
        account_id: String,
        component: String,
        amount: String,
        currency: String,
        fee_kind: String,
        instrument_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    DebtDraw {
        liability_account_id: String,
        principal_amount: String,
        principal_currency: String,
        cash_account_id: Option<String>,
        cash_component: Option<String>,
        cash_amount: Option<String>,
        cash_currency: Option<String>,
        fx_rate: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    DebtPayment {
        liability_account_id: String,
        principal_amount: String,
        principal_currency: String,
        cash_account_id: String,
        cash_component: String,
        cash_amount: String,
        cash_currency: String,
        fx_rate: Option<String>,
        fee_amount: Option<String>,
        fee_kind: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreatePendingActivityInput {
    pub scheduled_local_date: String,
    pub payload: PendingActivityPayloadInput,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePendingActivityInput {
    pub id: String,
    pub scheduled_local_date: String,
    pub payload: PendingActivityPayloadInput,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListPendingActivitiesInput {
    pub cursor: Option<String>,
    pub limit: Option<i32>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingActivityTimeInput {
    pub id: String,
    pub local_date: String,
    pub local_time: String,
    pub ambiguous_offset: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecurringActivityRuleInput {
    pub cadence: String,
    pub interval_value: i32,
    pub start_local_date: String,
    pub end_local_date: Option<String>,
    pub payload: PendingActivityPayloadInput,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateRecurringActivityRuleInput {
    #[serde(flatten)]
    pub rule: RecurringActivityRuleInput,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRecurringActivityRuleInput {
    pub id: String,
    pub end_local_date: Option<String>,
    pub payload: PendingActivityPayloadInput,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingActivityDto {
    pub id: String,
    pub recurring_rule_id: Option<String>,
    pub recurring_rule_revision: Option<i32>,
    pub scheduled_local_date: String,
    pub creation_source: String,
    pub payload: PendingActivityPayloadInput,
    pub note: Option<String>,
    pub status: String,
    pub posted_activity_id: Option<String>,
    pub skipped_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingActivityPageDto {
    pub items: Vec<PendingActivityDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingActivityPreviewDto {
    pub pending: PendingActivityDto,
    pub activity: ActivityPreviewDto,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingActivityPostDto {
    pub pending: PendingActivityDto,
    pub activity: ActivityDetailDto,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecurringActivityRuleDto {
    pub id: String,
    pub cadence: String,
    pub interval_value: i32,
    pub start_local_date: String,
    pub end_local_date: Option<String>,
    pub anchor_local_date: String,
    pub payload: PendingActivityPayloadInput,
    pub note: Option<String>,
    pub revision: i32,
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BlockedRecurringRuleDto {
    pub rule_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GenerateDuePendingActivitiesResultDto {
    pub generated_count: i32,
    pub blocked: Vec<BlockedRecurringRuleDto>,
    pub has_more: bool,
}

pub async fn create_pending_activity(
    state: &AppState,
    input: CreatePendingActivityInput,
) -> Result<PendingActivityDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let scheduled = CalendarDate::parse(&input.scheduled_local_date)?;
        let payload = pending_payload_from_input(&mut tx, &household.id, input.payload).await?;
        let command = command_for_payload(&mut tx, &household.id, payload.clone()).await?;
        activity_service::validate_pending_command_in_tx(&mut tx, &household.id, &command).await?;
        let now = Timestamp::now().to_rfc3339();
        let note = crate::domain::parse_optional_note(input.note.as_deref())?;
        let (kind, record_payload) = pending_record_from_payload(&payload)?;
        let row = repositories::PendingActivityRecord {
            id: crate::domain::PendingActivityId::new().to_string(),
            household_id: household.id.clone(),
            recurring_rule_id: None,
            recurring_rule_revision: None,
            scheduled_local_date: scheduled.to_ymd(),
            creation_source: "manual".to_owned(),
            kind,
            payload: record_payload,
            note,
            status: "open".to_owned(),
            posted_activity_id: None,
            skipped_at: None,
            created_at: now.clone(),
            updated_at: now,
        };
        repositories::insert_pending_activity(&mut tx, &row).await?;
        pending_dto(&row)
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn update_pending_activity(
    state: &AppState,
    input: UpdatePendingActivityInput,
) -> Result<PendingActivityDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let mut row = repositories::get_pending_activity(&mut tx, &household.id, &input.id)
            .await?
            .ok_or_else(|| AppError::not_found("pending activity", &input.id))?;
        require_open(&row)?;
        let scheduled = CalendarDate::parse(&input.scheduled_local_date)?;
        let payload = pending_payload_from_input(&mut tx, &household.id, input.payload).await?;
        let command = command_for_payload(&mut tx, &household.id, payload.clone()).await?;
        activity_service::validate_pending_command_in_tx(&mut tx, &household.id, &command).await?;
        let (kind, record_payload) = pending_record_from_payload(&payload)?;
        row.scheduled_local_date = scheduled.to_ymd();
        row.kind = kind;
        row.payload = record_payload;
        row.note = crate::domain::parse_optional_note(input.note.as_deref())?;
        row.updated_at = Timestamp::now().to_rfc3339();
        repositories::update_pending_activity(&mut tx, &row).await?;
        pending_dto(&row)
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn list_pending_activities(
    state: &AppState,
    input: ListPendingActivitiesInput,
) -> Result<PendingActivityPageDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let limit = page_limit(input.limit)?;
        if let Some(status) = input.status.as_deref() {
            validate_status(status)?;
        }
        let decoded = input
            .cursor
            .as_deref()
            .map(decode_pending_cursor)
            .transpose()?;
        let cursor = decoded
            .as_ref()
            .map(|value| repositories::PendingActivityCursor {
                scheduled_local_date: value.0.clone(),
                created_at: value.1.clone(),
                id: value.2.clone(),
            });
        let mut rows = repositories::list_pending_activities(
            &mut tx,
            &household.id,
            input.status.as_deref(),
            cursor.as_ref(),
            limit + 1,
        )
        .await?;
        let has_more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        if has_more {
            rows.truncate(usize::try_from(limit).unwrap_or(rows.len()));
        }
        let next_cursor = if has_more {
            rows.last().map(encode_pending_cursor)
        } else {
            None
        };
        let items = rows
            .iter()
            .map(pending_dto)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PendingActivityPageDto {
            items,
            next_cursor,
            has_more,
        })
    }
    .await;
    finish_read_tx(tx, result).await
}

pub async fn preview_pending_activity(
    state: &AppState,
    input: PendingActivityTimeInput,
) -> Result<PendingActivityPreviewDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let row = repositories::get_pending_activity(&mut tx, &household.id, &input.id)
            .await?
            .ok_or_else(|| AppError::not_found("pending activity", &input.id))?;
        require_open(&row)?;
        let payload =
            pending_payload_from_record(&mut tx, &household.id, &row.kind, &row.payload).await?;
        let command = command_for_payload(&mut tx, &household.id, payload).await?;
        let ambiguous_offset = input
            .ambiguous_offset
            .as_deref()
            .map(AmbiguousOffset::parse)
            .transpose()?;
        let time = ActivityTimeSpec {
            local_date: &input.local_date,
            local_time: &input.local_time,
            ambiguous_offset,
        };
        let activity = history_query_service::preview_pending_command_in_tx(
            &mut tx,
            &household.id,
            &household.base_currency,
            command,
            time,
            row.note.as_deref(),
        )
        .await?;
        Ok(PendingActivityPreviewDto {
            pending: pending_dto(&row)?,
            activity,
        })
    }
    .await;
    finish_read_tx(tx, result).await
}

pub async fn post_pending_activity(
    state: &AppState,
    input: PendingActivityTimeInput,
) -> Result<PendingActivityPostDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let mut row = repositories::get_pending_activity(&mut tx, &household.id, &input.id)
            .await?
            .ok_or_else(|| AppError::not_found("pending activity", &input.id))?;
        require_open(&row)?;
        let (_, today) = confirmed_today(&mut tx, &household.id).await?;
        let scheduled = CalendarDate::parse(&row.scheduled_local_date)?;
        if scheduled > today {
            return Err(AppError::invalid_pending_activity(
                "A pending Activity cannot be posted before its scheduled local date.",
            ));
        }
        let payload =
            pending_payload_from_record(&mut tx, &household.id, &row.kind, &row.payload).await?;
        let command = command_for_payload(&mut tx, &household.id, payload).await?;
        let ambiguous_offset = input
            .ambiguous_offset
            .as_deref()
            .map(AmbiguousOffset::parse)
            .transpose()?;
        let time = ActivityTimeSpec {
            local_date: &input.local_date,
            local_time: &input.local_time,
            ambiguous_offset,
        };
        let activity = history_query_service::post_pending_command_in_tx(
            &mut tx,
            &household.id,
            &household.base_currency,
            command,
            time,
            row.note.as_deref(),
        )
        .await?;
        row.status = "posted".to_owned();
        row.posted_activity_id = Some(activity.id.clone());
        row.skipped_at = None;
        row.updated_at = Timestamp::now().to_rfc3339();
        repositories::update_pending_activity(&mut tx, &row).await?;
        Ok(PendingActivityPostDto {
            pending: pending_dto(&row)?,
            activity,
        })
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn skip_pending_activity(
    state: &AppState,
    id: &str,
) -> Result<PendingActivityDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let mut row = repositories::get_pending_activity(&mut tx, &household.id, id)
            .await?
            .ok_or_else(|| AppError::not_found("pending activity", id))?;
        require_open(&row)?;
        let now = Timestamp::now().to_rfc3339();
        row.status = "skipped".to_owned();
        row.skipped_at = Some(now.clone());
        row.updated_at = now;
        repositories::update_pending_activity(&mut tx, &row).await?;
        pending_dto(&row)
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn create_recurring_activity_rule(
    state: &AppState,
    input: CreateRecurringActivityRuleInput,
) -> Result<RecurringActivityRuleDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let schedule = schedule_from_input(&input.rule)?;
        let payload =
            pending_payload_from_input(&mut tx, &household.id, input.rule.payload).await?;
        let recurring = RecurringActivityPayload::new(payload.clone())?;
        let command = command_for_payload(&mut tx, &household.id, payload).await?;
        activity_service::validate_pending_command_in_tx(&mut tx, &household.id, &command).await?;
        let now = Timestamp::now().to_rfc3339();
        let note = crate::domain::parse_optional_note(input.rule.note.as_deref())?;
        let (kind, payload) = rule_record_from_payload(recurring.as_pending())?;
        let row = repositories::RecurringActivityRuleRecord {
            id: crate::domain::RecurringActivityRuleId::new().to_string(),
            household_id: household.id.clone(),
            cadence: schedule.cadence().as_str().to_owned(),
            interval_value: i64::from(schedule.interval().value()),
            start_local_date: schedule.start().to_ymd(),
            end_local_date: schedule.end().map(|date| date.to_ymd()),
            anchor_local_date: schedule.anchor().to_ymd(),
            kind,
            payload,
            note,
            revision: 1,
            archived_at: None,
            created_at: now.clone(),
            updated_at: now,
        };
        repositories::insert_recurring_activity_rule(&mut tx, &row).await?;
        recurring_rule_dto(&row)
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn update_recurring_activity_rule(
    state: &AppState,
    input: UpdateRecurringActivityRuleInput,
) -> Result<RecurringActivityRuleDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let mut row = repositories::get_recurring_activity_rule(&mut tx, &household.id, &input.id)
            .await?
            .ok_or_else(|| AppError::not_found("recurring rule", &input.id))?;
        let cadence = ScheduleCadence::parse(&row.cadence)?;
        let interval = ScheduleInterval::new(
            cadence,
            u16::try_from(row.interval_value)
                .map_err(|_| AppError::invalid_recurring_rule("Schedule interval is invalid."))?,
        )?;
        let start = CalendarDate::parse(&row.start_local_date)?;
        let anchor = CalendarDate::parse(&row.anchor_local_date)?;
        let end = input
            .end_local_date
            .as_deref()
            .map(CalendarDate::parse)
            .transpose()?;
        let _schedule = Schedule::with_anchor(cadence, interval, start, anchor, end)?;
        let payload = pending_payload_from_input(&mut tx, &household.id, input.payload).await?;
        let recurring = RecurringActivityPayload::new(payload.clone())?;
        let command = command_for_payload(&mut tx, &household.id, payload).await?;
        activity_service::validate_pending_command_in_tx(&mut tx, &household.id, &command).await?;
        let (kind, payload) = rule_record_from_payload(recurring.as_pending())?;
        row.end_local_date = end.map(|date| date.to_ymd());
        row.kind = kind;
        row.payload = payload;
        row.note = crate::domain::parse_optional_note(input.note.as_deref())?;
        row.revision = row
            .revision
            .checked_add(1)
            .ok_or_else(|| AppError::invalid_recurring_rule("Rule revision overflowed."))?;
        row.updated_at = Timestamp::now().to_rfc3339();
        repositories::update_recurring_activity_rule(&mut tx, &row).await?;
        recurring_rule_dto(&row)
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn list_recurring_activity_rules(
    state: &AppState,
    include_archived: bool,
) -> Result<Vec<RecurringActivityRuleDto>, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        repositories::list_recurring_activity_rules(&mut tx, &household.id, include_archived)
            .await?
            .iter()
            .map(recurring_rule_dto)
            .collect::<Result<Vec<_>, _>>()
    }
    .await;
    finish_read_tx(tx, result).await
}

pub async fn archive_recurring_activity_rule(
    state: &AppState,
    id: &str,
) -> Result<RecurringActivityRuleDto, AppError> {
    set_rule_archived(state, id, true).await
}

pub async fn restore_recurring_activity_rule(
    state: &AppState,
    id: &str,
) -> Result<RecurringActivityRuleDto, AppError> {
    set_rule_archived(state, id, false).await
}

async fn set_rule_archived(
    state: &AppState,
    id: &str,
    archived: bool,
) -> Result<RecurringActivityRuleDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let mut row = repositories::get_recurring_activity_rule(&mut tx, &household.id, id)
            .await?
            .ok_or_else(|| AppError::not_found("recurring rule", id))?;
        if row.archived_at.is_some() == archived {
            return recurring_rule_dto(&row);
        }
        row.archived_at = archived.then(|| Timestamp::now().to_rfc3339());
        row.updated_at = Timestamp::now().to_rfc3339();
        repositories::update_recurring_activity_rule(&mut tx, &row).await?;
        recurring_rule_dto(&row)
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn generate_due_pending_activities(
    state: &AppState,
) -> Result<GenerateDuePendingActivitiesResultDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let (_, today) = confirmed_today(&mut tx, &household.id).await?;
        let rules =
            repositories::list_recurring_activity_rules(&mut tx, &household.id, false).await?;
        let existing = repositories::list_recurring_pending_dates(&mut tx, &household.id).await?;
        let mut existing_by_rule: HashMap<String, HashSet<String>> = HashMap::new();
        for (rule_id, date) in existing {
            existing_by_rule.entry(rule_id).or_default().insert(date);
        }
        let mut remaining = crate::domain::MAX_RECURRENCE_OCCURRENCES;
        let mut generated_count = 0usize;
        let mut has_more = false;
        let mut blocked = Vec::new();
        for rule in rules {
            let result = generate_for_rule(
                &mut tx,
                &household.id,
                &rule,
                today,
                &mut existing_by_rule,
                &mut remaining,
            )
            .await;
            match result {
                Ok(rule_result) => {
                    generated_count = generated_count.saturating_add(rule_result.generated);
                    has_more |= rule_result.has_more;
                }
                Err(error) => {
                    blocked.push(BlockedRecurringRuleDto {
                        rule_id: rule.id,
                        reason: stable_rule_block_reason(error),
                    });
                }
            }
            if remaining == 0 {
                has_more = true;
                break;
            }
        }
        Ok(GenerateDuePendingActivitiesResultDto {
            generated_count: i32::try_from(generated_count).unwrap_or(i32::MAX),
            blocked,
            has_more,
        })
    }
    .await;
    finish_write_tx(tx, result).await
}

struct RuleGenerationResult {
    generated: usize,
    has_more: bool,
}

async fn generate_for_rule(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    rule: &repositories::RecurringActivityRuleRecord,
    today: CalendarDate,
    existing_by_rule: &mut HashMap<String, HashSet<String>>,
    remaining: &mut usize,
) -> Result<RuleGenerationResult, AppError> {
    let cadence = ScheduleCadence::parse(&rule.cadence)?;
    let interval = ScheduleInterval::new(
        cadence,
        u16::try_from(rule.interval_value)
            .map_err(|_| AppError::invalid_recurring_rule("Schedule interval is invalid."))?,
    )?;
    let schedule = Schedule::with_anchor(
        cadence,
        interval,
        CalendarDate::parse(&rule.start_local_date)?,
        CalendarDate::parse(&rule.anchor_local_date)?,
        rule.end_local_date
            .as_deref()
            .map(CalendarDate::parse)
            .transpose()?,
    )?;
    let payload = pending_payload_from_rule_record(rule).await?;
    let command = command_for_payload(tx, household_id, payload.clone()).await?;
    activity_service::validate_pending_command_in_tx(tx, household_id, &command).await?;
    let existing = existing_by_rule.entry(rule.id.clone()).or_default();
    let after = existing
        .iter()
        .filter_map(|value| CalendarDate::parse(value).ok())
        .filter(|date| *date <= today)
        .max();
    let recurrence = match after {
        Some(after) => schedule.occurrences_after(after, today, (*remaining).max(1))?,
        None => schedule.occurrences_through(today, (*remaining).max(1))?,
    };
    let mut generated = 0usize;
    for date in recurrence.dates {
        let key = date.to_ymd();
        if existing.contains(&key) {
            continue;
        }
        if *remaining == 0 {
            return Ok(RuleGenerationResult {
                generated,
                has_more: true,
            });
        }
        let (kind, record_payload) = pending_record_from_payload(&payload)?;
        let now = Timestamp::now().to_rfc3339();
        let row = repositories::PendingActivityRecord {
            id: crate::domain::PendingActivityId::new().to_string(),
            household_id: household_id.to_owned(),
            recurring_rule_id: Some(rule.id.clone()),
            recurring_rule_revision: Some(rule.revision),
            scheduled_local_date: key.clone(),
            creation_source: "recurring".to_owned(),
            kind,
            payload: record_payload,
            note: rule.note.clone(),
            status: "open".to_owned(),
            posted_activity_id: None,
            skipped_at: None,
            created_at: now.clone(),
            updated_at: now,
        };
        repositories::insert_pending_activity(tx, &row).await?;
        existing.insert(key);
        *remaining -= 1;
        generated += 1;
    }
    Ok(RuleGenerationResult {
        generated,
        has_more: recurrence.has_more,
    })
}

async fn command_for_payload(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    payload: PendingActivityPayload,
) -> Result<activity_service::PostCommand, AppError> {
    history_query_service::post_command_from_pending_payload(tx, household_id, payload).await
}

async fn pending_payload_from_input(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    input: PendingActivityPayloadInput,
) -> Result<PendingActivityPayload, AppError> {
    let payload = match input {
        PendingActivityPayloadInput::Deposit {
            account_id,
            component,
            amount,
            currency,
        } => PendingActivityPayload::Deposit {
            endpoint: monetary_endpoint(&account_id, &component)?,
            amount: parse_money(&amount, &currency)?,
        },
        PendingActivityPayloadInput::Withdrawal {
            account_id,
            component,
            amount,
            currency,
        } => PendingActivityPayload::Withdrawal {
            endpoint: monetary_endpoint(&account_id, &component)?,
            amount: parse_money(&amount, &currency)?,
        },
        PendingActivityPayloadInput::Transfer {
            source_account_id,
            source_component,
            source_amount,
            source_currency,
            destination_account_id,
            destination_component,
            destination_amount,
            destination_currency,
            fee_amount,
            fee_kind,
        } => {
            let source_money = parse_money(&source_amount, &source_currency)?;
            let fee = optional_fee(fee_amount, fee_kind, source_money.currency(), "feeAmount")?;
            let (fee, fee_kind) =
                fee.map_or((None, None), |(amount, kind)| (Some(amount), Some(kind)));
            PendingActivityPayload::Transfer {
                source: monetary_endpoint(&source_account_id, &source_component)?,
                source_amount: source_money,
                destination: monetary_endpoint(&destination_account_id, &destination_component)?,
                destination_amount: parse_money(&destination_amount, &destination_currency)?,
                fee,
                fee_kind,
            }
        }
        PendingActivityPayloadInput::PositionTransfer {
            source_holding_id,
            destination_holding_id,
            quantity,
        } => PendingActivityPayload::PositionTransfer {
            source: quantity_endpoint(tx, household_id, &source_holding_id).await?,
            destination: quantity_endpoint(tx, household_id, &destination_holding_id).await?,
            quantity: Quantity::parse(&quantity)?,
        },
        PendingActivityPayloadInput::Buy {
            holding_id,
            instrument_id,
            quantity,
            unit_price,
            gross_amount,
            settlement_currency,
            fee_amount,
            confirm_zero_unit_price,
        } => PendingActivityPayload::Buy {
            holding_id: HoldingId::parse(&holding_id)?,
            instrument_id: InstrumentId::parse(&instrument_id)?,
            quantity: Quantity::parse(&quantity)?,
            unit_price: UnitPrice::parse(&unit_price)?,
            gross_amount: parse_money(&gross_amount, &settlement_currency)?,
            fee: fee_amount
                .as_deref()
                .map(|amount| parse_money(amount, &settlement_currency))
                .transpose()?,
            confirm_zero_unit_price,
        },
        PendingActivityPayloadInput::Sell {
            holding_id,
            instrument_id,
            quantity,
            unit_price,
            gross_amount,
            settlement_currency,
            fee_amount,
            confirm_zero_unit_price,
        } => PendingActivityPayload::Sell {
            holding_id: HoldingId::parse(&holding_id)?,
            instrument_id: InstrumentId::parse(&instrument_id)?,
            quantity: Quantity::parse(&quantity)?,
            unit_price: UnitPrice::parse(&unit_price)?,
            gross_amount: parse_money(&gross_amount, &settlement_currency)?,
            fee: fee_amount
                .as_deref()
                .map(|amount| parse_money(amount, &settlement_currency))
                .transpose()?,
            confirm_zero_unit_price,
        },
        PendingActivityPayloadInput::Income {
            account_id,
            component,
            amount,
            currency,
            income_kind,
            instrument_id,
        } => PendingActivityPayload::Income {
            endpoint: monetary_endpoint(&account_id, &component)?,
            amount: parse_money(&amount, &currency)?,
            income_kind: IncomeKind::parse(&income_kind)?,
            instrument_id: instrument_id
                .as_deref()
                .map(InstrumentId::parse)
                .transpose()?,
        },
        PendingActivityPayloadInput::Fee {
            account_id,
            component,
            amount,
            currency,
            fee_kind,
            instrument_id,
        } => PendingActivityPayload::Fee {
            endpoint: monetary_endpoint(&account_id, &component)?,
            amount: parse_money(&amount, &currency)?,
            fee_kind: FeeKind::parse(&fee_kind)?,
            instrument_id: instrument_id
                .as_deref()
                .map(InstrumentId::parse)
                .transpose()?,
        },
        PendingActivityPayloadInput::DebtDraw {
            liability_account_id,
            principal_amount,
            principal_currency,
            cash_account_id,
            cash_component,
            cash_amount,
            cash_currency,
            fx_rate,
        } => PendingActivityPayload::DebtDraw {
            liability_account_id: AccountId::parse(&liability_account_id)?,
            principal: parse_money(&principal_amount, &principal_currency)?,
            cash: optional_cash(
                tx,
                cash_account_id,
                cash_component,
                cash_amount,
                cash_currency,
            )
            .await?,
            fx_rate: fx_rate.as_deref().map(FxRate::parse).transpose()?,
        },
        PendingActivityPayloadInput::DebtPayment {
            liability_account_id,
            principal_amount,
            principal_currency,
            cash_account_id,
            cash_component,
            cash_amount,
            cash_currency,
            fx_rate,
            fee_amount,
            fee_kind,
        } => {
            let cash_currency_code = CurrencyCode::parse(&cash_currency)?;
            let fee = optional_fee(fee_amount, fee_kind, cash_currency_code, "feeAmount")?;
            PendingActivityPayload::DebtPayment {
                liability_account_id: AccountId::parse(&liability_account_id)?,
                principal: parse_money(&principal_amount, &principal_currency)?,
                cash: (
                    monetary_endpoint(&cash_account_id, &cash_component)?,
                    parse_money(&cash_amount, &cash_currency)?,
                ),
                fee,
                fx_rate: fx_rate.as_deref().map(FxRate::parse).transpose()?,
            }
        }
    };
    payload.validate()?;
    Ok(payload)
}

async fn optional_cash(
    _tx: &mut Transaction<'_, Sqlite>,
    account_id: Option<String>,
    component: Option<String>,
    amount: Option<String>,
    currency: Option<String>,
) -> Result<Option<(MonetaryEndpoint, Money)>, AppError> {
    match (account_id, component, amount, currency) {
        (None, None, None, None) => Ok(None),
        (Some(account_id), Some(component), Some(amount), Some(currency)) => Ok(Some((
            monetary_endpoint(&account_id, &component)?,
            parse_money(&amount, &currency)?,
        ))),
        _ => Err(AppError::invalid_pending_activity(
            "Debt cash endpoint fields must be supplied together.",
        )),
    }
}

async fn quantity_endpoint(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    holding_id: &str,
) -> Result<QuantityEndpoint, AppError> {
    let (_, account_id, instrument_id) =
        history_repositories::load_holding_endpoint(tx, household_id, holding_id).await?;
    Ok(QuantityEndpoint {
        account_id: AccountId::parse(&account_id)?,
        holding_id: HoldingId::parse(holding_id)?,
        instrument_id: InstrumentId::parse(&instrument_id)?,
    })
}

fn optional_fee(
    amount: Option<String>,
    kind: Option<String>,
    currency: CurrencyCode,
    field: &str,
) -> Result<Option<(Money, FeeKind)>, AppError> {
    match (amount, kind) {
        (None, None) => Ok(None),
        (Some(amount), Some(kind)) => Ok(Some((
            parse_money(&amount, currency.as_str())?,
            FeeKind::parse(&kind)?,
        ))),
        _ => Err(AppError::validation(
            field,
            "Fee amount and fee kind must be supplied together.",
        )),
    }
}

fn parse_money(amount: &str, currency: &str) -> Result<Money, AppError> {
    Money::parse(amount, CurrencyCode::parse(currency)?)
}

fn monetary_endpoint(account_id: &str, component: &str) -> Result<MonetaryEndpoint, AppError> {
    Ok(MonetaryEndpoint {
        account_id: AccountId::parse(account_id)?,
        component: match component {
            "account_value" => MonetaryComponent::AccountValue,
            "holdings_cash" => MonetaryComponent::HoldingsCash,
            _ => {
                return Err(AppError::validation(
                    "component",
                    "Monetary component is not supported.",
                ))
            }
        },
    })
}

fn pending_record_from_payload(
    payload: &PendingActivityPayload,
) -> Result<(String, repositories::PendingPayloadRecord), AppError> {
    let mut record = repositories::PendingPayloadRecord::default();
    let kind = match payload {
        PendingActivityPayload::Deposit { endpoint, amount } => {
            set_endpoint(
                &mut record.endpoint_account_id,
                &mut record.endpoint_component,
                &mut record.amount,
                &mut record.currency,
                *endpoint,
                *amount,
            );
            PendingActivityKind::Deposit
        }
        PendingActivityPayload::Withdrawal { endpoint, amount } => {
            set_endpoint(
                &mut record.endpoint_account_id,
                &mut record.endpoint_component,
                &mut record.amount,
                &mut record.currency,
                *endpoint,
                *amount,
            );
            PendingActivityKind::Withdrawal
        }
        PendingActivityPayload::Transfer {
            source,
            source_amount,
            destination,
            destination_amount,
            fee,
            fee_kind,
        } => {
            set_endpoint(
                &mut record.source_account_id,
                &mut record.source_component,
                &mut record.source_amount,
                &mut record.source_currency,
                *source,
                *source_amount,
            );
            set_endpoint(
                &mut record.destination_account_id,
                &mut record.destination_component,
                &mut record.destination_amount,
                &mut record.destination_currency,
                *destination,
                *destination_amount,
            );
            set_fee(&mut record, *fee, *fee_kind);
            PendingActivityKind::Transfer
        }
        PendingActivityPayload::PositionTransfer {
            source,
            destination,
            quantity,
        } => {
            record.source_account_id = Some(source.account_id.to_string());
            record.source_holding_id = Some(source.holding_id.to_string());
            record.source_instrument_id = Some(source.instrument_id.to_string());
            record.destination_account_id = Some(destination.account_id.to_string());
            record.destination_holding_id = Some(destination.holding_id.to_string());
            record.destination_instrument_id = Some(destination.instrument_id.to_string());
            record.quantity = Some(quantity.canonical());
            PendingActivityKind::PositionTransfer
        }
        PendingActivityPayload::Buy {
            holding_id,
            instrument_id,
            quantity,
            unit_price,
            gross_amount,
            fee,
            confirm_zero_unit_price,
        } => {
            record.holding_id = Some(holding_id.to_string());
            record.instrument_id = Some(instrument_id.to_string());
            record.quantity = Some(quantity.canonical());
            record.unit_price = Some(unit_price.canonical());
            record.gross_amount = Some(gross_amount.canonical_amount());
            record.gross_currency = Some(gross_amount.currency().as_str().to_owned());
            record.confirm_zero_unit_price = *confirm_zero_unit_price;
            if let Some(fee) = fee {
                record.fee_amount = Some(fee.canonical_amount());
                record.fee_currency = Some(fee.currency().as_str().to_owned());
            }
            PendingActivityKind::Buy
        }
        PendingActivityPayload::Sell {
            holding_id,
            instrument_id,
            quantity,
            unit_price,
            gross_amount,
            fee,
            confirm_zero_unit_price,
        } => {
            record.holding_id = Some(holding_id.to_string());
            record.instrument_id = Some(instrument_id.to_string());
            record.quantity = Some(quantity.canonical());
            record.unit_price = Some(unit_price.canonical());
            record.gross_amount = Some(gross_amount.canonical_amount());
            record.gross_currency = Some(gross_amount.currency().as_str().to_owned());
            record.confirm_zero_unit_price = *confirm_zero_unit_price;
            if let Some(fee) = fee {
                record.fee_amount = Some(fee.canonical_amount());
                record.fee_currency = Some(fee.currency().as_str().to_owned());
            }
            PendingActivityKind::Sell
        }
        PendingActivityPayload::Income {
            endpoint,
            amount,
            income_kind,
            instrument_id,
        } => {
            set_endpoint(
                &mut record.endpoint_account_id,
                &mut record.endpoint_component,
                &mut record.amount,
                &mut record.currency,
                *endpoint,
                *amount,
            );
            record.income_kind = Some(income_kind.as_str().to_owned());
            record.related_instrument_id = instrument_id.map(|id| id.to_string());
            PendingActivityKind::Income
        }
        PendingActivityPayload::Fee {
            endpoint,
            amount,
            fee_kind,
            instrument_id,
        } => {
            set_endpoint(
                &mut record.endpoint_account_id,
                &mut record.endpoint_component,
                &mut record.amount,
                &mut record.currency,
                *endpoint,
                *amount,
            );
            record.fee_kind = Some(fee_kind.as_str().to_owned());
            record.related_instrument_id = instrument_id.map(|id| id.to_string());
            PendingActivityKind::Fee
        }
        PendingActivityPayload::DebtDraw {
            liability_account_id,
            principal,
            cash,
            fx_rate,
        } => {
            record.liability_account_id = Some(liability_account_id.to_string());
            record.principal_amount = Some(principal.canonical_amount());
            record.principal_currency = Some(principal.currency().as_str().to_owned());
            if let Some((endpoint, amount)) = cash {
                set_endpoint(
                    &mut record.cash_account_id,
                    &mut record.cash_component,
                    &mut record.cash_amount,
                    &mut record.cash_currency,
                    *endpoint,
                    *amount,
                );
            }
            record.fx_rate = fx_rate.map(|value| value.canonical());
            PendingActivityKind::DebtDraw
        }
        PendingActivityPayload::DebtPayment {
            liability_account_id,
            principal,
            cash,
            fee,
            fx_rate,
        } => {
            record.liability_account_id = Some(liability_account_id.to_string());
            record.principal_amount = Some(principal.canonical_amount());
            record.principal_currency = Some(principal.currency().as_str().to_owned());
            set_endpoint(
                &mut record.cash_account_id,
                &mut record.cash_component,
                &mut record.cash_amount,
                &mut record.cash_currency,
                cash.0,
                cash.1,
            );
            set_fee(
                &mut record,
                fee.map(|value| value.0),
                fee.map(|value| value.1),
            );
            record.fx_rate = fx_rate.map(|value| value.canonical());
            PendingActivityKind::DebtPayment
        }
    };
    Ok((kind.as_str().to_owned(), record))
}

fn rule_record_from_payload(
    payload: &PendingActivityPayload,
) -> Result<(String, repositories::RulePayloadRecord), AppError> {
    let mut record = repositories::RulePayloadRecord::default();
    let kind = match payload {
        PendingActivityPayload::Deposit { endpoint, amount } => {
            set_endpoint(
                &mut record.endpoint_account_id,
                &mut record.endpoint_component,
                &mut record.amount,
                &mut record.currency,
                *endpoint,
                *amount,
            );
            PendingActivityKind::Deposit
        }
        PendingActivityPayload::Withdrawal { endpoint, amount } => {
            set_endpoint(
                &mut record.endpoint_account_id,
                &mut record.endpoint_component,
                &mut record.amount,
                &mut record.currency,
                *endpoint,
                *amount,
            );
            PendingActivityKind::Withdrawal
        }
        PendingActivityPayload::Transfer {
            source,
            source_amount,
            destination,
            destination_amount,
            fee,
            fee_kind,
        } => {
            set_endpoint(
                &mut record.source_account_id,
                &mut record.source_component,
                &mut record.source_amount,
                &mut record.source_currency,
                *source,
                *source_amount,
            );
            set_endpoint(
                &mut record.destination_account_id,
                &mut record.destination_component,
                &mut record.destination_amount,
                &mut record.destination_currency,
                *destination,
                *destination_amount,
            );
            set_rule_fee(&mut record, *fee, *fee_kind);
            PendingActivityKind::Transfer
        }
        PendingActivityPayload::Income {
            endpoint,
            amount,
            income_kind,
            instrument_id,
        } => {
            set_endpoint(
                &mut record.endpoint_account_id,
                &mut record.endpoint_component,
                &mut record.amount,
                &mut record.currency,
                *endpoint,
                *amount,
            );
            record.income_kind = Some(income_kind.as_str().to_owned());
            record.related_instrument_id = instrument_id.map(|id| id.to_string());
            PendingActivityKind::Income
        }
        PendingActivityPayload::Fee {
            endpoint,
            amount,
            fee_kind,
            instrument_id,
        } => {
            set_endpoint(
                &mut record.endpoint_account_id,
                &mut record.endpoint_component,
                &mut record.amount,
                &mut record.currency,
                *endpoint,
                *amount,
            );
            record.fee_kind = Some(fee_kind.as_str().to_owned());
            record.related_instrument_id = instrument_id.map(|id| id.to_string());
            PendingActivityKind::Fee
        }
        PendingActivityPayload::DebtDraw {
            liability_account_id,
            principal,
            cash,
            fx_rate,
        } => {
            record.liability_account_id = Some(liability_account_id.to_string());
            record.principal_amount = Some(principal.canonical_amount());
            record.principal_currency = Some(principal.currency().as_str().to_owned());
            if let Some((endpoint, amount)) = cash {
                set_endpoint(
                    &mut record.cash_account_id,
                    &mut record.cash_component,
                    &mut record.cash_amount,
                    &mut record.cash_currency,
                    *endpoint,
                    *amount,
                );
            }
            record.fx_rate = fx_rate.map(|value| value.canonical());
            PendingActivityKind::DebtDraw
        }
        PendingActivityPayload::DebtPayment {
            liability_account_id,
            principal,
            cash,
            fee,
            fx_rate,
        } => {
            record.liability_account_id = Some(liability_account_id.to_string());
            record.principal_amount = Some(principal.canonical_amount());
            record.principal_currency = Some(principal.currency().as_str().to_owned());
            set_endpoint(
                &mut record.cash_account_id,
                &mut record.cash_component,
                &mut record.cash_amount,
                &mut record.cash_currency,
                cash.0,
                cash.1,
            );
            set_rule_fee(
                &mut record,
                fee.map(|value| value.0),
                fee.map(|value| value.1),
            );
            record.fx_rate = fx_rate.map(|value| value.canonical());
            PendingActivityKind::DebtPayment
        }
        PendingActivityPayload::PositionTransfer { .. }
        | PendingActivityPayload::Buy { .. }
        | PendingActivityPayload::Sell { .. } => {
            return Err(AppError::invalid_recurring_rule(
                "This Activity payload is not supported by recurring rules.",
            ))
        }
    };
    Ok((kind.as_str().to_owned(), record))
}

fn set_endpoint(
    account_id: &mut Option<String>,
    component: &mut Option<String>,
    amount: &mut Option<String>,
    currency: &mut Option<String>,
    endpoint: MonetaryEndpoint,
    value: Money,
) {
    *account_id = Some(endpoint.account_id.to_string());
    *component = Some(endpoint.component.kind().as_str().to_owned());
    *amount = Some(value.canonical_amount());
    *currency = Some(value.currency().as_str().to_owned());
}

fn set_fee(
    record: &mut repositories::PendingPayloadRecord,
    amount: Option<Money>,
    kind: Option<FeeKind>,
) {
    if let Some(amount) = amount {
        record.fee_amount = Some(amount.canonical_amount());
        record.fee_currency = Some(amount.currency().as_str().to_owned());
    }
    record.fee_kind = kind.map(|value| value.as_str().to_owned());
}

fn set_rule_fee(
    record: &mut repositories::RulePayloadRecord,
    amount: Option<Money>,
    kind: Option<FeeKind>,
) {
    if let Some(amount) = amount {
        record.fee_amount = Some(amount.canonical_amount());
        record.fee_currency = Some(amount.currency().as_str().to_owned());
    }
    record.fee_kind = kind.map(|value| value.as_str().to_owned());
}

async fn pending_payload_from_record(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    kind: &str,
    record: &repositories::PendingPayloadRecord,
) -> Result<PendingActivityPayload, AppError> {
    let input = pending_input_from_record(kind, record)?;
    pending_payload_from_input(tx, household_id, input).await
}

async fn pending_payload_from_rule_record(
    rule: &repositories::RecurringActivityRuleRecord,
) -> Result<PendingActivityPayload, AppError> {
    let input = pending_input_from_rule_record(&rule.kind, &rule.payload)?;
    // Recurring records contain only stable typed endpoint IDs. Reference
    // validity is checked by ActivityService after this conversion.
    pending_payload_from_input_without_tx(input)
}

fn pending_payload_from_input_without_tx(
    input: PendingActivityPayloadInput,
) -> Result<PendingActivityPayload, AppError> {
    // This path is used only for recurring rows whose payloads do not contain
    // holdings/trades and therefore do not need a current holding lookup.
    let payload = match input {
        PendingActivityPayloadInput::Deposit {
            account_id,
            component,
            amount,
            currency,
        } => PendingActivityPayload::Deposit {
            endpoint: monetary_endpoint(&account_id, &component)?,
            amount: parse_money(&amount, &currency)?,
        },
        PendingActivityPayloadInput::Withdrawal {
            account_id,
            component,
            amount,
            currency,
        } => PendingActivityPayload::Withdrawal {
            endpoint: monetary_endpoint(&account_id, &component)?,
            amount: parse_money(&amount, &currency)?,
        },
        PendingActivityPayloadInput::Transfer {
            source_account_id,
            source_component,
            source_amount,
            source_currency,
            destination_account_id,
            destination_component,
            destination_amount,
            destination_currency,
            fee_amount,
            fee_kind,
        } => {
            let source_currency = CurrencyCode::parse(&source_currency)?;
            let fee = optional_fee(fee_amount, fee_kind, source_currency, "feeAmount")?;
            let (fee, fee_kind) =
                fee.map_or((None, None), |(amount, kind)| (Some(amount), Some(kind)));
            PendingActivityPayload::Transfer {
                source: monetary_endpoint(&source_account_id, &source_component)?,
                source_amount: parse_money(&source_amount, source_currency.as_str())?,
                destination: monetary_endpoint(&destination_account_id, &destination_component)?,
                destination_amount: parse_money(&destination_amount, &destination_currency)?,
                fee,
                fee_kind,
            }
        }
        PendingActivityPayloadInput::Income {
            account_id,
            component,
            amount,
            currency,
            income_kind,
            instrument_id,
        } => PendingActivityPayload::Income {
            endpoint: monetary_endpoint(&account_id, &component)?,
            amount: parse_money(&amount, &currency)?,
            income_kind: IncomeKind::parse(&income_kind)?,
            instrument_id: instrument_id
                .as_deref()
                .map(InstrumentId::parse)
                .transpose()?,
        },
        PendingActivityPayloadInput::Fee {
            account_id,
            component,
            amount,
            currency,
            fee_kind,
            instrument_id,
        } => PendingActivityPayload::Fee {
            endpoint: monetary_endpoint(&account_id, &component)?,
            amount: parse_money(&amount, &currency)?,
            fee_kind: FeeKind::parse(&fee_kind)?,
            instrument_id: instrument_id
                .as_deref()
                .map(InstrumentId::parse)
                .transpose()?,
        },
        PendingActivityPayloadInput::DebtDraw {
            liability_account_id,
            principal_amount,
            principal_currency,
            cash_account_id,
            cash_component,
            cash_amount,
            cash_currency,
            fx_rate,
        } => PendingActivityPayload::DebtDraw {
            liability_account_id: AccountId::parse(&liability_account_id)?,
            principal: parse_money(&principal_amount, &principal_currency)?,
            cash: match (cash_account_id, cash_component, cash_amount, cash_currency) {
                (None, None, None, None) => None,
                (Some(account_id), Some(component), Some(amount), Some(currency)) => Some((
                    monetary_endpoint(&account_id, &component)?,
                    parse_money(&amount, &currency)?,
                )),
                _ => {
                    return Err(AppError::invalid_recurring_rule(
                        "Debt cash endpoint fields must be supplied together.",
                    ))
                }
            },
            fx_rate: fx_rate.as_deref().map(FxRate::parse).transpose()?,
        },
        PendingActivityPayloadInput::DebtPayment {
            liability_account_id,
            principal_amount,
            principal_currency,
            cash_account_id,
            cash_component,
            cash_amount,
            cash_currency,
            fx_rate,
            fee_amount,
            fee_kind,
        } => {
            let currency = CurrencyCode::parse(&cash_currency)?;
            let fee = optional_fee(fee_amount, fee_kind, currency, "feeAmount")?;
            PendingActivityPayload::DebtPayment {
                liability_account_id: AccountId::parse(&liability_account_id)?,
                principal: parse_money(&principal_amount, &principal_currency)?,
                cash: (
                    monetary_endpoint(&cash_account_id, &cash_component)?,
                    parse_money(&cash_amount, currency.as_str())?,
                ),
                fee,
                fx_rate: fx_rate.as_deref().map(FxRate::parse).transpose()?,
            }
        }
        PendingActivityPayloadInput::PositionTransfer { .. }
        | PendingActivityPayloadInput::Buy { .. }
        | PendingActivityPayloadInput::Sell { .. } => {
            return Err(AppError::invalid_recurring_rule(
                "This Activity payload is not supported by recurring rules.",
            ))
        }
    };
    payload.validate()?;
    Ok(payload)
}

fn pending_input_from_record(
    kind: &str,
    record: &repositories::PendingPayloadRecord,
) -> Result<PendingActivityPayloadInput, AppError> {
    let kind = PendingActivityKind::parse(kind)?;
    let required = |value: &Option<String>, field: &str| {
        value
            .clone()
            .ok_or_else(|| AppError::invalid_pending_activity(field))
    };
    match kind {
        PendingActivityKind::Deposit | PendingActivityKind::Withdrawal => {
            let input = if kind == PendingActivityKind::Deposit {
                PendingActivityPayloadInput::Deposit {
                    account_id: required(&record.endpoint_account_id, "endpoint account")?,
                    component: required(&record.endpoint_component, "endpoint component")?,
                    amount: required(&record.amount, "amount")?,
                    currency: required(&record.currency, "currency")?,
                }
            } else {
                PendingActivityPayloadInput::Withdrawal {
                    account_id: required(&record.endpoint_account_id, "endpoint account")?,
                    component: required(&record.endpoint_component, "endpoint component")?,
                    amount: required(&record.amount, "amount")?,
                    currency: required(&record.currency, "currency")?,
                }
            };
            Ok(input)
        }
        PendingActivityKind::Transfer => Ok(PendingActivityPayloadInput::Transfer {
            source_account_id: required(&record.source_account_id, "source account")?,
            source_component: required(&record.source_component, "source component")?,
            source_amount: required(&record.source_amount, "source amount")?,
            source_currency: required(&record.source_currency, "source currency")?,
            destination_account_id: required(
                &record.destination_account_id,
                "destination account",
            )?,
            destination_component: required(
                &record.destination_component,
                "destination component",
            )?,
            destination_amount: required(&record.destination_amount, "destination amount")?,
            destination_currency: required(&record.destination_currency, "destination currency")?,
            fee_amount: record.fee_amount.clone(),
            fee_kind: record.fee_kind.clone(),
        }),
        PendingActivityKind::PositionTransfer => {
            Ok(PendingActivityPayloadInput::PositionTransfer {
                source_holding_id: required(&record.source_holding_id, "source holding")?,
                destination_holding_id: required(
                    &record.destination_holding_id,
                    "destination holding",
                )?,
                quantity: required(&record.quantity, "quantity")?,
            })
        }
        PendingActivityKind::Buy | PendingActivityKind::Sell => {
            let holding_id = required(&record.holding_id, "holding")?;
            let instrument_id = required(&record.instrument_id, "instrument")?;
            let quantity = required(&record.quantity, "quantity")?;
            let unit_price = required(&record.unit_price, "unit price")?;
            let gross_amount = required(&record.gross_amount, "gross amount")?;
            let settlement_currency = required(&record.gross_currency, "gross currency")?;
            let input = if kind == PendingActivityKind::Buy {
                PendingActivityPayloadInput::Buy {
                    holding_id,
                    instrument_id,
                    quantity,
                    unit_price,
                    gross_amount,
                    settlement_currency,
                    fee_amount: record.fee_amount.clone(),
                    confirm_zero_unit_price: record.confirm_zero_unit_price,
                }
            } else {
                PendingActivityPayloadInput::Sell {
                    holding_id,
                    instrument_id,
                    quantity,
                    unit_price,
                    gross_amount,
                    settlement_currency,
                    fee_amount: record.fee_amount.clone(),
                    confirm_zero_unit_price: record.confirm_zero_unit_price,
                }
            };
            Ok(input)
        }
        PendingActivityKind::Income | PendingActivityKind::Fee => {
            let account_id = required(&record.endpoint_account_id, "endpoint account")?;
            let component = required(&record.endpoint_component, "endpoint component")?;
            let amount = required(&record.amount, "amount")?;
            let currency = required(&record.currency, "currency")?;
            if kind == PendingActivityKind::Income {
                Ok(PendingActivityPayloadInput::Income {
                    account_id,
                    component,
                    amount,
                    currency,
                    income_kind: required(&record.income_kind, "income kind")?,
                    instrument_id: record.related_instrument_id.clone(),
                })
            } else {
                Ok(PendingActivityPayloadInput::Fee {
                    account_id,
                    component,
                    amount,
                    currency,
                    fee_kind: required(&record.fee_kind, "fee kind")?,
                    instrument_id: record.related_instrument_id.clone(),
                })
            }
        }
        PendingActivityKind::DebtDraw => Ok(PendingActivityPayloadInput::DebtDraw {
            liability_account_id: required(&record.liability_account_id, "liability account")?,
            principal_amount: required(&record.principal_amount, "principal amount")?,
            principal_currency: required(&record.principal_currency, "principal currency")?,
            cash_account_id: record.cash_account_id.clone(),
            cash_component: record.cash_component.clone(),
            cash_amount: record.cash_amount.clone(),
            cash_currency: record.cash_currency.clone(),
            fx_rate: record.fx_rate.clone(),
        }),
        PendingActivityKind::DebtPayment => Ok(PendingActivityPayloadInput::DebtPayment {
            liability_account_id: required(&record.liability_account_id, "liability account")?,
            principal_amount: required(&record.principal_amount, "principal amount")?,
            principal_currency: required(&record.principal_currency, "principal currency")?,
            cash_account_id: required(&record.cash_account_id, "cash account")?,
            cash_component: required(&record.cash_component, "cash component")?,
            cash_amount: required(&record.cash_amount, "cash amount")?,
            cash_currency: required(&record.cash_currency, "cash currency")?,
            fx_rate: record.fx_rate.clone(),
            fee_amount: record.fee_amount.clone(),
            fee_kind: record.fee_kind.clone(),
        }),
    }
}

fn pending_input_from_rule_record(
    kind: &str,
    record: &repositories::RulePayloadRecord,
) -> Result<PendingActivityPayloadInput, AppError> {
    let pending = repositories::PendingPayloadRecord {
        endpoint_account_id: record.endpoint_account_id.clone(),
        endpoint_component: record.endpoint_component.clone(),
        amount: record.amount.clone(),
        currency: record.currency.clone(),
        source_account_id: record.source_account_id.clone(),
        source_component: record.source_component.clone(),
        source_amount: record.source_amount.clone(),
        source_currency: record.source_currency.clone(),
        destination_account_id: record.destination_account_id.clone(),
        destination_component: record.destination_component.clone(),
        destination_amount: record.destination_amount.clone(),
        destination_currency: record.destination_currency.clone(),
        fee_amount: record.fee_amount.clone(),
        fee_currency: record.fee_currency.clone(),
        fee_kind: record.fee_kind.clone(),
        income_kind: record.income_kind.clone(),
        related_instrument_id: record.related_instrument_id.clone(),
        liability_account_id: record.liability_account_id.clone(),
        principal_amount: record.principal_amount.clone(),
        principal_currency: record.principal_currency.clone(),
        cash_account_id: record.cash_account_id.clone(),
        cash_component: record.cash_component.clone(),
        cash_amount: record.cash_amount.clone(),
        cash_currency: record.cash_currency.clone(),
        fx_rate: record.fx_rate.clone(),
        ..Default::default()
    };
    pending_input_from_record(kind, &pending)
}

fn pending_dto(row: &repositories::PendingActivityRecord) -> Result<PendingActivityDto, AppError> {
    Ok(PendingActivityDto {
        id: row.id.clone(),
        recurring_rule_id: row.recurring_rule_id.clone(),
        recurring_rule_revision: row
            .recurring_rule_revision
            .map(|value| i32::try_from(value).map_err(|_| AppError::DatabaseUnavailable))
            .transpose()?,
        scheduled_local_date: row.scheduled_local_date.clone(),
        creation_source: row.creation_source.clone(),
        payload: pending_input_from_record(&row.kind, &row.payload)?,
        note: row.note.clone(),
        status: row.status.clone(),
        posted_activity_id: row.posted_activity_id.clone(),
        skipped_at: row.skipped_at.clone(),
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    })
}

fn recurring_rule_dto(
    row: &repositories::RecurringActivityRuleRecord,
) -> Result<RecurringActivityRuleDto, AppError> {
    Ok(RecurringActivityRuleDto {
        id: row.id.clone(),
        cadence: row.cadence.clone(),
        interval_value: i32::try_from(row.interval_value)
            .map_err(|_| AppError::DatabaseUnavailable)?,
        start_local_date: row.start_local_date.clone(),
        end_local_date: row.end_local_date.clone(),
        anchor_local_date: row.anchor_local_date.clone(),
        payload: pending_input_from_rule_record(&row.kind, &row.payload)?,
        note: row.note.clone(),
        revision: i32::try_from(row.revision).map_err(|_| AppError::DatabaseUnavailable)?,
        archived_at: row.archived_at.clone(),
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    })
}

fn require_open(row: &repositories::PendingActivityRecord) -> Result<(), AppError> {
    if row.status != "open" {
        return Err(AppError::conflict(
            "Posted and skipped pending Activities are immutable.",
        ));
    }
    Ok(())
}

fn validate_status(status: &str) -> Result<(), AppError> {
    if matches!(status, "open" | "posted" | "skipped") {
        Ok(())
    } else {
        Err(AppError::validation(
            "status",
            "Pending Activity status is not supported.",
        ))
    }
}

fn page_limit(limit: Option<i32>) -> Result<i64, AppError> {
    let limit = i64::from(limit.unwrap_or(i32::try_from(DEFAULT_PAGE_SIZE).unwrap_or(50)));
    if !(1..=MAX_PAGE_SIZE).contains(&limit) {
        return Err(AppError::validation(
            "limit",
            "The page size must be between 1 and 100.",
        ));
    }
    Ok(limit)
}

fn encode_pending_cursor(row: &repositories::PendingActivityRecord) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}\n{}\n{}",
        row.scheduled_local_date, row.created_at, row.id
    ))
}

fn decode_pending_cursor(value: &str) -> Result<(String, String, String), AppError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| AppError::validation("cursor", "The pending Activity cursor is invalid."))?;
    let text = String::from_utf8(decoded)
        .map_err(|_| AppError::validation("cursor", "The pending Activity cursor is invalid."))?;
    let mut parts = text.splitn(3, '\n');
    let values = [parts.next(), parts.next(), parts.next()];
    if values.iter().any(Option::is_none) {
        return Err(AppError::validation(
            "cursor",
            "The pending Activity cursor is invalid.",
        ));
    }
    Ok((
        values[0].unwrap_or_default().to_owned(),
        values[1].unwrap_or_default().to_owned(),
        values[2].unwrap_or_default().to_owned(),
    ))
}

fn schedule_from_input(input: &RecurringActivityRuleInput) -> Result<Schedule, AppError> {
    let cadence = ScheduleCadence::parse(&input.cadence)?;
    let interval = ScheduleInterval::new(
        cadence,
        u16::try_from(input.interval_value)
            .map_err(|_| AppError::invalid_recurring_rule("Schedule interval is invalid."))?,
    )?;
    let start = CalendarDate::parse(&input.start_local_date)?;
    let end = input
        .end_local_date
        .as_deref()
        .map(CalendarDate::parse)
        .transpose()?;
    Schedule::new(cadence, interval, start, end)
}

async fn confirmed_today(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<(HistoryOriginRecord, CalendarDate), AppError> {
    let origin = history_repositories::get_origin_by_household(tx, household_id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    if !origin.timezone_confirmed {
        return Err(AppError::HistoryTimezoneConfirmationRequired);
    }
    let timezone = HistoryTimezone::parse(&origin.timezone)?;
    Ok((origin, timezone.local_date(&Timestamp::now())))
}

fn stable_rule_block_reason(error: AppError) -> String {
    match error {
        AppError::Validation { .. }
        | AppError::InvalidActivity { .. }
        | AppError::InvalidCategory { .. }
        | AppError::NotFound { .. }
        | AppError::Conflict { .. }
        | AppError::InvalidPendingActivity { .. }
        | AppError::InvalidRecurringRule { .. } => "rule_reference_invalid".to_owned(),
        _ => "rule_unavailable".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            account_service::{self, CreateAccountInput, OwnershipShareInput},
            onboarding_service::{CompleteOnboardingInput, OnboardingMemberInput},
            overview_service,
        },
        state::AppState,
    };
    use std::{fs, path::PathBuf, time::SystemTime};

    fn test_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "nestworth-phase3-pending-{name}-{}-{nonce}",
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

    async fn state_with_accounts(path: PathBuf) -> (AppState, String, String) {
        let state = AppState::initialize(path).await;
        complete_onboarding_for_test(&state).await;
        let member_id = {
            let database = state.writable_db().expect("database");
            sqlx::query_scalar::<_, String>("SELECT id FROM members LIMIT 1")
                .fetch_one(database)
                .await
                .expect("member")
        };
        let input = |name: &str| CreateAccountInput {
            name: name.to_owned(),
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
                member_id: member_id.clone(),
                percent: Some("100".to_owned()),
                share_bps: None,
            }],
            initial_amount: Some("1000".to_owned()),
        };
        let source = account_service::create_account(&state, input("Source"))
            .await
            .expect("source");
        let destination = account_service::create_account(&state, input("Destination"))
            .await
            .expect("destination");
        (state, source.id, destination.id)
    }

    async fn complete_onboarding_for_test(state: &AppState) {
        super::super::onboarding_service::complete_onboarding(
            state,
            CompleteOnboardingInput {
                household_name: "Phase 3 Household".to_owned(),
                base_currency: "CNY".to_owned(),
                members: vec![OnboardingMemberInput {
                    name: "Owner".to_owned(),
                }],
            },
        )
        .await
        .expect("onboarding");
        let database = state.writable_db().expect("database");
        let confirmed: i64 =
            sqlx::query_scalar("SELECT timezone_confirmed FROM history_origins LIMIT 1")
                .fetch_one(database)
                .await
                .expect("timezone state");
        if confirmed == 0 {
            super::super::history_origin::confirm_history_timezone(state, "UTC")
                .await
                .expect("timezone");
        }
        sqlx::query(
            "UPDATE history_origins
             SET origin_at = '2020-01-01T00:00:00Z', origin_local_date = '2020-01-01'",
        )
        .execute(database)
        .await
        .expect("deterministic origin");
        for sql in [
            "UPDATE activities SET effective_at = '2020-01-02T00:00:00Z', effective_local_date = '2020-01-02'",
            "UPDATE account_values SET effective_at = '2020-01-02T00:00:00Z'",
            "UPDATE account_cash_values SET effective_at = '2020-01-02T00:00:00Z'",
            "UPDATE history_snapshot_state SET dirty_from = NULL, last_completed_on = NULL, rebuild_cursor_on = NULL, rebuild_status = 'idle'",
        ] {
            sqlx::query(sql)
                .execute(database)
                .await
                .expect("deterministic activity time");
        }
    }

    fn deposit(account_id: &str, amount: &str) -> PendingActivityPayloadInput {
        PendingActivityPayloadInput::Deposit {
            account_id: account_id.to_owned(),
            component: "account_value".to_owned(),
            amount: amount.to_owned(),
            currency: "CNY".to_owned(),
        }
    }

    fn transfer(
        source_account_id: &str,
        destination_account_id: &str,
    ) -> PendingActivityPayloadInput {
        PendingActivityPayloadInput::Transfer {
            source_account_id: source_account_id.to_owned(),
            source_component: "account_value".to_owned(),
            source_amount: "10".to_owned(),
            source_currency: "CNY".to_owned(),
            destination_account_id: destination_account_id.to_owned(),
            destination_component: "account_value".to_owned(),
            destination_amount: "10".to_owned(),
            destination_currency: "CNY".to_owned(),
            fee_amount: None,
            fee_kind: None,
        }
    }

    fn today() -> String {
        HistoryTimezone::parse("UTC")
            .expect("timezone")
            .local_date(&Timestamp::now())
            .to_ymd()
    }

    fn pending_deposit_amount(pending: &PendingActivityDto) -> &str {
        match &pending.payload {
            PendingActivityPayloadInput::Deposit { amount, .. } => amount,
            other => panic!("expected deposit payload, got {other:?}"),
        }
    }

    async fn count(state: &AppState, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(state.writable_db().expect("database"))
            .await
            .expect("count")
    }

    #[test]
    fn pending_crud_is_typed_and_terminal_rows_are_immutable() {
        tauri::async_runtime::block_on(async {
            let path = test_path("crud");
            cleanup(&path);
            let (state, source, _) = state_with_accounts(path.clone()).await;
            let pending = create_pending_activity(
                &state,
                CreatePendingActivityInput {
                    scheduled_local_date: "2026-08-20".to_owned(),
                    payload: deposit(&source, "10"),
                    note: Some("  future deposit  ".to_owned()),
                },
            )
            .await
            .expect("pending");
            assert_eq!(pending.status, "open");
            assert_eq!(pending.note.as_deref(), Some("future deposit"));
            let skipped = skip_pending_activity(&state, &pending.id)
                .await
                .expect("skip");
            assert_eq!(skipped.status, "skipped");
            let update = update_pending_activity(
                &state,
                UpdatePendingActivityInput {
                    id: pending.id.clone(),
                    scheduled_local_date: "2026-08-21".to_owned(),
                    payload: deposit(&source, "11"),
                    note: None,
                },
            )
            .await;
            assert!(matches!(update, Err(AppError::Conflict { .. })));
            let listed = list_pending_activities(
                &state,
                ListPendingActivitiesInput {
                    cursor: None,
                    limit: Some(10),
                    status: Some("skipped".to_owned()),
                },
            )
            .await
            .expect("list");
            assert_eq!(listed.items.len(), 1);
            assert_eq!(listed.items[0].id, pending.id);
            cleanup(&path);
        });
    }

    #[test]
    fn pending_preview_and_post_keep_proposals_out_of_financial_reads_until_commit() {
        tauri::async_runtime::block_on(async {
            let path = test_path("post");
            cleanup(&path);
            let (state, source, _) = state_with_accounts(path.clone()).await;
            let before_overview = overview_service::get_overview(&state)
                .await
                .expect("overview");
            let before_activities = count(&state, "SELECT COUNT(*) FROM activities").await;
            let pending = create_pending_activity(
                &state,
                CreatePendingActivityInput {
                    scheduled_local_date: today(),
                    payload: deposit(&source, "10"),
                    note: Some("pending deposit".to_owned()),
                },
            )
            .await
            .expect("pending");
            assert_eq!(
                overview_service::get_overview(&state)
                    .await
                    .expect("pending overview"),
                before_overview
            );
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                before_activities
            );

            let preview = preview_pending_activity(
                &state,
                PendingActivityTimeInput {
                    id: pending.id.clone(),
                    local_date: today(),
                    local_time: "00:01".to_owned(),
                    ambiguous_offset: None,
                },
            )
            .await
            .expect("preview");
            assert_eq!(preview.pending.status, "open");
            assert_eq!(preview.activity.activity.kind, "deposit");
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                before_activities
            );

            let posted = post_pending_activity(
                &state,
                PendingActivityTimeInput {
                    id: pending.id,
                    local_date: today(),
                    local_time: "00:01".to_owned(),
                    ambiguous_offset: None,
                },
            )
            .await
            .expect("post");
            assert_eq!(posted.pending.status, "posted");
            assert_eq!(
                posted.pending.posted_activity_id.as_deref(),
                Some(posted.activity.id.as_str())
            );
            assert_eq!(posted.activity.kind, "deposit");
            assert_eq!(posted.activity.note.as_deref(), Some("pending deposit"));
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                before_activities + 1
            );
            let posted_amount = sqlx::query_scalar::<_, String>(
                "SELECT amount FROM account_values WHERE activity_id = ?",
            )
            .bind(&posted.activity.id)
            .fetch_one(state.writable_db().expect("database"))
            .await
            .expect("posted account value");
            assert_eq!(posted_amount, "1010");
            let dirty_from: Option<String> =
                sqlx::query_scalar("SELECT dirty_from FROM history_snapshot_state LIMIT 1")
                    .fetch_one(state.writable_db().expect("database"))
                    .await
                    .expect("dirty date");
            assert_eq!(dirty_from.as_deref(), Some(today().as_str()));
            cleanup(&path);
        });
    }

    #[test]
    fn future_and_stale_pending_posts_leave_the_row_open_without_activity_mutation() {
        tauri::async_runtime::block_on(async {
            let path = test_path("post-boundaries");
            cleanup(&path);
            let (state, source, _) = state_with_accounts(path.clone()).await;
            let future = create_pending_activity(
                &state,
                CreatePendingActivityInput {
                    scheduled_local_date: "2099-01-01".to_owned(),
                    payload: deposit(&source, "10"),
                    note: None,
                },
            )
            .await
            .expect("future pending");
            let before = count(&state, "SELECT COUNT(*) FROM activities").await;
            let future_post = post_pending_activity(
                &state,
                PendingActivityTimeInput {
                    id: future.id.clone(),
                    local_date: today(),
                    local_time: "00:01".to_owned(),
                    ambiguous_offset: None,
                },
            )
            .await;
            assert!(matches!(
                future_post,
                Err(AppError::InvalidPendingActivity { .. })
            ));
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                before
            );
            assert_eq!(
                list_pending_activities(
                    &state,
                    ListPendingActivitiesInput {
                        cursor: None,
                        limit: Some(10),
                        status: Some("open".to_owned()),
                    },
                )
                .await
                .expect("future list")
                .items
                .len(),
                1
            );

            let stale = create_pending_activity(
                &state,
                CreatePendingActivityInput {
                    scheduled_local_date: today(),
                    payload: deposit(&source, "11"),
                    note: None,
                },
            )
            .await
            .expect("stale pending");
            preview_pending_activity(
                &state,
                PendingActivityTimeInput {
                    id: stale.id.clone(),
                    local_date: today(),
                    local_time: "00:01".to_owned(),
                    ambiguous_offset: None,
                },
            )
            .await
            .expect("stale preview");
            account_service::archive_account(&state, &source)
                .await
                .expect("archive source");
            let stale_post = post_pending_activity(
                &state,
                PendingActivityTimeInput {
                    id: stale.id.clone(),
                    local_date: today(),
                    local_time: "00:01".to_owned(),
                    ambiguous_offset: None,
                },
            )
            .await;
            match stale_post {
                Err(error) => assert!(
                    matches!(error, AppError::Validation { .. }),
                    "unexpected stale-post error: {error:?}"
                ),
                Ok(value) => panic!("stale post unexpectedly succeeded: {value:?}"),
            }
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                before
            );
            let open = list_pending_activities(
                &state,
                ListPendingActivitiesInput {
                    cursor: None,
                    limit: Some(10),
                    status: Some("open".to_owned()),
                },
            )
            .await
            .expect("open list");
            assert_eq!(open.items.len(), 2);
            assert!(open.items.iter().all(|item| item.status == "open"));
            cleanup(&path);
        });
    }

    #[test]
    fn concurrent_pending_posts_have_one_winner_and_one_terminal_conflict() {
        tauri::async_runtime::block_on(async {
            let path = test_path("concurrent-post");
            cleanup(&path);
            let (state, source, _) = state_with_accounts(path.clone()).await;
            let pending = create_pending_activity(
                &state,
                CreatePendingActivityInput {
                    scheduled_local_date: today(),
                    payload: deposit(&source, "10"),
                    note: None,
                },
            )
            .await
            .expect("pending");
            let input = PendingActivityTimeInput {
                id: pending.id.clone(),
                local_date: today(),
                local_time: "00:01".to_owned(),
                ambiguous_offset: None,
            };
            let (first, second) = tokio::join!(
                post_pending_activity(&state, input.clone()),
                post_pending_activity(&state, input),
            );
            assert_eq!(first.is_ok() as u8 + second.is_ok() as u8, 1);
            let error = first.err().or_else(|| second.err()).expect("loser");
            assert!(matches!(error, AppError::Conflict { .. }));
            assert_eq!(count(&state, "SELECT COUNT(*) FROM activities").await, 3);
            let posted = list_pending_activities(
                &state,
                ListPendingActivitiesInput {
                    cursor: None,
                    limit: Some(10),
                    status: Some("posted".to_owned()),
                },
            )
            .await
            .expect("posted list");
            assert_eq!(posted.items.len(), 1);
            assert_eq!(posted.items[0].id, pending.id);
            cleanup(&path);
        });
    }

    #[test]
    fn recurring_generation_is_idempotent_and_bounded() {
        tauri::async_runtime::block_on(async {
            let path = test_path("generation");
            cleanup(&path);
            let (state, source, _) = state_with_accounts(path.clone()).await;
            let rule = create_recurring_activity_rule(
                &state,
                CreateRecurringActivityRuleInput {
                    rule: RecurringActivityRuleInput {
                        cadence: "daily".to_owned(),
                        interval_value: 1,
                        start_local_date: "2025-01-01".to_owned(),
                        end_local_date: Some("2027-01-01".to_owned()),
                        payload: deposit(&source, "1"),
                        note: None,
                    },
                },
            )
            .await
            .expect("rule");
            let generated = generate_due_pending_activities(&state)
                .await
                .expect("generation");
            assert!(generated.generated_count > 0);
            assert!(generated.generated_count <= 366);
            assert!(generated.has_more);
            let updated = update_recurring_activity_rule(
                &state,
                UpdateRecurringActivityRuleInput {
                    id: rule.id.clone(),
                    end_local_date: Some("2027-01-01".to_owned()),
                    payload: deposit(&source, "2"),
                    note: Some("updated".to_owned()),
                },
            )
            .await
            .expect("update rule");
            assert_eq!(updated.revision, 2);
            assert_eq!(updated.start_local_date, "2025-01-01");
            assert_eq!(updated.anchor_local_date, "2025-01-01");
            let first_page = list_pending_activities(
                &state,
                ListPendingActivitiesInput {
                    cursor: None,
                    limit: Some(1),
                    status: Some("open".to_owned()),
                },
            )
            .await
            .expect("first generated row");
            assert_eq!(first_page.items.len(), 1);
            assert_eq!(first_page.items[0].recurring_rule_revision, Some(1));
            assert_eq!(pending_deposit_amount(&first_page.items[0]), "1");

            archive_recurring_activity_rule(&state, &rule.id)
                .await
                .expect("archive rule");
            let archived = generate_due_pending_activities(&state)
                .await
                .expect("archived generation");
            assert_eq!(archived.generated_count, 0);
            restore_recurring_activity_rule(&state, &rule.id)
                .await
                .expect("restore rule");
            let again = generate_due_pending_activities(&state)
                .await
                .expect("continuation");
            assert!(again.generated_count > 0);
            assert!(again.generated_count <= 366);
            let done = generate_due_pending_activities(&state)
                .await
                .expect("idempotent generation");
            assert_eq!(done.generated_count, 0);
            let database = state.writable_db().expect("database");
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM pending_activities WHERE recurring_rule_id = ?",
            )
            .bind(rule.id)
            .fetch_one(database)
            .await
            .expect("count");
            assert_eq!(
                count,
                i64::from(generated.generated_count + again.generated_count)
            );
            cleanup(&path);
        });
    }

    #[test]
    fn invalid_recurring_reference_is_blocked_without_generating_a_row() {
        tauri::async_runtime::block_on(async {
            let path = test_path("blocked");
            cleanup(&path);
            let (state, source, _) = state_with_accounts(path.clone()).await;
            let rule = create_recurring_activity_rule(
                &state,
                CreateRecurringActivityRuleInput {
                    rule: RecurringActivityRuleInput {
                        cadence: "daily".to_owned(),
                        interval_value: 1,
                        start_local_date: today(),
                        end_local_date: Some(today()),
                        payload: deposit(&source, "1"),
                        note: None,
                    },
                },
            )
            .await
            .expect("rule");
            account_service::archive_account(&state, &source)
                .await
                .expect("archive source");
            let generated = generate_due_pending_activities(&state)
                .await
                .expect("blocked generation");
            assert_eq!(generated.generated_count, 0);
            assert_eq!(generated.blocked.len(), 1);
            assert_eq!(generated.blocked[0].rule_id, rule.id);
            assert_eq!(generated.blocked[0].reason, "rule_reference_invalid");
            assert_eq!(
                count(
                    &state,
                    "SELECT COUNT(*) FROM pending_activities WHERE recurring_rule_id IS NOT NULL",
                )
                .await,
                0
            );
            cleanup(&path);
        });
    }

    #[test]
    fn same_currency_recurring_transfer_is_neutral_until_and_after_post() {
        tauri::async_runtime::block_on(async {
            let path = test_path("transfer");
            cleanup(&path);
            let (state, source, destination) = state_with_accounts(path.clone()).await;
            let before = overview_service::get_overview(&state)
                .await
                .expect("overview");
            let cross_currency = create_recurring_activity_rule(
                &state,
                CreateRecurringActivityRuleInput {
                    rule: RecurringActivityRuleInput {
                        cadence: "daily".to_owned(),
                        interval_value: 1,
                        start_local_date: today(),
                        end_local_date: Some(today()),
                        payload: PendingActivityPayloadInput::Transfer {
                            source_account_id: source.clone(),
                            source_component: "account_value".to_owned(),
                            source_amount: "10".to_owned(),
                            source_currency: "CNY".to_owned(),
                            destination_account_id: destination.clone(),
                            destination_component: "account_value".to_owned(),
                            destination_amount: "10".to_owned(),
                            destination_currency: "USD".to_owned(),
                            fee_amount: None,
                            fee_kind: None,
                        },
                        note: None,
                    },
                },
            )
            .await;
            assert!(matches!(
                cross_currency,
                Err(AppError::InvalidRecurringRule { .. })
            ));
            let transfer_rule = create_recurring_activity_rule(
                &state,
                CreateRecurringActivityRuleInput {
                    rule: RecurringActivityRuleInput {
                        cadence: "daily".to_owned(),
                        interval_value: 1,
                        start_local_date: today(),
                        end_local_date: Some(today()),
                        payload: transfer(&source, &destination),
                        note: None,
                    },
                },
            )
            .await
            .unwrap_or_else(|error| panic!("transfer rule: {error:?}"));
            let generated = generate_due_pending_activities(&state)
                .await
                .unwrap_or_else(|error| panic!("generate transfer: {error:?}"));
            assert_eq!(
                generated.generated_count, 1,
                "generation result: {generated:?}"
            );
            assert_eq!(
                overview_service::get_overview(&state)
                    .await
                    .expect("pending overview"),
                before
            );
            let pending = list_pending_activities(
                &state,
                ListPendingActivitiesInput {
                    cursor: None,
                    limit: Some(10),
                    status: Some("open".to_owned()),
                },
            )
            .await
            .expect("pending list")
            .items
            .into_iter()
            .find(|item| item.recurring_rule_id.as_deref() == Some(transfer_rule.id.as_str()))
            .expect("transfer pending");
            let posted = post_pending_activity(
                &state,
                PendingActivityTimeInput {
                    id: pending.id,
                    local_date: today(),
                    local_time: "00:01".to_owned(),
                    ambiguous_offset: None,
                },
            )
            .await
            .expect("post transfer");
            assert_eq!(posted.activity.kind, "transfer");
            assert_eq!(
                overview_service::get_overview(&state)
                    .await
                    .expect("posted overview"),
                before
            );
            cleanup(&path);
        });
    }
}
