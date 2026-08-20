//! Activity, origin, and account timeline queries plus kind-specific IPC inputs.

use std::collections::HashMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    activity_service::{
        self, ActivityPreview, ActivityTimeSpec, PostCommand, PreviewEndpointChange,
    },
    fx_conversion::{self, ActivityFxConversionDto},
    history_origin,
    history_repositories::{
        self, ActivityListCursor, ActivityListFilter, CorrectionLink, HistoryOriginRecord,
        InstrumentLabel,
    },
    query_count,
    reference::{
        begin_read_tx, begin_write_tx, finish_read_tx, finish_write_tx, require_household_tx,
    },
};
use crate::{
    domain::{
        classify, AccountId, Activity, ActivityKind, AmbiguousOffset, Classification,
        ComponentOpening, CurrencyCode, DebtCashLink, DebtDrawSpec, DebtPaymentSpec, FeeKind,
        HoldingId, IncomeKind, InstrumentId, LegComponent, MonetaryComponent, MonetaryEndpoint,
        Money, PendingActivityPayload, Quantity, QuantityEndpoint, TradeSpec, UnitPrice,
    },
    error::AppError,
    state::AppState,
};

const DEFAULT_PAGE_SIZE: i64 = 50;
const MAX_PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmHistoryTimezoneInput {
    pub timezone: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoryOriginDto {
    pub id: String,
    pub timezone: String,
    pub timezone_confirmed: bool,
    pub origin_at: String,
    pub origin_local_date: String,
    pub source: String,
    pub schema_version: i32,
    pub created_at: String,
    pub account_values: Vec<OriginComponentDto>,
    pub cash_values: Vec<OriginComponentDto>,
    pub holdings: Vec<OriginHoldingDto>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OriginComponentDto {
    pub account_id: String,
    pub amount: String,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OriginHoldingDto {
    pub holding_id: String,
    pub account_id: String,
    pub instrument_id: String,
    pub quantity: String,
    pub active: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListActivitiesInput {
    pub cursor: Option<String>,
    pub limit: Option<i32>,
    pub start_local_date: Option<String>,
    pub end_local_date: Option<String>,
    pub account_id: Option<String>,
    pub instrument_id: Option<String>,
    pub kind: Option<String>,
    pub classification: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActivityPageDto {
    pub items: Vec<ActivityDetailDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDetailDto {
    pub id: String,
    pub kind: String,
    pub classification: String,
    pub effective_at: String,
    pub effective_local_date: String,
    pub created_at: String,
    pub note: Option<String>,
    pub reverses: Option<String>,
    pub corrects: Option<String>,
    pub correction_group: Option<String>,
    pub income_kind: Option<String>,
    pub fee_kind: Option<String>,
    pub related_instrument_id: Option<String>,
    pub reversed: bool,
    pub is_reversal: bool,
    pub is_replacement: bool,
    pub legs: Vec<ActivityLegDto>,
    pub chain: CorrectionChainDto,
    pub fx_conversion: Option<ActivityFxConversionDto>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLegDto {
    pub id: String,
    pub account_id: String,
    pub account_name: String,
    pub role: String,
    pub direction: String,
    pub component_kind: String,
    pub classification: String,
    pub amount: Option<String>,
    pub currency: Option<String>,
    pub holding_id: Option<String>,
    pub instrument_id: Option<String>,
    pub instrument_name: Option<String>,
    pub quantity: Option<String>,
    pub fx_rate: Option<String>,
    pub sort_order: i32,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionChainDto {
    pub original_id: String,
    pub reversal_id: Option<String>,
    pub replacement_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActivityPreviewDto {
    pub activity: ActivityDetailDto,
    pub resulting: Vec<ResultingEndpointDto>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResultingEndpointDto {
    pub account_id: String,
    pub account_name: String,
    pub component_kind: String,
    pub holding_id: Option<String>,
    pub currency: Option<String>,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReverseActivityInput {
    pub id: String,
    pub local_date: Option<String>,
    pub local_time: Option<String>,
    pub ambiguous_offset: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CorrectActivityInput {
    pub original_id: String,
    pub replacement: CreateActivityInput,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PostedCorrectionDto {
    pub reversal: ActivityDetailDto,
    pub replacement: ActivityDetailDto,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountTimelineInput {
    pub account_id: String,
    pub cursor: Option<String>,
    pub limit: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountTimelinePageDto {
    pub items: Vec<AccountTimelineItemDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum AccountTimelineItemDto {
    #[serde(rename_all = "camelCase")]
    Origin {
        id: String,
        occurred_at: String,
        created_at: String,
        local_date: String,
        label: String,
    },
    #[serde(rename_all = "camelCase")]
    Activity {
        occurred_at: String,
        created_at: String,
        activity: ActivityDetailDto,
    },
    #[serde(rename_all = "camelCase")]
    Observation {
        id: String,
        occurred_at: String,
        created_at: String,
        component_kind: String,
        amount: String,
        currency: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    AccountState {
        id: String,
        occurred_at: String,
        created_at: String,
        archived: bool,
        primary_category: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreateActivityInput {
    #[serde(rename_all = "camelCase")]
    OpeningAdjustment {
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
        account_id: String,
        component: String,
        amount: Option<String>,
        currency: Option<String>,
        holding_id: Option<String>,
        instrument_id: Option<String>,
        quantity: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    BalanceAdjustment {
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
        account_id: String,
        amount: String,
        currency: String,
    },
    #[serde(rename_all = "camelCase")]
    PositionAdjustment {
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
        holding_id: String,
        quantity: String,
    },
    #[serde(rename_all = "camelCase")]
    Deposit {
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
        account_id: String,
        component: String,
        amount: String,
        currency: String,
    },
    #[serde(rename_all = "camelCase")]
    Withdrawal {
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
        account_id: String,
        component: String,
        amount: String,
        currency: String,
    },
    #[serde(rename_all = "camelCase")]
    Transfer {
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
        source_account_id: String,
        source_component: String,
        source_amount: String,
        source_currency: String,
        destination_account_id: String,
        destination_component: String,
        destination_amount: String,
        destination_currency: String,
        source_holding_id: Option<String>,
        destination_holding_id: Option<String>,
        quantity: Option<String>,
        fee_amount: Option<String>,
        fee_kind: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Buy {
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
        holding_id: String,
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
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
        holding_id: String,
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
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
        account_id: String,
        component: String,
        amount: String,
        currency: String,
        income_kind: String,
        instrument_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Fee {
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
        account_id: String,
        component: String,
        amount: String,
        currency: String,
        fee_kind: String,
        instrument_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    DebtDraw {
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
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
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
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
    #[serde(rename_all = "camelCase")]
    DebtAdjustment {
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
        account_id: String,
        amount: String,
        currency: String,
    },
    #[serde(rename_all = "camelCase")]
    ManualValuation {
        local_date: String,
        local_time: String,
        ambiguous_offset: Option<String>,
        note: Option<String>,
        account_id: String,
        amount: String,
        currency: String,
    },
}

struct PostingFields {
    local_date: String,
    local_time: String,
    ambiguous_offset: Option<String>,
    note: Option<String>,
}

impl CreateActivityInput {
    fn posting_fields(&self) -> PostingFields {
        match self {
            Self::OpeningAdjustment {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::BalanceAdjustment {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::PositionAdjustment {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::Deposit {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::Withdrawal {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::Transfer {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::Buy {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::Sell {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::Income {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::Fee {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::DebtDraw {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::DebtPayment {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::DebtAdjustment {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            }
            | Self::ManualValuation {
                local_date,
                local_time,
                ambiguous_offset,
                note,
                ..
            } => PostingFields {
                local_date: local_date.clone(),
                local_time: local_time.clone(),
                ambiguous_offset: ambiguous_offset.clone(),
                note: note.clone(),
            },
        }
    }
}

pub async fn get_history_origin(state: &AppState) -> Result<HistoryOriginDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_history_origin_in_tx(&mut tx).await;
    finish_read_tx(tx, result).await
}

pub async fn confirm_history_timezone(
    state: &AppState,
    input: ConfirmHistoryTimezoneInput,
) -> Result<HistoryOriginDto, AppError> {
    history_origin::confirm_history_timezone(state, &input.timezone).await?;
    get_history_origin(state).await
}

pub async fn list_activities(
    state: &AppState,
    input: ListActivitiesInput,
) -> Result<ActivityPageDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = list_activities_in_tx(&mut tx, input).await;
    finish_read_tx(tx, result).await
}

pub async fn get_activity(state: &AppState, id: &str) -> Result<ActivityDetailDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_activity_in_tx(&mut tx, id).await;
    finish_read_tx(tx, result).await
}

pub async fn preview_activity(
    state: &AppState,
    input: CreateActivityInput,
) -> Result<ActivityPreviewDto, AppError> {
    let fields = input.posting_fields();
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let command = post_command_from_input(&mut tx, &household.id, input).await?;
        let time = time_spec(&fields)?;
        let preview =
            activity_service::preview_in_tx(&mut tx, command, Some(time), fields.note.as_deref())
                .await?;
        preview_dto(&mut tx, &household.id, &household.base_currency, preview).await
    }
    .await;
    finish_read_tx(tx, result).await
}

pub async fn create_activity(
    state: &AppState,
    input: CreateActivityInput,
) -> Result<ActivityDetailDto, AppError> {
    let fields = input.posting_fields();
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let command = post_command_from_input(&mut tx, &household.id, input).await?;
        let time = time_spec(&fields)?;
        let activity =
            activity_service::post_in_tx(&mut tx, command, Some(time), fields.note.as_deref())
                .await?;
        activity_detail(&mut tx, &household.id, &household.base_currency, &activity).await
    }
    .await;
    finish_write_tx(tx, result).await
}

pub(crate) async fn preview_pending_command_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    base_currency: &str,
    command: PostCommand,
    time: ActivityTimeSpec<'_>,
    note: Option<&str>,
) -> Result<ActivityPreviewDto, AppError> {
    let preview = activity_service::preview_in_tx(tx, command, Some(time), note).await?;
    preview_dto(tx, household_id, base_currency, preview).await
}

pub(crate) async fn post_pending_command_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    base_currency: &str,
    command: PostCommand,
    time: ActivityTimeSpec<'_>,
    note: Option<&str>,
) -> Result<ActivityDetailDto, AppError> {
    let activity = activity_service::post_in_tx(tx, command, Some(time), note).await?;
    activity_detail(tx, household_id, base_currency, &activity).await
}

pub async fn reverse_activity(
    state: &AppState,
    input: ReverseActivityInput,
) -> Result<ActivityDetailDto, AppError> {
    let time = optional_time_spec(
        input.local_date.as_deref(),
        input.local_time.as_deref(),
        input.ambiguous_offset.as_deref(),
    )?;
    let activity = activity_service::reverse_activity(state, &input.id, time).await?;
    get_activity(state, &activity.id().to_string()).await
}

pub async fn correct_activity(
    state: &AppState,
    input: CorrectActivityInput,
) -> Result<PostedCorrectionDto, AppError> {
    let fields = input.replacement.posting_fields();
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let replacement =
            post_command_from_input(&mut tx, &household.id, input.replacement).await?;
        let time = time_spec(&fields)?;
        let posted = activity_service::correct_activity_in_tx(
            &mut tx,
            &input.original_id,
            replacement,
            Some(time),
        )
        .await?;
        Ok(PostedCorrectionDto {
            reversal: activity_detail(
                &mut tx,
                &household.id,
                &household.base_currency,
                &posted.reversal,
            )
            .await?,
            replacement: activity_detail(
                &mut tx,
                &household.id,
                &household.base_currency,
                &posted.replacement,
            )
            .await?,
        })
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn get_account_timeline(
    state: &AppState,
    input: GetAccountTimelineInput,
) -> Result<AccountTimelinePageDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_account_timeline_in_tx(&mut tx, input).await;
    finish_read_tx(tx, result).await
}

async fn get_history_origin_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<HistoryOriginDto, AppError> {
    let household = require_household_tx(tx).await?;
    let origin = history_repositories::get_origin_by_household(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    origin_dto(tx, origin).await
}

async fn origin_dto(
    tx: &mut Transaction<'_, Sqlite>,
    origin: HistoryOriginRecord,
) -> Result<HistoryOriginDto, AppError> {
    query_count::record("origin_baseline");
    let account_values = history_repositories::list_origin_account_values(tx, &origin.id)
        .await?
        .into_iter()
        .map(|row| OriginComponentDto {
            account_id: row.account_id,
            amount: row.amount,
            currency: row.currency,
        })
        .collect();
    let cash_values = history_repositories::list_origin_cash_values(tx, &origin.id)
        .await?
        .into_iter()
        .map(|row| OriginComponentDto {
            account_id: row.account_id,
            amount: row.amount,
            currency: row.currency,
        })
        .collect();
    let holdings = history_repositories::list_origin_holdings(tx, &origin.id)
        .await?
        .into_iter()
        .map(|row| OriginHoldingDto {
            holding_id: row.holding_id,
            account_id: row.account_id,
            instrument_id: row.instrument_id,
            quantity: row.quantity,
            active: row.active,
        })
        .collect();
    Ok(HistoryOriginDto {
        id: origin.id,
        timezone: origin.timezone,
        timezone_confirmed: origin.timezone_confirmed,
        origin_at: origin.origin_at,
        origin_local_date: origin.origin_local_date,
        source: origin.source,
        schema_version: i32::try_from(origin.schema_version).unwrap_or(0),
        created_at: origin.created_at,
        account_values,
        cash_values,
        holdings,
    })
}

async fn list_activities_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: ListActivitiesInput,
) -> Result<ActivityPageDto, AppError> {
    let household = require_household_tx(tx).await?;
    let limit = page_limit(input.limit)?;
    if let Some(account_id) = input.account_id.as_deref() {
        require_household_account(tx, &household.id, account_id).await?;
    }
    if let Some(instrument_id) = input.instrument_id.as_deref() {
        require_household_instrument(tx, &household.id, instrument_id).await?;
    }
    if let Some(kind) = input.kind.as_deref() {
        ActivityKind::parse(kind)?;
    }
    if let Some(classification) = input.classification.as_deref() {
        Classification::parse(classification)?;
    }
    if let Some(date) = input.start_local_date.as_deref() {
        crate::domain::CalendarDate::parse(date)?;
    }
    if let Some(date) = input.end_local_date.as_deref() {
        crate::domain::CalendarDate::parse(date)?;
    }
    let cursor = input
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
    let filter = ActivityListFilter {
        start_local_date: input.start_local_date.as_deref(),
        end_local_date: input.end_local_date.as_deref(),
        account_id: input.account_id.as_deref(),
        instrument_id: input.instrument_id.as_deref(),
        kind: input.kind.as_deref(),
        classification: input.classification.as_deref(),
    };
    let mut rows = history_repositories::list_activities_filtered(
        tx,
        &household.id,
        cursor.as_ref(),
        limit + 1,
        filter,
    )
    .await?;
    let has_more = i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit;
    if has_more {
        rows.truncate(usize::try_from(limit).unwrap_or(rows.len()));
    }
    let labels = load_labels(tx, &household.id).await?;
    let chains = load_chains_for_activities(tx, &rows).await?;
    let items = rows
        .iter()
        .map(|activity| {
            let original_id = chain_original_id(activity);
            activity_detail_from(
                &labels,
                activity,
                Some(chain_dto(&original_id, chains.get(&original_id))),
                None,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = has_more
        .then(|| rows.last().map(encode_activity_cursor))
        .flatten();
    tracing::info!(
        event = "activity.list",
        count = items.len(),
        "activities listed"
    );
    Ok(ActivityPageDto {
        items,
        next_cursor,
        has_more,
    })
}

async fn get_activity_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> Result<ActivityDetailDto, AppError> {
    let household = require_household_tx(tx).await?;
    let activity = history_repositories::get_activity(tx, id)
        .await?
        .ok_or_else(|| AppError::not_found("activity", id))?;
    if activity.household_id().to_string() != household.id {
        return Err(AppError::not_found("activity", id));
    }
    activity_detail(tx, &household.id, &household.base_currency, &activity).await
}

async fn get_account_timeline_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: GetAccountTimelineInput,
) -> Result<AccountTimelinePageDto, AppError> {
    let household = require_household_tx(tx).await?;
    require_household_account(tx, &household.id, &input.account_id).await?;
    let origin = history_repositories::get_origin_by_household(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    let limit = page_limit(input.limit)?;
    let labels = load_labels(tx, &household.id).await?;
    let mut items = Vec::new();
    items.push(AccountTimelineItemDto::Origin {
        id: origin.id.clone(),
        occurred_at: origin.origin_at.clone(),
        created_at: origin.created_at.clone(),
        local_date: origin.origin_local_date.clone(),
        label: "opening_state".to_owned(),
    });
    let activities = history_repositories::list_activities_touching_account(
        tx,
        &household.id,
        &input.account_id,
    )
    .await?;
    let chains = load_chains_for_activities(tx, &activities).await?;
    for activity in activities {
        let original_id = chain_original_id(&activity);
        items.push(AccountTimelineItemDto::Activity {
            occurred_at: activity.effective_at().to_rfc3339(),
            created_at: activity.created_at().to_rfc3339(),
            activity: activity_detail_from(
                &labels,
                &activity,
                Some(chain_dto(&original_id, chains.get(&original_id))),
                None,
            )?,
        });
    }
    let observations = history_repositories::list_legacy_account_observations(
        tx,
        &input.account_id,
        &origin.origin_at,
    )
    .await?;
    for observation in observations {
        items.push(AccountTimelineItemDto::Observation {
            id: observation.id,
            occurred_at: observation.occurred_at,
            created_at: observation.created_at,
            component_kind: observation.kind,
            amount: observation.amount,
            currency: observation.currency,
        });
    }
    let states =
        history_repositories::list_account_state_changes(tx, &input.account_id, &origin.origin_at)
            .await?;
    for state in states {
        items.push(AccountTimelineItemDto::AccountState {
            id: state.id,
            occurred_at: state.effective_at,
            created_at: state.created_at,
            archived: state.archived_at.is_some(),
            primary_category: state.primary_category,
        });
    }
    items.sort_by_key(|item| std::cmp::Reverse(timeline_sort_key(item)));
    let cursor = input
        .cursor
        .as_deref()
        .map(decode_timeline_cursor)
        .transpose()?;
    if let Some(cursor) = cursor {
        items.retain(|item| timeline_sort_key(item) < cursor);
    }
    let has_more = i64::try_from(items.len()).unwrap_or(i64::MAX) > limit;
    if has_more {
        items.truncate(usize::try_from(limit).unwrap_or(items.len()));
    }
    let next_cursor = has_more
        .then(|| items.last().map(encode_timeline_cursor))
        .flatten();
    tracing::info!(
        event = "history.timeline",
        count = items.len(),
        "account timeline listed"
    );
    Ok(AccountTimelinePageDto {
        items,
        next_cursor,
        has_more,
    })
}

struct LabelMaps {
    accounts: HashMap<String, String>,
    instruments: HashMap<String, InstrumentLabel>,
}

async fn load_labels(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<LabelMaps, AppError> {
    Ok(LabelMaps {
        accounts: history_repositories::list_account_labels(tx, household_id).await?,
        instruments: history_repositories::list_instrument_labels(tx, household_id).await?,
    })
}

async fn activity_detail(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    base_currency: &str,
    activity: &Activity,
) -> Result<ActivityDetailDto, AppError> {
    let labels = load_labels(tx, household_id).await?;
    let chain = load_chain(tx, activity).await?;
    let fx_conversion =
        fx_conversion::overlay_for_activity(tx, household_id, base_currency, activity).await?;
    activity_detail_from(&labels, activity, Some(chain), fx_conversion)
}

async fn load_chains_for_activities(
    tx: &mut Transaction<'_, Sqlite>,
    activities: &[Activity],
) -> Result<HashMap<String, CorrectionLink>, AppError> {
    let mut original_ids: Vec<String> = activities.iter().map(chain_original_id).collect();
    original_ids.sort();
    original_ids.dedup();
    history_repositories::list_correction_links(tx, &original_ids).await
}

fn chain_original_id(activity: &Activity) -> String {
    activity
        .reverses()
        .or(activity.corrects())
        .unwrap_or_else(|| activity.id())
        .to_string()
}

fn chain_dto(original_id: &str, link: Option<&CorrectionLink>) -> CorrectionChainDto {
    CorrectionChainDto {
        original_id: original_id.to_owned(),
        reversal_id: link.and_then(|item| item.reversal_id.clone()),
        replacement_id: link.and_then(|item| item.replacement_id.clone()),
    }
}

async fn load_chain(
    tx: &mut Transaction<'_, Sqlite>,
    activity: &Activity,
) -> Result<CorrectionChainDto, AppError> {
    let original_id = chain_original_id(activity);
    let links =
        history_repositories::list_correction_links(tx, std::slice::from_ref(&original_id)).await?;
    Ok(chain_dto(&original_id, links.get(&original_id)))
}

fn activity_detail_from(
    labels: &LabelMaps,
    activity: &Activity,
    chain: Option<CorrectionChainDto>,
    fx_conversion: Option<ActivityFxConversionDto>,
) -> Result<ActivityDetailDto, AppError> {
    let chain = chain.unwrap_or(CorrectionChainDto {
        original_id: activity
            .reverses()
            .or(activity.corrects())
            .unwrap_or_else(|| activity.id())
            .to_string(),
        reversal_id: None,
        replacement_id: None,
    });
    let reversed = chain.reversal_id.is_some() && activity.reverses().is_none();
    Ok(ActivityDetailDto {
        id: activity.id().to_string(),
        kind: activity.kind().as_str().to_owned(),
        classification: header_classification(activity).as_str().to_owned(),
        effective_at: activity.effective_at().to_rfc3339(),
        effective_local_date: activity.effective_local_date().to_ymd(),
        created_at: activity.created_at().to_rfc3339(),
        note: activity.note().map(ToOwned::to_owned),
        reverses: activity.reverses().map(|id| id.to_string()),
        corrects: activity.corrects().map(|id| id.to_string()),
        correction_group: activity.correction_group().map(|id| id.to_string()),
        income_kind: activity.income_kind().map(|kind| kind.as_str().to_owned()),
        fee_kind: activity.fee_kind().map(|kind| kind.as_str().to_owned()),
        related_instrument_id: activity.related_instrument_id().map(|id| id.to_string()),
        reversed,
        is_reversal: activity.kind() == ActivityKind::Reversal,
        is_replacement: activity.corrects().is_some(),
        legs: activity
            .legs()
            .iter()
            .map(|leg| {
                let (amount, currency, holding_id, instrument_id, quantity) = match leg.component()
                {
                    LegComponent::AccountValue { amount }
                    | LegComponent::HoldingsCash { amount } => (
                        Some(amount.canonical_amount()),
                        Some(amount.currency().as_str().to_owned()),
                        None,
                        None,
                        None,
                    ),
                    LegComponent::HoldingQuantity {
                        holding_id,
                        instrument_id,
                        quantity,
                    } => (
                        None,
                        None,
                        Some(holding_id.to_string()),
                        Some(instrument_id.to_string()),
                        Some(quantity.canonical()),
                    ),
                };
                ActivityLegDto {
                    id: leg.id().to_string(),
                    account_id: leg.account_id().to_string(),
                    account_name: labels
                        .accounts
                        .get(&leg.account_id().to_string())
                        .cloned()
                        .unwrap_or_default(),
                    role: leg.role().as_str().to_owned(),
                    direction: leg.direction().as_str().to_owned(),
                    component_kind: leg.component_kind().as_str().to_owned(),
                    classification: classify(activity.kind(), leg.role()).as_str().to_owned(),
                    amount,
                    currency,
                    holding_id,
                    instrument_id: instrument_id.clone(),
                    instrument_name: instrument_id
                        .as_ref()
                        .and_then(|id| labels.instruments.get(id).map(|label| label.name.clone())),
                    quantity,
                    fx_rate: leg.fx_rate().map(|rate| rate.canonical()),
                    sort_order: i32::try_from(leg.sort_order()).unwrap_or(0),
                }
            })
            .collect(),
        chain,
        fx_conversion,
    })
}

async fn preview_dto(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    base_currency: &str,
    preview: ActivityPreview,
) -> Result<ActivityPreviewDto, AppError> {
    let labels = load_labels(tx, household_id).await?;
    let fx_conversion =
        fx_conversion::overlay_for_activity(tx, household_id, base_currency, &preview.activity)
            .await?;
    Ok(ActivityPreviewDto {
        activity: activity_detail_from(&labels, &preview.activity, None, fx_conversion)?,
        resulting: preview
            .endpoints
            .into_iter()
            .map(|change| resulting_dto(&labels, change))
            .collect(),
    })
}

fn resulting_dto(labels: &LabelMaps, change: PreviewEndpointChange) -> ResultingEndpointDto {
    ResultingEndpointDto {
        account_name: labels
            .accounts
            .get(&change.account_id)
            .cloned()
            .unwrap_or_default(),
        account_id: change.account_id,
        component_kind: change.component_kind,
        holding_id: change.holding_id,
        currency: change.currency,
        before: change.before_amount.unwrap_or_else(|| "0".to_owned()),
        after: change.after_amount.unwrap_or_else(|| "0".to_owned()),
    }
}

fn header_classification(activity: &Activity) -> Classification {
    let role = activity
        .legs()
        .iter()
        .find(|leg| {
            !matches!(
                (activity.kind(), leg.role()),
                (
                    ActivityKind::Buy
                        | ActivityKind::Sell
                        | ActivityKind::DebtDraw
                        | ActivityKind::DebtPayment
                        | ActivityKind::Transfer,
                    crate::domain::LegRole::Fee
                )
            )
        })
        .or_else(|| activity.legs().first())
        .map(crate::domain::ActivityLeg::role)
        .unwrap_or(crate::domain::LegRole::Adjustment);
    classify(activity.kind(), role)
}

fn page_limit(limit: Option<i32>) -> Result<i64, AppError> {
    let limit = i64::from(limit.unwrap_or(DEFAULT_PAGE_SIZE as i32));
    if limit < 1 {
        return Err(AppError::validation(
            "limit",
            "Page size must be at least 1.",
        ));
    }
    if limit > MAX_PAGE_SIZE {
        return Err(AppError::validation(
            "limit",
            "Page size cannot exceed 100.",
        ));
    }
    Ok(limit)
}

async fn require_household_account(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    account_id: &str,
) -> Result<(), AppError> {
    if history_repositories::household_account_exists(tx, household_id, account_id).await? {
        Ok(())
    } else {
        Err(AppError::not_found("account", account_id))
    }
}

async fn require_household_instrument(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    instrument_id: &str,
) -> Result<(), AppError> {
    if history_repositories::household_instrument_exists(tx, household_id, instrument_id).await? {
        Ok(())
    } else {
        Err(AppError::not_found("instrument", instrument_id))
    }
}

fn encode_activity_cursor(activity: &Activity) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}\n{}\n{}",
        activity.effective_at().to_rfc3339(),
        activity.created_at().to_rfc3339(),
        activity.id()
    ))
}

fn decode_activity_cursor(value: &str) -> Result<ActivityListCursor, AppError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| AppError::validation("cursor", "The activity cursor is invalid."))?;
    let text = String::from_utf8(decoded)
        .map_err(|_| AppError::validation("cursor", "The activity cursor is invalid."))?;
    let mut parts = text.splitn(3, '\n');
    let effective_at = parts
        .next()
        .ok_or_else(|| AppError::validation("cursor", "The activity cursor is invalid."))?;
    let created_at = parts
        .next()
        .ok_or_else(|| AppError::validation("cursor", "The activity cursor is invalid."))?;
    let id = parts
        .next()
        .ok_or_else(|| AppError::validation("cursor", "The activity cursor is invalid."))?;
    Ok(ActivityListCursor {
        effective_at: effective_at.to_owned(),
        created_at: created_at.to_owned(),
        id: id.to_owned(),
    })
}

fn timeline_sort_key(item: &AccountTimelineItemDto) -> (String, String, String) {
    match item {
        AccountTimelineItemDto::Origin {
            occurred_at,
            created_at,
            id,
            ..
        } => (occurred_at.clone(), created_at.clone(), id.clone()),
        AccountTimelineItemDto::Activity {
            occurred_at,
            created_at,
            activity,
        } => (occurred_at.clone(), created_at.clone(), activity.id.clone()),
        AccountTimelineItemDto::Observation {
            occurred_at,
            created_at,
            id,
            ..
        }
        | AccountTimelineItemDto::AccountState {
            occurred_at,
            created_at,
            id,
            ..
        } => (occurred_at.clone(), created_at.clone(), id.clone()),
    }
}

fn encode_timeline_cursor(item: &AccountTimelineItemDto) -> String {
    let (occurred_at, created_at, id) = timeline_sort_key(item);
    URL_SAFE_NO_PAD.encode(format!("{occurred_at}\n{created_at}\n{id}"))
}

fn decode_timeline_cursor(value: &str) -> Result<(String, String, String), AppError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| AppError::validation("cursor", "The timeline cursor is invalid."))?;
    let text = String::from_utf8(decoded)
        .map_err(|_| AppError::validation("cursor", "The timeline cursor is invalid."))?;
    let mut parts = text.splitn(3, '\n');
    Ok((
        parts
            .next()
            .ok_or_else(|| AppError::validation("cursor", "The timeline cursor is invalid."))?
            .to_owned(),
        parts
            .next()
            .ok_or_else(|| AppError::validation("cursor", "The timeline cursor is invalid."))?
            .to_owned(),
        parts
            .next()
            .ok_or_else(|| AppError::validation("cursor", "The timeline cursor is invalid."))?
            .to_owned(),
    ))
}

fn time_spec(fields: &PostingFields) -> Result<ActivityTimeSpec<'_>, AppError> {
    Ok(ActivityTimeSpec {
        local_date: &fields.local_date,
        local_time: &fields.local_time,
        ambiguous_offset: fields
            .ambiguous_offset
            .as_deref()
            .map(AmbiguousOffset::parse)
            .transpose()?,
    })
}

fn optional_time_spec<'a>(
    local_date: Option<&'a str>,
    local_time: Option<&'a str>,
    ambiguous_offset: Option<&str>,
) -> Result<Option<ActivityTimeSpec<'a>>, AppError> {
    match (local_date, local_time) {
        (None, None) => Ok(None),
        (Some(local_date), Some(local_time)) => Ok(Some(ActivityTimeSpec {
            local_date,
            local_time,
            ambiguous_offset: ambiguous_offset.map(AmbiguousOffset::parse).transpose()?,
        })),
        _ => Err(AppError::invalid_activity_time(
            "Local date and time must be supplied together.",
        )),
    }
}

pub(crate) async fn post_command_from_pending_payload(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    payload: PendingActivityPayload,
) -> Result<PostCommand, AppError> {
    payload.validate()?;
    match payload {
        PendingActivityPayload::Deposit { endpoint, amount } => {
            Ok(PostCommand::Deposit { endpoint, amount })
        }
        PendingActivityPayload::Withdrawal { endpoint, amount } => {
            Ok(PostCommand::Withdrawal { endpoint, amount })
        }
        PendingActivityPayload::Transfer {
            source,
            source_amount,
            destination,
            destination_amount,
            fee,
            fee_kind,
        } => {
            let fee = match (fee, fee_kind) {
                (None, None) => None,
                (Some(amount), Some(kind)) => Some((amount, kind)),
                _ => {
                    return Err(AppError::invalid_pending_activity(
                        "Transfer fee amount and kind must be supplied together.",
                    ))
                }
            };
            Ok(PostCommand::CashTransfer {
                source,
                destination,
                source_amount,
                destination_amount,
                fee,
            })
        }
        PendingActivityPayload::PositionTransfer {
            source,
            destination,
            quantity,
        } => Ok(PostCommand::PositionTransfer {
            source,
            destination,
            quantity,
        }),
        PendingActivityPayload::Buy {
            holding_id,
            instrument_id,
            quantity,
            unit_price,
            gross_amount,
            fee,
            confirm_zero_unit_price,
        } => Ok(PostCommand::Buy(
            pending_trade_spec(
                tx,
                household_id,
                PendingTradeInput {
                    holding_id,
                    instrument_id,
                    quantity,
                    unit_price,
                    gross_amount,
                    fee,
                    confirm_zero_unit_price,
                },
            )
            .await?,
        )),
        PendingActivityPayload::Sell {
            holding_id,
            instrument_id,
            quantity,
            unit_price,
            gross_amount,
            fee,
            confirm_zero_unit_price,
        } => Ok(PostCommand::Sell(
            pending_trade_spec(
                tx,
                household_id,
                PendingTradeInput {
                    holding_id,
                    instrument_id,
                    quantity,
                    unit_price,
                    gross_amount,
                    fee,
                    confirm_zero_unit_price,
                },
            )
            .await?,
        )),
        PendingActivityPayload::Income {
            endpoint,
            amount,
            income_kind,
            instrument_id,
        } => Ok(PostCommand::Income {
            endpoint,
            amount,
            kind: income_kind,
            instrument_id,
        }),
        PendingActivityPayload::Fee {
            endpoint,
            amount,
            fee_kind,
            instrument_id,
        } => Ok(PostCommand::Fee {
            endpoint,
            amount,
            kind: fee_kind,
            instrument_id,
        }),
        PendingActivityPayload::DebtDraw {
            liability_account_id,
            principal,
            cash,
            fx_rate: _,
        } => Ok(PostCommand::DebtDraw(DebtDrawSpec {
            liability_account_id,
            principal,
            cash: cash.map(|(endpoint, amount)| DebtCashLink { endpoint, amount }),
        })),
        PendingActivityPayload::DebtPayment {
            liability_account_id,
            principal,
            cash,
            fee,
            fx_rate: _,
        } => {
            let (fee, fee_kind) = match fee {
                None => (None, None),
                Some((amount, kind)) => (Some(amount), Some(kind)),
            };
            Ok(PostCommand::DebtPayment(DebtPaymentSpec {
                liability_account_id,
                principal,
                cash: DebtCashLink {
                    endpoint: cash.0,
                    amount: cash.1,
                },
                fee,
                fee_kind,
            }))
        }
    }
}

struct PendingTradeInput {
    holding_id: HoldingId,
    instrument_id: InstrumentId,
    quantity: Quantity,
    unit_price: UnitPrice,
    gross_amount: Money,
    fee: Option<Money>,
    confirm_zero_unit_price: bool,
}

async fn pending_trade_spec(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    input: PendingTradeInput,
) -> Result<TradeSpec, AppError> {
    let (_, account_id, actual_instrument_id) = history_repositories::load_holding_endpoint(
        tx,
        household_id,
        &input.holding_id.to_string(),
    )
    .await?;
    let actual_instrument_id = InstrumentId::parse(&actual_instrument_id)?;
    if actual_instrument_id != input.instrument_id {
        return Err(AppError::invalid_pending_activity(
            "The trade instrument does not match the holding.",
        ));
    }
    Ok(TradeSpec {
        account_id: AccountId::parse(&account_id)?,
        holding_id: input.holding_id,
        instrument_id: input.instrument_id,
        quantity: input.quantity,
        unit_price: input.unit_price,
        quote_currency: input.gross_amount.currency(),
        gross_amount: input.gross_amount,
        settlement_currency: input.gross_amount.currency(),
        fee: input.fee,
        confirm_zero_unit_price: input.confirm_zero_unit_price,
    })
}

async fn post_command_from_input(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    input: CreateActivityInput,
) -> Result<PostCommand, AppError> {
    match input {
        CreateActivityInput::OpeningAdjustment {
            account_id,
            component,
            amount,
            currency,
            holding_id,
            instrument_id,
            quantity,
            ..
        } => Ok(PostCommand::Opening(opening_from_input(
            account_id,
            component,
            amount,
            currency,
            holding_id,
            instrument_id,
            quantity,
        )?)),
        CreateActivityInput::BalanceAdjustment {
            account_id,
            amount,
            currency,
            ..
        } => Ok(PostCommand::BalanceAdjustment {
            account_id: AccountId::parse(&account_id)?,
            target: money(&amount, &currency)?,
        }),
        CreateActivityInput::PositionAdjustment {
            holding_id,
            quantity,
            ..
        } => Ok(PostCommand::PositionAdjustment {
            holding_id: HoldingId::parse(&holding_id)?,
            target: Quantity::parse(&quantity)?,
        }),
        CreateActivityInput::Deposit {
            account_id,
            component,
            amount,
            currency,
            ..
        } => Ok(PostCommand::Deposit {
            endpoint: monetary_endpoint(&account_id, &component)?,
            amount: money(&amount, &currency)?,
        }),
        CreateActivityInput::Withdrawal {
            account_id,
            component,
            amount,
            currency,
            ..
        } => Ok(PostCommand::Withdrawal {
            endpoint: monetary_endpoint(&account_id, &component)?,
            amount: money(&amount, &currency)?,
        }),
        CreateActivityInput::Transfer {
            source_account_id,
            source_component,
            source_amount,
            source_currency,
            destination_account_id,
            destination_component,
            destination_amount,
            destination_currency,
            source_holding_id,
            destination_holding_id,
            quantity,
            fee_amount,
            fee_kind,
            ..
        } => {
            if source_component == "holding_quantity" || destination_component == "holding_quantity"
            {
                let quantity = Quantity::parse(quantity.as_deref().ok_or_else(|| {
                    AppError::validation("quantity", "Position transfer quantity is required.")
                })?)?;
                let source_holding = source_holding_id.as_deref().ok_or_else(|| {
                    AppError::validation("sourceHoldingId", "Position transfer source is required.")
                })?;
                let destination_holding = destination_holding_id.as_deref().ok_or_else(|| {
                    AppError::validation(
                        "destinationHoldingId",
                        "Position transfer destination is required.",
                    )
                })?;
                let source = quantity_endpoint(tx, household_id, source_holding).await?;
                let destination = quantity_endpoint(tx, household_id, destination_holding).await?;
                Ok(PostCommand::PositionTransfer {
                    source,
                    destination,
                    quantity,
                })
            } else {
                let fee = match (fee_amount.as_deref(), fee_kind.as_deref()) {
                    (None, None) => None,
                    (Some(amount), Some(kind)) => {
                        Some((money(amount, &source_currency)?, FeeKind::parse(kind)?))
                    }
                    _ => {
                        return Err(AppError::validation(
                            "feeAmount",
                            "A transfer fee requires both an amount and a fee kind.",
                        ))
                    }
                };
                Ok(PostCommand::CashTransfer {
                    source: monetary_endpoint(&source_account_id, &source_component)?,
                    destination: monetary_endpoint(
                        &destination_account_id,
                        &destination_component,
                    )?,
                    source_amount: money(&source_amount, &source_currency)?,
                    destination_amount: money(&destination_amount, &destination_currency)?,
                    fee,
                })
            }
        }
        CreateActivityInput::Buy {
            holding_id,
            quantity,
            unit_price,
            gross_amount,
            settlement_currency,
            fee_amount,
            confirm_zero_unit_price,
            ..
        } => Ok(PostCommand::Buy(
            trade_spec(
                tx,
                household_id,
                &holding_id,
                &quantity,
                &unit_price,
                &gross_amount,
                &settlement_currency,
                fee_amount.as_deref(),
                confirm_zero_unit_price,
            )
            .await?,
        )),
        CreateActivityInput::Sell {
            holding_id,
            quantity,
            unit_price,
            gross_amount,
            settlement_currency,
            fee_amount,
            confirm_zero_unit_price,
            ..
        } => Ok(PostCommand::Sell(
            trade_spec(
                tx,
                household_id,
                &holding_id,
                &quantity,
                &unit_price,
                &gross_amount,
                &settlement_currency,
                fee_amount.as_deref(),
                confirm_zero_unit_price,
            )
            .await?,
        )),
        CreateActivityInput::Income {
            account_id,
            component,
            amount,
            currency,
            income_kind,
            instrument_id,
            ..
        } => Ok(PostCommand::Income {
            endpoint: monetary_endpoint(&account_id, &component)?,
            amount: money(&amount, &currency)?,
            kind: IncomeKind::parse(&income_kind)?,
            instrument_id: instrument_id
                .as_deref()
                .map(InstrumentId::parse)
                .transpose()?,
        }),
        CreateActivityInput::Fee {
            account_id,
            component,
            amount,
            currency,
            fee_kind,
            instrument_id,
            ..
        } => Ok(PostCommand::Fee {
            endpoint: monetary_endpoint(&account_id, &component)?,
            amount: money(&amount, &currency)?,
            kind: FeeKind::parse(&fee_kind)?,
            instrument_id: instrument_id
                .as_deref()
                .map(InstrumentId::parse)
                .transpose()?,
        }),
        CreateActivityInput::DebtDraw {
            liability_account_id,
            principal_amount,
            principal_currency,
            cash_account_id,
            cash_component,
            cash_amount,
            cash_currency,
            fx_rate,
            ..
        } => Ok(PostCommand::DebtDraw(DebtDrawSpec {
            liability_account_id: AccountId::parse(&liability_account_id)?,
            principal: money(&principal_amount, &principal_currency)?,
            cash: debt_cash(
                cash_account_id,
                cash_component,
                cash_amount,
                cash_currency,
                fx_rate,
            )?,
        })),
        CreateActivityInput::DebtPayment {
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
            ..
        } => {
            let cash = debt_cash(
                Some(cash_account_id),
                Some(cash_component),
                Some(cash_amount),
                Some(cash_currency),
                fx_rate,
            )?
            .ok_or_else(|| AppError::invalid_activity("Debt payments require a cash source."))?;
            Ok(PostCommand::DebtPayment(DebtPaymentSpec {
                liability_account_id: AccountId::parse(&liability_account_id)?,
                principal: money(&principal_amount, &principal_currency)?,
                cash,
                fee: fee_amount
                    .as_deref()
                    .map(|amount| money(amount, &principal_currency))
                    .transpose()?,
                fee_kind: fee_kind.as_deref().map(FeeKind::parse).transpose()?,
            }))
        }
        CreateActivityInput::DebtAdjustment {
            account_id,
            amount,
            currency,
            ..
        } => Ok(PostCommand::DebtAdjustment {
            account_id: AccountId::parse(&account_id)?,
            target: money(&amount, &currency)?,
        }),
        CreateActivityInput::ManualValuation {
            account_id,
            amount,
            currency,
            ..
        } => Ok(PostCommand::ManualValuation {
            account_id: AccountId::parse(&account_id)?,
            target: money(&amount, &currency)?,
        }),
    }
}

fn opening_from_input(
    account_id: String,
    component: String,
    amount: Option<String>,
    currency: Option<String>,
    holding_id: Option<String>,
    instrument_id: Option<String>,
    quantity: Option<String>,
) -> Result<ComponentOpening, AppError> {
    let account_id = AccountId::parse(&account_id)?;
    match component.as_str() {
        "account_value" => Ok(ComponentOpening::AccountValue {
            account_id,
            amount: money(
                amount
                    .as_deref()
                    .ok_or_else(|| AppError::validation("amount", "Opening amount is required."))?,
                currency.as_deref().ok_or_else(|| {
                    AppError::validation("currency", "Opening currency is required.")
                })?,
            )?,
        }),
        "holdings_cash" => Ok(ComponentOpening::HoldingsCash {
            account_id,
            amount: money(
                amount
                    .as_deref()
                    .ok_or_else(|| AppError::validation("amount", "Opening amount is required."))?,
                currency.as_deref().ok_or_else(|| {
                    AppError::validation("currency", "Opening currency is required.")
                })?,
            )?,
        }),
        "holding_quantity" => Ok(ComponentOpening::HoldingQuantity {
            account_id,
            holding_id: HoldingId::parse(holding_id.as_deref().ok_or_else(|| {
                AppError::validation("holdingId", "Opening holding is required.")
            })?)?,
            instrument_id: InstrumentId::parse(instrument_id.as_deref().ok_or_else(|| {
                AppError::validation("instrumentId", "Opening instrument is required.")
            })?)?,
            quantity: Quantity::parse(quantity.as_deref().ok_or_else(|| {
                AppError::validation("quantity", "Opening quantity is required.")
            })?)?,
        }),
        _ => Err(AppError::validation(
            "component",
            "Opening component is not supported.",
        )),
    }
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

fn money(amount: &str, currency: &str) -> Result<Money, AppError> {
    Money::parse(amount, CurrencyCode::parse(currency)?)
}

fn debt_cash(
    account_id: Option<String>,
    component: Option<String>,
    amount: Option<String>,
    currency: Option<String>,
    _fx_rate: Option<String>,
) -> Result<Option<DebtCashLink>, AppError> {
    match (account_id, component, amount, currency) {
        (None, None, None, None) => Ok(None),
        (Some(account_id), Some(component), Some(amount), Some(currency)) => {
            Ok(Some(DebtCashLink {
                endpoint: monetary_endpoint(&account_id, &component)?,
                amount: money(&amount, &currency)?,
            }))
        }
        _ => Err(AppError::invalid_activity(
            "Debt cash endpoint fields must be supplied together.",
        )),
    }
}

#[allow(clippy::too_many_arguments)]
async fn trade_spec(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    holding_id: &str,
    quantity: &str,
    unit_price: &str,
    gross_amount: &str,
    settlement_currency: &str,
    fee_amount: Option<&str>,
    confirm_zero_unit_price: bool,
) -> Result<TradeSpec, AppError> {
    let (_, account_id, instrument_id) =
        history_repositories::load_holding_endpoint(tx, household_id, holding_id).await?;
    let currency = CurrencyCode::parse(settlement_currency)?;
    Ok(TradeSpec {
        account_id: AccountId::parse(&account_id)?,
        holding_id: HoldingId::parse(holding_id)?,
        instrument_id: InstrumentId::parse(&instrument_id)?,
        quantity: Quantity::parse(quantity)?,
        unit_price: UnitPrice::parse(unit_price)?,
        quote_currency: currency,
        gross_amount: Money::parse(gross_amount, currency)?,
        settlement_currency: currency,
        fee: fee_amount
            .map(|amount| Money::parse(amount, currency))
            .transpose()?,
        confirm_zero_unit_price,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            account_service::{self, CreateAccountInput, OwnershipShareInput},
            member_service, query_count,
        },
        domain::{closed_day_cutoff, CalendarDate, HistoryTimezone},
        error::{AppError, CommandError, ErrorCode},
        test_support::{
            assert_activity_history_commands_write_nothing, blocked_future_state, cleanup,
            onboarded_state, stable_sqlite_hash, UNKNOWN_UUID,
        },
    };

    fn owner(member_id: &str) -> OwnershipShareInput {
        OwnershipShareInput {
            member_id: member_id.to_owned(),
            percent: Some("100".to_owned()),
            share_bps: None,
        }
    }

    fn bank_input(name: &str, member_id: &str, amount: &str) -> CreateAccountInput {
        CreateAccountInput {
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
            owners: vec![owner(member_id)],
            initial_amount: Some(amount.to_owned()),
        }
    }

    fn deposit(
        local_date: &str,
        account_id: &str,
        amount: &str,
        note: Option<&str>,
    ) -> CreateActivityInput {
        CreateActivityInput::Deposit {
            local_date: local_date.to_owned(),
            local_time: "00:01".to_owned(),
            ambiguous_offset: None,
            note: note.map(ToOwned::to_owned),
            account_id: account_id.to_owned(),
            component: "account_value".to_owned(),
            amount: amount.to_owned(),
            currency: "CNY".to_owned(),
        }
    }

    async fn confirm_tz(state: &crate::state::AppState) -> HistoryOriginDto {
        let origin = get_history_origin(state).await.expect("origin");
        if origin.timezone_confirmed {
            return origin;
        }
        confirm_history_timezone(
            state,
            ConfirmHistoryTimezoneInput {
                timezone: origin.timezone.clone(),
            },
        )
        .await
        .expect("confirm timezone")
    }

    async fn origin_ready(state: &crate::state::AppState) -> HistoryOriginDto {
        let origin = confirm_tz(state).await;
        let timezone = HistoryTimezone::parse(&origin.timezone).expect("tz");
        let date = CalendarDate::parse(&origin.origin_local_date).expect("date");
        if let Some(previous) = date.pred() {
            let origin_at = closed_day_cutoff(timezone, previous).expect("start of day");
            sqlx::query("UPDATE history_origins SET origin_at = ?")
                .bind(origin_at.to_rfc3339())
                .execute(state.writable_db().expect("db"))
                .await
                .expect("nudge origin");
        }
        get_history_origin(state).await.expect("origin")
    }

    async fn member_id(state: &crate::state::AppState) -> String {
        member_service::list_members(state, false)
            .await
            .expect("members")[0]
            .id
            .clone()
    }

    async fn count(state: &crate::state::AppState, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(state.writable_db().expect("writable"))
            .await
            .expect("count")
    }

    async fn text(state: &crate::state::AppState, sql: &str) -> String {
        sqlx::query_scalar(sql)
            .fetch_one(state.writable_db().expect("writable"))
            .await
            .expect("text")
    }

    fn family_count(families: &[&str], name: &str) -> usize {
        families.iter().filter(|family| **family == name).count()
    }

    #[test]
    fn create_activity_input_is_kind_tagged() {
        let value = serde_json::json!({
            "kind": "deposit",
            "localDate": "2026-01-02",
            "localTime": "12:00:00",
            "accountId": "acct",
            "component": "account_value",
            "amount": "10",
            "currency": "CNY"
        });
        let parsed: CreateActivityInput =
            serde_json::from_value(value).expect("tagged deposit input");
        match parsed {
            CreateActivityInput::Deposit { amount, .. } => assert_eq!(amount, "10"),
            other => panic!("expected deposit, got {other:?}"),
        }
        let encoded = serde_json::to_value(CreateActivityInput::Fee {
            local_date: "2026-01-02".to_owned(),
            local_time: "12:00".to_owned(),
            ambiguous_offset: None,
            note: None,
            account_id: "acct".to_owned(),
            component: "account_value".to_owned(),
            amount: "1".to_owned(),
            currency: "CNY".to_owned(),
            fee_kind: "account_fee".to_owned(),
            instrument_id: None,
        })
        .expect("encode");
        assert_eq!(encoded["kind"], "fee");
        assert!(encoded.get("legs").is_none());
        assert!(encoded.get("classification").is_none());
    }

    #[test]
    fn snapshot_rebuild_errors_do_not_leak_sensitive_details() {
        for error in [
            AppError::SnapshotRebuildRequired,
            AppError::SnapshotRebuildFailed,
        ] {
            let command = CommandError::from(error);
            let payload = serde_json::to_string(&command).expect("json");
            assert!(
                !payload.to_lowercase().contains("sql")
                    && !payload.contains("SELECT")
                    && !payload.contains('/')
                    && !payload.contains("note")
                    && !payload.contains("CNY")
                    && !payload.contains("QQQ"),
                "{payload}"
            );
            assert!(matches!(
                command.code,
                ErrorCode::SnapshotRebuildRequired | ErrorCode::SnapshotRebuildFailed
            ));
        }
    }

    #[test]
    fn preview_validates_without_writes_and_create_revalidates() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("phase6-preview").await;
            let origin = origin_ready(&state).await;
            let walt = member_id(&state).await;
            let account = account_service::create_account(&state, bank_input("Bank", &walt, "100"))
                .await
                .expect("account");
            let before = count(&state, "SELECT COUNT(*) FROM activities").await;
            let preview = preview_activity(
                &state,
                deposit(
                    &origin.origin_local_date,
                    &account.id,
                    "25",
                    Some("preview"),
                ),
            )
            .await
            .expect("preview");
            assert_eq!(preview.activity.kind, "deposit");
            assert_eq!(preview.activity.classification, "external_inflow");
            assert!(
                !preview.activity.legs.is_empty(),
                "preview should include legs"
            );
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                before,
                "preview must not write"
            );
            let posted = create_activity(
                &state,
                deposit(&origin.origin_local_date, &account.id, "25", Some("posted")),
            )
            .await
            .expect("create");
            assert_eq!(posted.kind, "deposit");
            assert_eq!(posted.note.as_deref(), Some("posted"));
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                before + 1
            );
            cleanup(&path);
        });
    }

    #[test]
    fn activity_list_cursor_has_no_gaps_or_duplicates() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("phase6-cursor").await;
            let origin = origin_ready(&state).await;
            let walt = member_id(&state).await;
            let account =
                account_service::create_account(&state, bank_input("Bank", &walt, "1000"))
                    .await
                    .expect("account");
            for amount in ["1", "2", "3"] {
                create_activity(
                    &state,
                    deposit(&origin.origin_local_date, &account.id, amount, None),
                )
                .await
                .expect("deposit");
            }
            let full = list_activities(
                &state,
                ListActivitiesInput {
                    cursor: None,
                    limit: Some(100),
                    start_local_date: None,
                    end_local_date: None,
                    account_id: Some(account.id.clone()),
                    instrument_id: None,
                    kind: Some("deposit".to_owned()),
                    classification: Some("external_inflow".to_owned()),
                },
            )
            .await
            .expect("full list");
            assert!(full.items.len() >= 3);
            let mut paged = Vec::new();
            let mut cursor = None;
            loop {
                let page = list_activities(
                    &state,
                    ListActivitiesInput {
                        cursor,
                        limit: Some(1),
                        start_local_date: Some(origin.origin_local_date.clone()),
                        end_local_date: Some(origin.origin_local_date.clone()),
                        account_id: Some(account.id.clone()),
                        instrument_id: None,
                        kind: Some("deposit".to_owned()),
                        classification: None,
                    },
                )
                .await
                .expect("page");
                paged.extend(page.items.into_iter().map(|item| item.id));
                if !page.has_more {
                    break;
                }
                cursor = page.next_cursor;
            }
            let full_ids: Vec<String> = full
                .items
                .into_iter()
                .filter(|item| item.kind == "deposit")
                .map(|item| item.id)
                .collect();
            assert_eq!(paged, full_ids);
            let mut unique = paged.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), paged.len());
            cleanup(&path);
        });
    }

    #[test]
    fn activity_filters_enforce_household_and_page_size() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("phase6-filters").await;
            confirm_tz(&state).await;
            let missing_account = list_activities(
                &state,
                ListActivitiesInput {
                    cursor: None,
                    limit: Some(10),
                    start_local_date: None,
                    end_local_date: None,
                    account_id: Some(UNKNOWN_UUID.to_owned()),
                    instrument_id: None,
                    kind: None,
                    classification: None,
                },
            )
            .await
            .expect_err("foreign account");
            assert!(matches!(missing_account, AppError::NotFound { .. }));
            let missing_instrument = list_activities(
                &state,
                ListActivitiesInput {
                    cursor: None,
                    limit: Some(10),
                    start_local_date: None,
                    end_local_date: None,
                    account_id: None,
                    instrument_id: Some(UNKNOWN_UUID.to_owned()),
                    kind: None,
                    classification: None,
                },
            )
            .await
            .expect_err("foreign instrument");
            assert!(matches!(missing_instrument, AppError::NotFound { .. }));
            let oversized = list_activities(
                &state,
                ListActivitiesInput {
                    cursor: None,
                    limit: Some(101),
                    start_local_date: None,
                    end_local_date: None,
                    account_id: None,
                    instrument_id: None,
                    kind: None,
                    classification: None,
                },
            )
            .await
            .expect_err("page size");
            assert!(matches!(oversized, AppError::Validation { .. }));
            cleanup(&path);
        });
    }

    #[test]
    fn list_detail_and_timeline_query_counts_are_bounded() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("phase6-qcount").await;
            let origin = origin_ready(&state).await;
            let walt = member_id(&state).await;
            let first = account_service::create_account(&state, bank_input("One", &walt, "100"))
                .await
                .expect("first");
            let second = account_service::create_account(&state, bank_input("Two", &walt, "200"))
                .await
                .expect("second");
            create_activity(
                &state,
                deposit(&origin.origin_local_date, &first.id, "5", None),
            )
            .await
            .expect("d1");
            create_activity(
                &state,
                deposit(&origin.origin_local_date, &second.id, "7", None),
            )
            .await
            .expect("d2");

            // list: 1 header query + 1 batched legs query regardless of activity count
            let (page, list_families) = query_count::capture_async(|| {
                list_activities(
                    &state,
                    ListActivitiesInput {
                        cursor: None,
                        limit: Some(50),
                        start_local_date: None,
                        end_local_date: None,
                        account_id: None,
                        instrument_id: None,
                        kind: None,
                        classification: None,
                    },
                )
            })
            .await;
            let page = page.expect("list");
            assert!(page.items.len() >= 2);
            assert_eq!(
                family_count(&list_families, "activity_headers"),
                1,
                "list headers {list_families:?}"
            );
            assert_eq!(
                family_count(&list_families, "activity_legs"),
                1,
                "list legs must be batched {list_families:?}"
            );
            assert_eq!(family_count(&list_families, "activity_correction_links"), 1);

            let activity_id = page.items[0].id.clone();
            let (detail, detail_families) =
                query_count::capture_async(|| get_activity(&state, &activity_id)).await;
            let detail = detail.expect("detail");
            assert!(!detail.legs.is_empty());
            assert_eq!(
                family_count(&detail_families, "activity_header"),
                1,
                "detail header {detail_families:?}"
            );
            assert_eq!(
                family_count(&detail_families, "activity_legs"),
                1,
                "detail legs {detail_families:?}"
            );

            let (timeline, timeline_families) = query_count::capture_async(|| {
                get_account_timeline(
                    &state,
                    GetAccountTimelineInput {
                        account_id: first.id.clone(),
                        cursor: None,
                        limit: Some(50),
                    },
                )
            })
            .await;
            let timeline = timeline.expect("timeline");
            assert!(timeline
                .items
                .iter()
                .any(|item| matches!(item, AccountTimelineItemDto::Origin { .. })));
            assert!(timeline
                .items
                .iter()
                .any(|item| matches!(item, AccountTimelineItemDto::Activity { .. })));
            assert_eq!(
                family_count(&timeline_families, "timeline_activity_headers"),
                1
            );
            assert_eq!(
                family_count(&timeline_families, "activity_legs"),
                1,
                "timeline legs must be batched {timeline_families:?}"
            );
            assert_eq!(family_count(&timeline_families, "timeline_observations"), 1);
            assert_eq!(family_count(&timeline_families, "timeline_states"), 1);
            cleanup(&path);
        });
    }

    #[test]
    fn activity_and_account_state_mutations_set_dirty_date() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("phase6-dirty-activity").await;
            let origin = origin_ready(&state).await;
            let walt = member_id(&state).await;
            let account = account_service::create_account(&state, bank_input("Bank", &walt, "500"))
                .await
                .expect("account");
            sqlx::query("UPDATE history_snapshot_state SET dirty_from = NULL")
                .execute(state.writable_db().expect("db"))
                .await
                .expect("clear dirty");
            let posted = create_activity(
                &state,
                deposit(&origin.origin_local_date, &account.id, "9", None),
            )
            .await
            .expect("deposit");
            assert_eq!(
                text(&state, "SELECT dirty_from FROM history_snapshot_state").await,
                origin.origin_local_date
            );
            reverse_activity(
                &state,
                ReverseActivityInput {
                    id: posted.id,
                    local_date: Some(origin.origin_local_date.clone()),
                    local_time: Some("00:02".to_owned()),
                    ambiguous_offset: None,
                },
            )
            .await
            .expect("reverse");
            sqlx::query("UPDATE history_snapshot_state SET dirty_from = NULL")
                .execute(state.writable_db().expect("db"))
                .await
                .expect("clear dirty");
            account_service::archive_account(&state, &account.id)
                .await
                .expect("archive");
            assert_eq!(
                text(&state, "SELECT dirty_from FROM history_snapshot_state").await,
                origin.origin_local_date
            );
            cleanup(&path);
        });
    }

    #[test]
    fn blocked_future_database_rejects_activity_and_history_commands() {
        tauri::async_runtime::block_on(async {
            let (state, path, before_hash) = blocked_future_state("activity-history").await;
            assert_activity_history_commands_write_nothing(&state, &path, before_hash).await;
            assert_eq!(stable_sqlite_hash(&path).await, before_hash);
            cleanup(&path);
        });
    }
}
