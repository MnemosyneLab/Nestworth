//! Activity posting and current-state projection.
//!
//! Public v0.1.2 mutations route through this service after History Origin exists.
//! Transfer, Buy/Sell, Debt Draw/Payment, reverse_activity, and correct_activity
//! post through the same BEGIN IMMEDIATE apply-and-persist path.

use std::collections::HashMap;

use sqlx::{Row, Sqlite, Transaction};

use super::{
    history_origin::ensure_activity_writes_allowed,
    history_repositories::{
        get_activity, get_activity_by_corrects, get_activity_by_reverses, get_origin_by_household,
        insert_activity, insert_holding_quantity, list_all_activities_asc,
        list_origin_account_values, list_origin_cash_values, list_origin_holdings,
        mark_snapshots_dirty_from, HistoryOriginRecord, HoldingQuantityRecord,
    },
    instrument_service,
    reference::{
        begin_read_tx, begin_write_tx, finish_read_tx, finish_write_tx, map_read_error,
        map_write_error,
    },
};
use crate::{
    domain::{
        resolve_activity_time, validate_activity_time, AccountCashValue, AccountCashValueId,
        AccountId, AccountValue, AccountValueId, Activity, ActivityId, ActivityKind, ActivityLeg,
        ActivityRecordParams, AmbiguousOffset, ComponentOpening, ConstructActivity, CurrencyCode,
        DebtDrawSpec, DebtPaymentSpec, FeeKind, FxRate, HistoryTimezone, HoldingId,
        HoldingQuantityValueId, HouseholdId, IncomeKind, InstrumentId, LegComponent,
        MonetaryComponent, MonetaryEndpoint, Money, PersistedAccountValue, PrimaryCategory,
        Quantity, QuantityEndpoint, Timestamp, TrackingMode, TradeSpec, ValueKind,
    },
    error::AppError,
    state::AppState,
};

pub struct ActivityTimeSpec<'a> {
    pub local_date: &'a str,
    pub local_time: &'a str,
    pub ambiguous_offset: Option<AmbiguousOffset>,
}

pub enum PostCommand {
    Opening(ComponentOpening),
    BalanceAdjustment {
        account_id: AccountId,
        target: Money,
    },
    PositionAdjustment {
        holding_id: HoldingId,
        target: Quantity,
    },
    Deposit {
        endpoint: MonetaryEndpoint,
        amount: Money,
    },
    Withdrawal {
        endpoint: MonetaryEndpoint,
        amount: Money,
    },
    Income {
        endpoint: MonetaryEndpoint,
        amount: Money,
        kind: IncomeKind,
        instrument_id: Option<InstrumentId>,
    },
    Fee {
        endpoint: MonetaryEndpoint,
        amount: Money,
        kind: FeeKind,
        instrument_id: Option<InstrumentId>,
    },
    DebtAdjustment {
        account_id: AccountId,
        target: Money,
    },
    ManualValuation {
        account_id: AccountId,
        target: Money,
    },
    CashTransfer {
        source: MonetaryEndpoint,
        destination: MonetaryEndpoint,
        source_amount: Money,
        destination_amount: Money,
        fx_rate: Option<FxRate>,
    },
    PositionTransfer {
        source: QuantityEndpoint,
        destination: QuantityEndpoint,
        quantity: Quantity,
    },
    Buy(TradeSpec),
    Sell(TradeSpec),
    DebtDraw(DebtDrawSpec),
    DebtPayment(DebtPaymentSpec),
}

impl PostCommand {
    pub fn cash_transfer(
        source: MonetaryEndpoint,
        destination: MonetaryEndpoint,
        source_amount: Money,
        destination_amount: Money,
        fx_rate: Option<FxRate>,
    ) -> Self {
        Self::CashTransfer {
            source,
            destination,
            source_amount,
            destination_amount,
            fx_rate,
        }
    }

    pub fn position_transfer(
        source: QuantityEndpoint,
        destination: QuantityEndpoint,
        quantity: Quantity,
    ) -> Self {
        Self::PositionTransfer {
            source,
            destination,
            quantity,
        }
    }

    pub fn buy(spec: TradeSpec) -> Self {
        Self::Buy(spec)
    }

    pub fn sell(spec: TradeSpec) -> Self {
        Self::Sell(spec)
    }

    pub fn debt_draw(spec: DebtDrawSpec) -> Self {
        Self::DebtDraw(spec)
    }

    pub fn debt_payment(spec: DebtPaymentSpec) -> Self {
        Self::DebtPayment(spec)
    }
}

#[derive(Debug, Clone)]
pub struct PreviewEndpointChange {
    pub account_id: String,
    pub component_kind: String,
    pub holding_id: Option<String>,
    pub currency: Option<String>,
    pub before_amount: Option<String>,
    pub after_amount: Option<String>,
}

pub struct ActivityPreview {
    pub activity: Activity,
    pub endpoints: Vec<PreviewEndpointChange>,
}

#[derive(Debug, Clone)]
pub struct PostedCorrection {
    pub reversal: Activity,
    pub replacement: Activity,
}

struct AccountRow {
    tracking_mode: TrackingMode,
    primary_category: PrimaryCategory,
    archived: bool,
}

struct HoldingRow {
    id: HoldingId,
    account_id: AccountId,
    instrument_id: InstrumentId,
    archived: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum EndpointKey {
    AccountValue {
        account_id: String,
        currency: String,
    },
    HoldingsCash {
        account_id: String,
        currency: String,
    },
    HoldingQuantity {
        holding_id: String,
    },
}

#[derive(Clone)]
enum EndpointState {
    Money {
        account_id: AccountId,
        component: MonetaryComponent,
        resulting: Money,
    },
    Quantity {
        holding_id: HoldingId,
        resulting: Quantity,
    },
}

pub async fn mark_dirty_at(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    at: &Timestamp,
) -> Result<(), AppError> {
    let timezone = HistoryTimezone::parse(&origin.timezone)?;
    let local_date = timezone.local_date(at).to_ymd();
    let dirty_from = if local_date.as_str() < origin.origin_local_date.as_str() {
        origin.origin_local_date.clone()
    } else {
        local_date
    };
    mark_snapshots_dirty_from(tx, &origin.household_id, &dirty_from, &at.to_rfc3339()).await
}

pub async fn mark_dirty_for_household(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    at: &Timestamp,
) -> Result<(), AppError> {
    let Some(origin) = get_origin_by_household(tx, household_id).await? else {
        return Ok(());
    };
    mark_dirty_at(tx, &origin, at).await
}

pub async fn post(
    state: &AppState,
    command: PostCommand,
    time: Option<ActivityTimeSpec<'_>>,
) -> Result<Activity, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = post_in_tx(&mut tx, command, time, None).await;
    finish_write_tx(tx, result).await
}

pub async fn post_user(
    state: &AppState,
    command: PostCommand,
    time: Option<ActivityTimeSpec<'_>>,
    note: Option<&str>,
) -> Result<Activity, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = post_in_tx(&mut tx, command, time, note).await;
    finish_write_tx(tx, result).await
}

pub async fn preview(
    state: &AppState,
    command: PostCommand,
    time: Option<ActivityTimeSpec<'_>>,
    note: Option<&str>,
) -> Result<ActivityPreview, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = preview_in_tx(&mut tx, command, time, note).await;
    finish_read_tx(tx, result).await
}

pub async fn reverse_activity(
    state: &AppState,
    original_id: &str,
    time: Option<ActivityTimeSpec<'_>>,
) -> Result<Activity, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = reverse_activity_in_tx(&mut tx, original_id, time).await;
    finish_write_tx(tx, result).await
}

pub async fn correct_activity(
    state: &AppState,
    original_id: &str,
    replacement: PostCommand,
    replacement_time: Option<ActivityTimeSpec<'_>>,
) -> Result<PostedCorrection, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = correct_activity_in_tx(&mut tx, original_id, replacement, replacement_time).await;
    finish_write_tx(tx, result).await
}

pub async fn post_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    command: PostCommand,
    time: Option<ActivityTimeSpec<'_>>,
    note: Option<&str>,
) -> Result<Activity, AppError> {
    let origin = ensure_activity_writes_allowed(tx).await?;
    let (params, _) = resolve_record_params(&origin, time, note)?;
    let activity = construct_posted_activity(tx, &origin, &params, command).await?;
    apply_and_persist(tx, &origin, &activity).await?;
    tracing::info!(
        event = "activity.post",
        kind = activity.kind().as_str(),
        "activity posted"
    );
    Ok(activity)
}

pub async fn preview_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    command: PostCommand,
    time: Option<ActivityTimeSpec<'_>>,
    note: Option<&str>,
) -> Result<ActivityPreview, AppError> {
    let origin = ensure_activity_writes_allowed(tx).await?;
    let (params, _) = resolve_record_params(&origin, time, note)?;
    let activity = construct_posted_activity(tx, &origin, &params, command).await?;
    let before = load_endpoint_states(tx, &activity).await?;
    let mut after = before.clone();
    for leg in activity.legs() {
        let key = endpoint_key(leg)?;
        apply_leg_to_state(after.get_mut(&key).ok_or(AppError::Internal)?, leg)?;
    }
    let holding_accounts: HashMap<String, String> = activity
        .legs()
        .iter()
        .filter_map(|leg| match leg.component() {
            LegComponent::HoldingQuantity { holding_id, .. } => {
                Some((holding_id.to_string(), leg.account_id().to_string()))
            }
            _ => None,
        })
        .collect();
    let endpoints = before
        .iter()
        .filter_map(|(key, before_state)| {
            after
                .get(key)
                .map(|after_state| preview_change(before_state, after_state, &holding_accounts))
        })
        .collect();
    tracing::info!(
        event = "activity.preview",
        kind = activity.kind().as_str(),
        "activity previewed"
    );
    Ok(ActivityPreview {
        activity,
        endpoints,
    })
}

fn preview_change(
    before: &EndpointState,
    after: &EndpointState,
    holding_accounts: &HashMap<String, String>,
) -> PreviewEndpointChange {
    match (before, after) {
        (
            EndpointState::Money {
                account_id,
                component,
                resulting: before_amount,
            },
            EndpointState::Money {
                resulting: after_amount,
                ..
            },
        ) => PreviewEndpointChange {
            account_id: account_id.to_string(),
            component_kind: component.kind().as_str().to_owned(),
            holding_id: None,
            currency: Some(before_amount.currency().as_str().to_owned()),
            before_amount: Some(before_amount.canonical_amount()),
            after_amount: Some(after_amount.canonical_amount()),
        },
        (
            EndpointState::Quantity {
                holding_id,
                resulting: before_qty,
            },
            EndpointState::Quantity {
                resulting: after_qty,
                ..
            },
        ) => PreviewEndpointChange {
            account_id: holding_accounts
                .get(&holding_id.to_string())
                .cloned()
                .unwrap_or_default(),
            component_kind: "holding_quantity".to_owned(),
            holding_id: Some(holding_id.to_string()),
            currency: None,
            before_amount: Some(before_qty.canonical()),
            after_amount: Some(after_qty.canonical()),
        },
        _ => PreviewEndpointChange {
            account_id: String::new(),
            component_kind: "unknown".to_owned(),
            holding_id: None,
            currency: None,
            before_amount: None,
            after_amount: None,
        },
    }
}

pub async fn reverse_activity_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    original_id: &str,
    time: Option<ActivityTimeSpec<'_>>,
) -> Result<Activity, AppError> {
    let origin = ensure_activity_writes_allowed(tx).await?;
    let original = load_current_chain_activity(tx, original_id, ChainMutation::Reverse).await?;
    if original.household_id().to_string() != origin.household_id {
        return Err(AppError::not_found("activity", original_id));
    }
    let (params, _) = resolve_record_params(&origin, time, None)?;
    if params.effective_at < *original.effective_at() {
        return Err(AppError::invalid_activity_time(
            "A reversal cannot be dated before the original activity.",
        ));
    }
    let reversal = Activity::reversal(&params, &original)?;
    persist_ledger_activities(tx, &origin, std::slice::from_ref(&reversal)).await?;
    tracing::info!(
        event = "activity.reverse",
        kind = "reversal",
        "activity reversed"
    );
    Ok(reversal)
}

pub async fn correct_activity_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    original_id: &str,
    replacement: PostCommand,
    replacement_time: Option<ActivityTimeSpec<'_>>,
) -> Result<PostedCorrection, AppError> {
    let origin = ensure_activity_writes_allowed(tx).await?;
    let original = load_current_chain_activity(tx, original_id, ChainMutation::Correct).await?;
    if original.household_id().to_string() != origin.household_id {
        return Err(AppError::not_found("activity", original_id));
    }
    let group = uuid::Uuid::now_v7();
    let reversal_params = ActivityRecordParams {
        household_id: original.household_id(),
        effective_at: original.effective_at().clone(),
        effective_local_date: original.effective_local_date(),
        created_at: Timestamp::now(),
        note: None,
    };
    let reversal = Activity::reversal(&reversal_params, &original)?.with_correction_group(group);
    let (replacement_params, _) = resolve_record_params(&origin, replacement_time, None)?;
    let replacement = construct_posted_activity(tx, &origin, &replacement_params, replacement)
        .await?
        .with_corrects(original.id(), group);
    persist_ledger_activities(tx, &origin, &[reversal.clone(), replacement.clone()]).await?;
    tracing::info!(
        event = "activity.correct",
        kind = replacement.kind().as_str(),
        "activity corrected"
    );
    Ok(PostedCorrection {
        reversal,
        replacement,
    })
}

fn resolve_record_params<'a>(
    origin: &HistoryOriginRecord,
    time: Option<ActivityTimeSpec<'_>>,
    note: Option<&'a str>,
) -> Result<(ActivityRecordParams<'a>, HistoryTimezone), AppError> {
    let household_id = HouseholdId::parse(&origin.household_id)?;
    let timezone = HistoryTimezone::parse(&origin.timezone)?;
    let origin_at = Timestamp::parse(&origin.origin_at)?;
    let now = Timestamp::now();
    let (effective_at, effective_local_date) = match time {
        Some(spec) => resolve_activity_time(
            timezone,
            spec.local_date,
            spec.local_time,
            spec.ambiguous_offset,
            &origin_at,
            &now,
        )?,
        None => {
            validate_activity_time(&now, &origin_at, &now)?;
            (now.clone(), timezone.local_date(&now))
        }
    };
    Ok((
        ActivityRecordParams {
            household_id,
            effective_at,
            effective_local_date,
            created_at: now,
            note,
        },
        timezone,
    ))
}

async fn construct_posted_activity(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    params: &ActivityRecordParams<'_>,
    command: PostCommand,
) -> Result<Activity, AppError> {
    match command {
        PostCommand::Opening(opening) => {
            validate_opening(tx, origin, &opening).await?;
            match Activity::opening_adjustment(params, opening)? {
                ConstructActivity::Posted(activity) => Ok(activity),
                ConstructActivity::NoActivity => Err(AppError::invalid_activity(
                    "Zero-state creation must not post an opening adjustment.",
                )),
            }
        }
        PostCommand::BalanceAdjustment { account_id, target } => {
            let account = load_required_account(tx, origin, &account_id).await?;
            require_active(&account)?;
            if account.tracking_mode != TrackingMode::Balance
                || account.primary_category == PrimaryCategory::Liability
            {
                return Err(AppError::invalid_activity(
                    "Balance adjustments require a non-liability balance account.",
                ));
            }
            let current =
                latest_account_value_money(tx, &account_id.to_string(), target.currency()).await?;
            let endpoint = MonetaryEndpoint {
                account_id,
                component: MonetaryComponent::AccountValue,
            };
            Activity::balance_adjustment(params, endpoint, current, target)
        }
        PostCommand::ManualValuation { account_id, target } => {
            let account = load_required_account(tx, origin, &account_id).await?;
            require_active(&account)?;
            if account.tracking_mode != TrackingMode::ManualValue {
                return Err(AppError::invalid_activity(
                    "Manual valuation requires a manual value account.",
                ));
            }
            let current =
                latest_account_value_money(tx, &account_id.to_string(), target.currency()).await?;
            Activity::manual_valuation(params, account_id, current, target)
        }
        PostCommand::DebtAdjustment { account_id, target } => {
            let account = load_required_account(tx, origin, &account_id).await?;
            require_active(&account)?;
            if account.primary_category != PrimaryCategory::Liability {
                return Err(AppError::invalid_activity(
                    "Debt adjustments require a liability account.",
                ));
            }
            let current =
                latest_account_value_money(tx, &account_id.to_string(), target.currency()).await?;
            Activity::debt_adjustment(params, account_id, current, target)
        }
        PostCommand::PositionAdjustment { holding_id, target } => {
            let (holding, account) = load_required_holding(tx, origin, &holding_id).await?;
            require_active(&account)?;
            if holding.archived {
                return Err(AppError::validation(
                    "holdingId",
                    "Quantity cannot be updated on an archived holding.",
                ));
            }
            require_holdings_account(&account)?;
            let current = latest_holding_quantity(tx, &holding).await?;
            let endpoint = QuantityEndpoint {
                account_id: holding.account_id,
                holding_id: holding.id,
                instrument_id: holding.instrument_id,
            };
            Activity::position_adjustment(params, endpoint, current, target)
        }
        PostCommand::Deposit { endpoint, amount } => {
            validate_external_money_endpoint(tx, origin, endpoint, MoneyFlowKind::Deposit).await?;
            Activity::deposit(params, endpoint, amount)
        }
        PostCommand::Withdrawal { endpoint, amount } => {
            validate_external_money_endpoint(tx, origin, endpoint, MoneyFlowKind::Withdrawal)
                .await?;
            Activity::withdrawal(params, endpoint, amount)
        }
        PostCommand::Income {
            endpoint,
            amount,
            kind,
            instrument_id,
        } => {
            validate_external_money_endpoint(tx, origin, endpoint, MoneyFlowKind::Income).await?;
            validate_related_instrument(tx, origin, instrument_id).await?;
            Activity::income(params, endpoint, amount, kind, instrument_id)
        }
        PostCommand::Fee {
            endpoint,
            amount,
            kind,
            instrument_id,
        } => {
            validate_external_money_endpoint(tx, origin, endpoint, MoneyFlowKind::Fee).await?;
            validate_related_instrument(tx, origin, instrument_id).await?;
            Activity::fee(params, endpoint, amount, kind, instrument_id)
        }
        PostCommand::CashTransfer {
            source,
            destination,
            source_amount,
            destination_amount,
            fx_rate,
        } => {
            validate_transfer_money_endpoint(tx, origin, source).await?;
            validate_transfer_money_endpoint(tx, origin, destination).await?;
            Activity::cash_transfer(
                params,
                source,
                destination,
                source_amount,
                destination_amount,
                fx_rate,
            )
        }
        PostCommand::PositionTransfer {
            source,
            destination,
            quantity,
        } => {
            validate_quantity_endpoint(tx, origin, source).await?;
            validate_quantity_endpoint(tx, origin, destination).await?;
            Activity::position_transfer(params, source, destination, quantity)
        }
        PostCommand::Buy(spec) => {
            validate_trade(tx, origin, &spec).await?;
            Activity::buy(params, spec)
        }
        PostCommand::Sell(spec) => {
            validate_trade(tx, origin, &spec).await?;
            Activity::sell(params, spec)
        }
        PostCommand::DebtDraw(spec) => {
            validate_debt_draw(tx, origin, &spec).await?;
            Activity::debt_draw(params, spec)
        }
        PostCommand::DebtPayment(spec) => {
            validate_debt_payment(tx, origin, &spec).await?;
            Activity::debt_payment(params, spec)
        }
    }
}

#[derive(Clone, Copy)]
enum MoneyFlowKind {
    Deposit,
    Withdrawal,
    Income,
    Fee,
}

async fn validate_opening(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    opening: &ComponentOpening,
) -> Result<(), AppError> {
    match opening {
        ComponentOpening::AccountValue { account_id, .. } => {
            let account = load_required_account(tx, origin, account_id).await?;
            require_active(&account)?;
            if account.tracking_mode == TrackingMode::Holdings {
                return Err(AppError::invalid_activity(
                    "Holdings accounts cannot record a simple account value.",
                ));
            }
            if prior_account_value_exists(tx, &account_id.to_string()).await? {
                return Err(AppError::invalid_activity(
                    "Opening adjustments require a new account with no prior financial state.",
                ));
            }
            Ok(())
        }
        ComponentOpening::HoldingsCash { account_id, amount } => {
            let account = load_required_account(tx, origin, account_id).await?;
            require_active(&account)?;
            require_holdings_account(&account)?;
            if prior_cash_exists(tx, &account_id.to_string(), amount.currency()).await? {
                return Err(AppError::invalid_activity(
                    "Opening adjustments require a new cash component with no prior financial state.",
                ));
            }
            Ok(())
        }
        ComponentOpening::HoldingQuantity {
            account_id,
            holding_id,
            instrument_id,
            ..
        } => {
            let (holding, account) = load_required_holding(tx, origin, holding_id).await?;
            require_active(&account)?;
            require_holdings_account(&account)?;
            if holding.archived {
                return Err(AppError::validation(
                    "holdingId",
                    "Quantity cannot be updated on an archived holding.",
                ));
            }
            if holding.account_id != *account_id || holding.instrument_id != *instrument_id {
                return Err(AppError::invalid_activity(
                    "The holding does not match the opening adjustment endpoint.",
                ));
            }
            if prior_quantity_exists(tx, &holding_id.to_string()).await? {
                return Err(AppError::invalid_activity(
                    "Opening adjustments require a new holding with no prior quantity state.",
                ));
            }
            Ok(())
        }
    }
}

async fn validate_external_money_endpoint(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    endpoint: MonetaryEndpoint,
    kind: MoneyFlowKind,
) -> Result<(), AppError> {
    let account = load_required_account(tx, origin, &endpoint.account_id).await?;
    require_active(&account)?;
    match endpoint.component {
        MonetaryComponent::AccountValue => {
            if account.tracking_mode == TrackingMode::ManualValue {
                return Err(AppError::invalid_activity(
                    "Manual value accounts accept manual valuation, not deposits, withdrawals, income, or fees.",
                ));
            }
            if account.primary_category == PrimaryCategory::Liability {
                return Err(AppError::invalid_activity(
                    "Liability accounts use debt commands, not deposits, withdrawals, income, or fees.",
                ));
            }
            if account.tracking_mode != TrackingMode::Balance {
                return Err(wrong_tracking_mode(kind));
            }
            Ok(())
        }
        MonetaryComponent::HoldingsCash => {
            require_holdings_account(&account)?;
            Ok(())
        }
    }
}

fn wrong_tracking_mode(kind: MoneyFlowKind) -> AppError {
    let message = match kind {
        MoneyFlowKind::Deposit | MoneyFlowKind::Withdrawal => {
            "Deposits and withdrawals require a balance account or holdings cash."
        }
        MoneyFlowKind::Income => "Income requires a balance account or holdings cash.",
        MoneyFlowKind::Fee => "Fees require a balance account or holdings cash.",
    };
    AppError::invalid_activity(message)
}

async fn validate_related_instrument(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    instrument_id: Option<InstrumentId>,
) -> Result<(), AppError> {
    let Some(instrument_id) = instrument_id else {
        return Ok(());
    };
    instrument_service::load_instrument_domain(
        tx,
        &origin.household_id,
        &instrument_id.to_string(),
    )
    .await?;
    Ok(())
}

#[derive(Clone, Copy)]
enum ChainMutation {
    Reverse,
    Correct,
}

async fn load_current_chain_activity(
    tx: &mut Transaction<'_, Sqlite>,
    original_id: &str,
    mutation: ChainMutation,
) -> Result<Activity, AppError> {
    let original = get_activity(tx, original_id)
        .await?
        .ok_or_else(|| AppError::not_found("activity", original_id))?;
    if original.kind() == ActivityKind::Reversal || original.reverses().is_some() {
        return Err(AppError::activity_not_correctable(
            "A reversal cannot be reversed or corrected. Target the current replacement instead.",
        ));
    }
    if get_activity_by_reverses(tx, original_id).await?.is_some() {
        return Err(match mutation {
            ChainMutation::Reverse => AppError::ActivityAlreadyReversed,
            ChainMutation::Correct => AppError::activity_not_correctable(
                "This activity has already been reversed. Target the latest unreversed replacement.",
            ),
        });
    }
    if get_activity_by_corrects(tx, original_id).await?.is_some() {
        return Err(AppError::activity_not_correctable(
            "This activity is not the current correction-chain node.",
        ));
    }
    Ok(original)
}

async fn validate_transfer_money_endpoint(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    endpoint: MonetaryEndpoint,
) -> Result<(), AppError> {
    let account = load_required_account(tx, origin, &endpoint.account_id).await?;
    require_active(&account)?;
    match endpoint.component {
        MonetaryComponent::AccountValue => {
            if account.tracking_mode == TrackingMode::ManualValue {
                return Err(AppError::invalid_activity(
                    "Manual value accounts accept manual valuation, not transfers.",
                ));
            }
            if account.primary_category == PrimaryCategory::Liability {
                return Err(AppError::invalid_activity(
                    "Liability accounts use debt commands, not transfers.",
                ));
            }
            if account.tracking_mode != TrackingMode::Balance {
                return Err(AppError::invalid_activity(
                    "Cash transfers require a balance account or holdings cash.",
                ));
            }
            Ok(())
        }
        MonetaryComponent::HoldingsCash => {
            require_holdings_account(&account)?;
            Ok(())
        }
    }
}

async fn validate_quantity_endpoint(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    endpoint: QuantityEndpoint,
) -> Result<(), AppError> {
    let (holding, account) = load_required_holding(tx, origin, &endpoint.holding_id).await?;
    require_active(&account)?;
    require_holdings_account(&account)?;
    if holding.archived {
        return Err(AppError::validation(
            "holdingId",
            "Quantity cannot be updated on an archived holding.",
        ));
    }
    if holding.account_id != endpoint.account_id || holding.instrument_id != endpoint.instrument_id
    {
        return Err(AppError::invalid_activity(
            "The holding does not match the transfer endpoint.",
        ));
    }
    Ok(())
}

async fn validate_trade(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    spec: &TradeSpec,
) -> Result<(), AppError> {
    let (holding, account) = load_required_holding(tx, origin, &spec.holding_id).await?;
    require_active(&account)?;
    require_holdings_account(&account)?;
    if holding.archived {
        return Err(AppError::validation(
            "holdingId",
            "Quantity cannot be updated on an archived holding.",
        ));
    }
    if holding.account_id != spec.account_id || holding.instrument_id != spec.instrument_id {
        return Err(AppError::invalid_activity(
            "The holding does not match the trade endpoint.",
        ));
    }
    let instrument = instrument_service::load_instrument_domain(
        tx,
        &origin.household_id,
        &spec.instrument_id.to_string(),
    )
    .await?;
    if instrument.quote_currency() != spec.quote_currency {
        return Err(AppError::invalid_activity(
            "Settlement currency must equal the instrument quote currency.",
        ));
    }
    Ok(())
}

async fn validate_liability_account(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    account_id: &AccountId,
) -> Result<(), AppError> {
    let account = load_required_account(tx, origin, account_id).await?;
    require_active(&account)?;
    if account.primary_category != PrimaryCategory::Liability {
        return Err(AppError::invalid_activity(
            "Debt commands require a liability account.",
        ));
    }
    Ok(())
}

async fn validate_debt_draw(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    spec: &DebtDrawSpec,
) -> Result<(), AppError> {
    validate_liability_account(tx, origin, &spec.liability_account_id).await?;
    if let Some(cash) = &spec.cash {
        validate_transfer_money_endpoint(tx, origin, cash.endpoint).await?;
    }
    Ok(())
}

async fn validate_debt_payment(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    spec: &DebtPaymentSpec,
) -> Result<(), AppError> {
    validate_liability_account(tx, origin, &spec.liability_account_id).await?;
    validate_transfer_money_endpoint(tx, origin, spec.cash.endpoint).await?;
    Ok(())
}

async fn persist_ledger_activities(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    activities: &[Activity],
) -> Result<(), AppError> {
    if activities.is_empty() {
        return Err(AppError::Internal);
    }
    let finals = replay_affected_endpoints(tx, origin, activities).await?;
    for activity in activities {
        insert_activity(tx, activity).await?;
    }
    persist_replayed_projections(tx, activities, &finals).await?;
    let mut earliest = activities[0].effective_local_date();
    let mut dirty_at = activities[0].created_at().clone();
    for activity in activities.iter().skip(1) {
        if activity.effective_local_date() < earliest {
            earliest = activity.effective_local_date();
        }
        if activity.created_at() < &dirty_at {
            dirty_at = activity.created_at().clone();
        }
    }
    mark_snapshots_dirty_from(
        tx,
        &origin.household_id,
        &earliest.to_ymd(),
        &dirty_at.to_rfc3339(),
    )
    .await?;
    Ok(())
}

fn activity_order_key(activity: &Activity) -> (&Timestamp, &Timestamp, ActivityId) {
    (
        activity.effective_at(),
        activity.created_at(),
        activity.id(),
    )
}

async fn replay_affected_endpoints(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    extras: &[Activity],
) -> Result<HashMap<EndpointKey, EndpointState>, AppError> {
    let mut affected = Vec::new();
    for activity in extras {
        for leg in activity.legs() {
            let key = endpoint_key(leg)?;
            if !affected.iter().any(|existing| existing == &key) {
                affected.push(key);
            }
        }
    }
    let mut states = load_origin_endpoint_states(tx, origin, &affected).await?;
    let mut sequence = list_all_activities_asc(tx, &origin.household_id).await?;
    sequence.extend(extras.iter().cloned());
    sequence.sort_by(|left, right| activity_order_key(left).cmp(&activity_order_key(right)));
    for activity in &sequence {
        for leg in activity.legs() {
            let key = endpoint_key(leg)?;
            if !affected.contains(&key) {
                continue;
            }
            apply_leg_to_state(states.get_mut(&key).ok_or(AppError::Internal)?, leg)?;
        }
    }
    Ok(states)
}

fn apply_leg_to_state(state: &mut EndpointState, leg: &ActivityLeg) -> Result<(), AppError> {
    match (state, leg.component()) {
        (
            EndpointState::Money { resulting, .. },
            LegComponent::AccountValue { amount: _ } | LegComponent::HoldingsCash { amount: _ },
        ) => {
            *resulting = leg.apply_to_money(*resulting)?;
            Ok(())
        }
        (EndpointState::Quantity { resulting, .. }, LegComponent::HoldingQuantity { .. }) => {
            *resulting = leg.apply_to_quantity(*resulting)?;
            Ok(())
        }
        _ => Err(AppError::Internal),
    }
}

fn endpoint_key(leg: &ActivityLeg) -> Result<EndpointKey, AppError> {
    match leg.component() {
        LegComponent::AccountValue { amount } => money_key(
            leg.account_id(),
            amount.currency(),
            &LegComponent::AccountValue { amount: *amount },
        ),
        LegComponent::HoldingsCash { amount } => money_key(
            leg.account_id(),
            amount.currency(),
            &LegComponent::HoldingsCash { amount: *amount },
        ),
        LegComponent::HoldingQuantity { holding_id, .. } => Ok(EndpointKey::HoldingQuantity {
            holding_id: holding_id.to_string(),
        }),
    }
}

async fn load_origin_endpoint_states(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    keys: &[EndpointKey],
) -> Result<HashMap<EndpointKey, EndpointState>, AppError> {
    let account_values = list_origin_account_values(tx, &origin.id).await?;
    let cash_values = list_origin_cash_values(tx, &origin.id).await?;
    let holdings = list_origin_holdings(tx, &origin.id).await?;
    let mut states = HashMap::new();
    for key in keys {
        let state = match key {
            EndpointKey::AccountValue {
                account_id,
                currency,
            } => {
                let currency = CurrencyCode::parse(currency)?;
                let amount = account_values
                    .iter()
                    .find(|row| row.account_id == *account_id && row.currency == currency.as_str())
                    .map(|row| Money::parse(&row.amount, currency))
                    .transpose()?
                    .unwrap_or(Money::parse("0", currency)?);
                EndpointState::Money {
                    account_id: AccountId::parse(account_id)?,
                    component: MonetaryComponent::AccountValue,
                    resulting: amount,
                }
            }
            EndpointKey::HoldingsCash {
                account_id,
                currency,
            } => {
                let currency = CurrencyCode::parse(currency)?;
                let amount = cash_values
                    .iter()
                    .find(|row| row.account_id == *account_id && row.currency == currency.as_str())
                    .map(|row| Money::parse(&row.amount, currency))
                    .transpose()?
                    .unwrap_or(Money::parse("0", currency)?);
                EndpointState::Money {
                    account_id: AccountId::parse(account_id)?,
                    component: MonetaryComponent::HoldingsCash,
                    resulting: amount,
                }
            }
            EndpointKey::HoldingQuantity { holding_id } => {
                let quantity = holdings
                    .iter()
                    .find(|row| row.holding_id == *holding_id)
                    .map(|row| Quantity::parse(&row.quantity))
                    .transpose()?
                    .unwrap_or(Quantity::parse("0")?);
                EndpointState::Quantity {
                    holding_id: HoldingId::parse(holding_id)?,
                    resulting: quantity,
                }
            }
        };
        states.insert(key.clone(), state);
    }
    Ok(states)
}

async fn persist_replayed_projections(
    tx: &mut Transaction<'_, Sqlite>,
    activities: &[Activity],
    finals: &HashMap<EndpointKey, EndpointState>,
) -> Result<(), AppError> {
    let mut current_latest = HashMap::new();
    for key in finals.keys() {
        if let Some(existing) = latest_projection_effective_at(tx, key).await? {
            current_latest.insert(key.clone(), existing);
        }
    }
    let last_index = activities.len().saturating_sub(1);
    for (index, activity) in activities.iter().enumerate() {
        let mut effective_at = activity.effective_at().clone();
        if index == last_index {
            for existing in current_latest.values() {
                if existing > &effective_at {
                    effective_at = existing.clone();
                }
            }
        }
        persist_projections_at(tx, activity, finals, &effective_at).await?;
    }
    Ok(())
}

async fn latest_projection_effective_at(
    tx: &mut Transaction<'_, Sqlite>,
    key: &EndpointKey,
) -> Result<Option<Timestamp>, AppError> {
    let value: Option<String> = match key {
        EndpointKey::AccountValue { account_id, .. } => sqlx::query_scalar(
            "SELECT effective_at FROM account_values
             WHERE account_id = ?
             ORDER BY effective_at DESC, created_at DESC, id DESC
             LIMIT 1",
        )
        .bind(account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| map_read_error("activity.account_value_effective_failed", error))?,
        EndpointKey::HoldingsCash {
            account_id,
            currency,
        } => sqlx::query_scalar(
            "SELECT effective_at FROM account_cash_values
             WHERE account_id = ? AND currency = ?
             ORDER BY effective_at DESC, created_at DESC, id DESC
             LIMIT 1",
        )
        .bind(account_id)
        .bind(currency)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| map_read_error("activity.cash_effective_failed", error))?,
        EndpointKey::HoldingQuantity { holding_id } => sqlx::query_scalar(
            "SELECT effective_at FROM holding_quantity_values
             WHERE holding_id = ?
             ORDER BY effective_at DESC, created_at DESC, id DESC
             LIMIT 1",
        )
        .bind(holding_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| map_read_error("activity.quantity_effective_failed", error))?,
    };
    value.map(|value| Timestamp::parse(&value)).transpose()
}

async fn apply_and_persist(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    activity: &Activity,
) -> Result<(), AppError> {
    let mut states = load_endpoint_states(tx, activity).await?;
    for leg in activity.legs() {
        match leg.component() {
            LegComponent::AccountValue { amount } | LegComponent::HoldingsCash { amount } => {
                let key = money_key(leg.account_id(), amount.currency(), leg.component())?;
                let EndpointState::Money { resulting, .. } =
                    states.get_mut(&key).ok_or(AppError::Internal)?
                else {
                    return Err(AppError::Internal);
                };
                *resulting = leg.apply_to_money(*resulting)?;
            }
            LegComponent::HoldingQuantity { holding_id, .. } => {
                let key = EndpointKey::HoldingQuantity {
                    holding_id: holding_id.to_string(),
                };
                let EndpointState::Quantity { resulting, .. } =
                    states.get_mut(&key).ok_or(AppError::Internal)?
                else {
                    return Err(AppError::Internal);
                };
                *resulting = leg.apply_to_quantity(*resulting)?;
            }
        }
    }
    insert_activity(tx, activity).await?;
    persist_projections_at(tx, activity, &states, activity.effective_at()).await?;
    mark_snapshots_dirty_from(
        tx,
        &origin.household_id,
        &activity.effective_local_date().to_ymd(),
        &activity.created_at().to_rfc3339(),
    )
    .await?;
    Ok(())
}

async fn load_endpoint_states(
    tx: &mut Transaction<'_, Sqlite>,
    activity: &Activity,
) -> Result<HashMap<EndpointKey, EndpointState>, AppError> {
    let mut states = HashMap::new();
    for leg in activity.legs() {
        match leg.component() {
            LegComponent::AccountValue { amount } => {
                let key = money_key(
                    leg.account_id(),
                    amount.currency(),
                    &LegComponent::AccountValue { amount: *amount },
                )?;
                if states.contains_key(&key) {
                    continue;
                }
                let current = latest_account_value_money(
                    tx,
                    &leg.account_id().to_string(),
                    amount.currency(),
                )
                .await?;
                states.insert(
                    key,
                    EndpointState::Money {
                        account_id: leg.account_id(),
                        component: MonetaryComponent::AccountValue,
                        resulting: current,
                    },
                );
            }
            LegComponent::HoldingsCash { amount } => {
                let key = money_key(
                    leg.account_id(),
                    amount.currency(),
                    &LegComponent::HoldingsCash { amount: *amount },
                )?;
                if states.contains_key(&key) {
                    continue;
                }
                let current =
                    latest_cash_money(tx, &leg.account_id().to_string(), amount.currency()).await?;
                states.insert(
                    key,
                    EndpointState::Money {
                        account_id: leg.account_id(),
                        component: MonetaryComponent::HoldingsCash,
                        resulting: current,
                    },
                );
            }
            LegComponent::HoldingQuantity { holding_id, .. } => {
                let key = EndpointKey::HoldingQuantity {
                    holding_id: holding_id.to_string(),
                };
                if states.contains_key(&key) {
                    continue;
                }
                let quantity = latest_holding_quantity_for_id(tx, &holding_id.to_string()).await?;
                states.insert(
                    key,
                    EndpointState::Quantity {
                        holding_id: *holding_id,
                        resulting: quantity,
                    },
                );
            }
        }
    }
    Ok(states)
}

fn money_key(
    account_id: AccountId,
    currency: CurrencyCode,
    component: &LegComponent,
) -> Result<EndpointKey, AppError> {
    Ok(match component {
        LegComponent::AccountValue { .. } => EndpointKey::AccountValue {
            account_id: account_id.to_string(),
            currency: currency.as_str().to_owned(),
        },
        LegComponent::HoldingsCash { .. } => EndpointKey::HoldingsCash {
            account_id: account_id.to_string(),
            currency: currency.as_str().to_owned(),
        },
        LegComponent::HoldingQuantity { .. } => {
            return Err(AppError::Internal);
        }
    })
}

async fn persist_projections_at(
    tx: &mut Transaction<'_, Sqlite>,
    activity: &Activity,
    states: &HashMap<EndpointKey, EndpointState>,
    effective_at: &Timestamp,
) -> Result<(), AppError> {
    let activity_id = activity.id().to_string();
    for state in states.values() {
        match state {
            EndpointState::Money {
                account_id,
                component,
                resulting,
                ..
            } => match component {
                MonetaryComponent::AccountValue => {
                    insert_account_value_projection(
                        tx,
                        *account_id,
                        *resulting,
                        activity,
                        effective_at,
                    )
                    .await?;
                    touch_account(tx, &account_id.to_string(), activity.created_at()).await?;
                }
                MonetaryComponent::HoldingsCash => {
                    insert_cash_projection(tx, *account_id, *resulting, activity, effective_at)
                        .await?;
                }
            },
            EndpointState::Quantity {
                holding_id,
                resulting,
                ..
            } => {
                insert_holding_quantity(
                    tx,
                    &HoldingQuantityRecord {
                        id: HoldingQuantityValueId::new().to_string(),
                        holding_id: holding_id.to_string(),
                        quantity: resulting.canonical(),
                        effective_at: effective_at.to_rfc3339(),
                        created_at: activity.created_at().to_rfc3339(),
                        activity_id: Some(activity_id.clone()),
                    },
                )
                .await?;
                sqlx::query("UPDATE holdings SET quantity = ?, updated_at = ? WHERE id = ?")
                    .bind(resulting.canonical())
                    .bind(activity.created_at().to_rfc3339())
                    .bind(holding_id.to_string())
                    .execute(&mut **tx)
                    .await
                    .map_err(|error| {
                        map_write_error("activity.holding_quantity_sync_failed", error)
                    })?;
            }
        }
    }
    Ok(())
}

async fn insert_account_value_projection(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: AccountId,
    money: Money,
    activity: &Activity,
    effective_at: &Timestamp,
) -> Result<(), AppError> {
    let tracking = load_required_account_tracking(tx, &account_id).await?;
    let value = AccountValue::from_persisted(PersistedAccountValue {
        id: AccountValueId::new(),
        account_id,
        value_kind: ValueKind::from_tracking_mode(tracking)?,
        money,
        effective_at: effective_at.clone(),
        created_at: activity.created_at().clone(),
    });
    sqlx::query(
        "INSERT INTO account_values
         (id, account_id, value_kind, amount, currency, effective_at, created_at, activity_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(value.id().to_string())
    .bind(value.account_id().to_string())
    .bind(value.value_kind().as_str())
    .bind(value.money().canonical_amount())
    .bind(value.money().currency().as_str())
    .bind(value.effective_at().to_rfc3339())
    .bind(value.created_at().to_rfc3339())
    .bind(activity.id().to_string())
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("activity.account_value_insert_failed", error))?;
    Ok(())
}

async fn insert_cash_projection(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: AccountId,
    money: Money,
    activity: &Activity,
    effective_at: &Timestamp,
) -> Result<(), AppError> {
    let value = AccountCashValue::from_persisted(
        AccountCashValueId::new(),
        account_id,
        money,
        effective_at.clone(),
        activity.created_at().clone(),
    );
    sqlx::query(
        "INSERT INTO account_cash_values
         (id, account_id, amount, currency, effective_at, created_at, activity_id)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(value.id().to_string())
    .bind(value.account_id().to_string())
    .bind(value.money().canonical_amount())
    .bind(value.currency().as_str())
    .bind(value.effective_at().to_rfc3339())
    .bind(value.created_at().to_rfc3339())
    .bind(activity.id().to_string())
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("activity.cash_insert_failed", error))?;
    Ok(())
}

async fn touch_account(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    updated_at: &Timestamp,
) -> Result<(), AppError> {
    sqlx::query("UPDATE accounts SET updated_at = ? WHERE id = ?")
        .bind(updated_at.to_rfc3339())
        .bind(account_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("activity.account_touch_failed", error))?;
    Ok(())
}

async fn load_required_account(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    account_id: &AccountId,
) -> Result<AccountRow, AppError> {
    let row = sqlx::query(
        "SELECT id, household_id, tracking_mode, primary_category, default_currency, archived_at
         FROM accounts WHERE id = ?",
    )
    .bind(account_id.to_string())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("activity.account_load_failed", error))?
    .ok_or_else(|| AppError::not_found("account", &account_id.to_string()))?;
    let household_id: String = row
        .try_get("household_id")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    if household_id != origin.household_id {
        return Err(AppError::not_found("account", &account_id.to_string()));
    }
    let archived_at: Option<String> = row
        .try_get("archived_at")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    Ok(AccountRow {
        tracking_mode: TrackingMode::parse(
            &row.try_get::<String, _>("tracking_mode")
                .map_err(|_| AppError::DatabaseUnavailable)?,
        )?,
        primary_category: PrimaryCategory::parse(
            &row.try_get::<String, _>("primary_category")
                .map_err(|_| AppError::DatabaseUnavailable)?,
        )?,
        archived: archived_at.is_some(),
    })
}

async fn load_required_account_tracking(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &AccountId,
) -> Result<TrackingMode, AppError> {
    let tracking: String = sqlx::query_scalar("SELECT tracking_mode FROM accounts WHERE id = ?")
        .bind(account_id.to_string())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| map_read_error("activity.account_tracking_load_failed", error))?
        .ok_or_else(|| AppError::not_found("account", &account_id.to_string()))?;
    TrackingMode::parse(&tracking)
}

async fn load_required_holding(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    holding_id: &HoldingId,
) -> Result<(HoldingRow, AccountRow), AppError> {
    let row = sqlx::query(
        "SELECT h.id, h.account_id, h.instrument_id, h.quantity, h.archived_at,
                a.household_id, a.tracking_mode, a.primary_category, a.default_currency,
                a.archived_at AS account_archived_at
         FROM holdings h
         JOIN accounts a ON a.id = h.account_id
         WHERE h.id = ?",
    )
    .bind(holding_id.to_string())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("activity.holding_load_failed", error))?
    .ok_or_else(|| AppError::not_found("holding", &holding_id.to_string()))?;
    let household_id: String = row
        .try_get("household_id")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    if household_id != origin.household_id {
        return Err(AppError::not_found("holding", &holding_id.to_string()));
    }
    let account_id = AccountId::parse(
        &row.try_get::<String, _>("account_id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    )?;
    let holding_archived: Option<String> = row
        .try_get("archived_at")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    let account_archived: Option<String> = row
        .try_get("account_archived_at")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    let holding = HoldingRow {
        id: HoldingId::parse(
            &row.try_get::<String, _>("id")
                .map_err(|_| AppError::DatabaseUnavailable)?,
        )?,
        account_id,
        instrument_id: InstrumentId::parse(
            &row.try_get::<String, _>("instrument_id")
                .map_err(|_| AppError::DatabaseUnavailable)?,
        )?,
        archived: holding_archived.is_some(),
    };
    let account = AccountRow {
        tracking_mode: TrackingMode::parse(
            &row.try_get::<String, _>("tracking_mode")
                .map_err(|_| AppError::DatabaseUnavailable)?,
        )?,
        primary_category: PrimaryCategory::parse(
            &row.try_get::<String, _>("primary_category")
                .map_err(|_| AppError::DatabaseUnavailable)?,
        )?,
        archived: account_archived.is_some(),
    };
    Ok((holding, account))
}

fn require_active(account: &AccountRow) -> Result<(), AppError> {
    if account.archived {
        return Err(AppError::validation(
            "accountId",
            "Activities cannot be posted against an archived account.",
        ));
    }
    Ok(())
}

fn require_holdings_account(account: &AccountRow) -> Result<(), AppError> {
    if account.tracking_mode != TrackingMode::Holdings
        || account.primary_category != PrimaryCategory::Investment
    {
        return Err(AppError::invalid_category(
            "Holdings cash and quantity require an investment holdings account.",
        ));
    }
    Ok(())
}

async fn prior_account_value_exists(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
) -> Result<bool, AppError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM account_values WHERE account_id = ?")
        .bind(account_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| map_read_error("activity.account_value_count_failed", error))?;
    Ok(count > 0)
}

async fn prior_cash_exists(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    currency: CurrencyCode,
) -> Result<bool, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_cash_values WHERE account_id = ? AND currency = ?",
    )
    .bind(account_id)
    .bind(currency.as_str())
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| map_read_error("activity.cash_count_failed", error))?;
    Ok(count > 0)
}

async fn prior_quantity_exists(
    tx: &mut Transaction<'_, Sqlite>,
    holding_id: &str,
) -> Result<bool, AppError> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM holding_quantity_values WHERE holding_id = ?")
            .bind(holding_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| map_read_error("activity.quantity_count_failed", error))?;
    Ok(count > 0)
}

async fn latest_account_value_money(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    currency: CurrencyCode,
) -> Result<Money, AppError> {
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
    .map_err(|error| map_read_error("activity.account_value_latest_failed", error))?;
    latest_money_from_row(row, currency)
}

pub(crate) async fn latest_cash_money(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    currency: CurrencyCode,
) -> Result<Money, AppError> {
    let row = sqlx::query(
        "SELECT amount, currency
         FROM account_cash_values
         WHERE account_id = ? AND currency = ?
         ORDER BY effective_at DESC, created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(account_id)
    .bind(currency.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("activity.cash_latest_failed", error))?;
    latest_money_from_row(row, currency)
}

fn latest_money_from_row(
    row: Option<sqlx::sqlite::SqliteRow>,
    expected: CurrencyCode,
) -> Result<Money, AppError> {
    let Some(row) = row else {
        return Money::parse("0", expected);
    };
    let amount: String = row
        .try_get("amount")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    let currency = CurrencyCode::parse(
        &row.try_get::<String, _>("currency")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    )?;
    if currency != expected {
        return Err(AppError::invalid_activity(
            "The target currency must match the current currency.",
        ));
    }
    Money::parse(&amount, currency)
}

async fn latest_holding_quantity(
    tx: &mut Transaction<'_, Sqlite>,
    holding: &HoldingRow,
) -> Result<Quantity, AppError> {
    latest_holding_quantity_for_id(tx, &holding.id.to_string()).await
}

async fn latest_holding_quantity_for_id(
    tx: &mut Transaction<'_, Sqlite>,
    holding_id: &str,
) -> Result<Quantity, AppError> {
    let quantity: Option<String> = sqlx::query_scalar(
        "SELECT quantity
         FROM holding_quantity_values
         WHERE holding_id = ?
         ORDER BY effective_at DESC, created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(holding_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("activity.quantity_latest_failed", error))?;
    match quantity {
        Some(quantity) => Quantity::parse(&quantity),
        None => Quantity::parse("0"),
    }
}

// Phase 4 posts Transfer, Buy/Sell, Debt Draw/Payment, reverse_activity,
// and correct_activity through this service.

#[cfg(test)]
mod tests {
    use super::{post_in_tx, PostCommand};
    use crate::{
        application::{
            account_service::{
                archive_account, create_account, insert_value, update_account_value,
                CreateAccountInput, OwnershipShareInput, UpdateAccountValueInput,
            },
            cash_service::{append_account_cash, AppendAccountCashInput},
            history_origin::confirm_history_timezone,
            history_repositories::insert_activity,
            holding_service::{
                create_holding, update_holding, CreateHoldingInput, UpdateHoldingInput,
            },
            instrument_service::{create_instrument, CreateInstrumentInput},
            member_service::list_members,
            overview_service::get_overview,
            portfolio_service::get_portfolio,
            reference::{begin_write_tx, finish_write_tx},
        },
        domain::{
            Activity, ActivityId, ActivityKind, ActivityLeg, CalendarDate, CurrencyCode, Direction,
            FeeKind, HouseholdId, IncomeKind, LegComponent, LegRole, MonetaryComponent,
            MonetaryEndpoint, Money, Timestamp, TrackingMode,
        },
        error::{AppError, ErrorCode},
        infrastructure::{database::connect_writable, database_bootstrap::MIGRATOR},
        test_support::{cleanup, onboarded_state, UNKNOWN_UUID},
    };
    use uuid::Uuid;

    fn owner(member_id: &str, percent: &str) -> OwnershipShareInput {
        OwnershipShareInput {
            member_id: member_id.to_owned(),
            percent: Some(percent.to_owned()),
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
            owners: vec![owner(member_id, "100")],
            initial_amount: Some(amount.to_owned()),
        }
    }

    async fn member_id(state: &crate::state::AppState) -> String {
        list_members(state, false).await.expect("members")[0]
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

    async fn optional_text(state: &crate::state::AppState, sql: &str) -> Option<String> {
        sqlx::query_scalar(sql)
            .fetch_one(state.writable_db().expect("writable"))
            .await
            .expect("optional text")
    }

    fn cash_endpoint(account_id: &str) -> MonetaryEndpoint {
        MonetaryEndpoint {
            account_id: crate::domain::AccountId::parse(account_id).expect("account"),
            component: MonetaryComponent::HoldingsCash,
        }
    }

    fn balance_endpoint(account_id: &str) -> MonetaryEndpoint {
        MonetaryEndpoint {
            account_id: crate::domain::AccountId::parse(account_id).expect("account"),
            component: MonetaryComponent::AccountValue,
        }
    }

    async fn brokerage(
        state: &crate::state::AppState,
        member_id: &str,
    ) -> crate::application::account_service::AccountRecordDto {
        create_account(
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
                owners: vec![owner(member_id, "100")],
                initial_amount: None,
            },
        )
        .await
        .expect("brokerage")
    }

    async fn qqq(
        state: &crate::state::AppState,
    ) -> crate::application::instrument_service::InstrumentRecordDto {
        create_instrument(
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
        .expect("instrument")
    }

    #[test]
    fn opening_posts_header_legs_projection_and_dirty_state() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-opening").await;
            let walt = member_id(&state).await;
            let created = create_account(&state, bank_input("DBS", &walt, "100000"))
                .await
                .expect("create");
            assert_eq!(created.latest_value.as_ref().unwrap().amount, "100000");
            assert_eq!(count(&state, "SELECT COUNT(*) FROM activities").await, 1);
            assert_eq!(
                text(&state, "SELECT kind FROM activities").await,
                "opening_adjustment"
            );
            assert_eq!(count(&state, "SELECT COUNT(*) FROM activity_legs").await, 1);
            assert_eq!(
                text(&state, "SELECT role FROM activity_legs").await,
                "adjustment"
            );
            assert_eq!(
                text(&state, "SELECT direction FROM activity_legs").await,
                "increase"
            );
            assert_eq!(
                text(&state, "SELECT amount FROM activity_legs").await,
                "100000"
            );
            let activity_id = text(&state, "SELECT id FROM activities").await;
            let value_activity: Option<String> = optional_text(
                &state,
                "SELECT activity_id FROM account_values ORDER BY created_at DESC, id DESC LIMIT 1",
            )
            .await;
            assert_eq!(value_activity.as_deref(), Some(activity_id.as_str()));
            assert!(
                count(
                    &state,
                    "SELECT COUNT(*) FROM history_snapshot_state WHERE dirty_from IS NOT NULL"
                )
                .await
                    > 0
            );
            cleanup(&path);
        });
    }

    #[test]
    fn zero_state_account_and_holding_write_no_activity() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-zero").await;
            let walt = member_id(&state).await;
            let account = create_account(&state, bank_input("Empty", &walt, "0"))
                .await
                .expect("zero account");
            assert_eq!(account.latest_value.as_ref().unwrap().amount, "0");
            assert_eq!(count(&state, "SELECT COUNT(*) FROM activities").await, 0);
            let value_activity: Option<String> =
                optional_text(&state, "SELECT activity_id FROM account_values").await;
            assert!(value_activity.is_none());

            let brokerage = brokerage(&state, &walt).await;
            let instrument = qqq(&state).await;
            create_holding(
                &state,
                CreateHoldingInput {
                    account_id: brokerage.id,
                    instrument_id: instrument.id,
                    quantity: "0".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("zero holding");
            assert_eq!(count(&state, "SELECT COUNT(*) FROM activities").await, 0);
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM holding_quantity_values").await,
                1
            );
            let qty_activity: Option<String> =
                optional_text(&state, "SELECT activity_id FROM holding_quantity_values").await;
            assert!(qty_activity.is_none());
            cleanup(&path);
        });
    }

    #[test]
    fn liability_opening_is_not_a_deposit() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-liability-open").await;
            let walt = member_id(&state).await;
            let mut input = bank_input("Mortgage", &walt, "500000");
            input.primary_category = "liability".to_owned();
            input.secondary_category = "mortgage".to_owned();
            create_account(&state, input).await.expect("liability");
            assert_eq!(
                text(&state, "SELECT kind FROM activities").await,
                "opening_adjustment"
            );
            assert_ne!(text(&state, "SELECT kind FROM activities").await, "deposit");
            cleanup(&path);
        });
    }

    #[test]
    fn update_account_value_maps_to_kind_specific_adjustments() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-update-map").await;
            let walt = member_id(&state).await;
            let bank = create_account(&state, bank_input("DBS", &walt, "100000"))
                .await
                .expect("bank");
            update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: bank.id.clone(),
                    amount: "110000".to_owned(),
                },
            )
            .await
            .expect("balance adjustment");
            let kinds: Vec<String> =
                sqlx::query_scalar("SELECT kind FROM activities ORDER BY created_at, id")
                    .fetch_all(state.writable_db().expect("db"))
                    .await
                    .expect("kinds");
            assert_eq!(
                kinds,
                vec![
                    "opening_adjustment".to_owned(),
                    "balance_adjustment".to_owned()
                ]
            );
            let no_change = update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: bank.id,
                    amount: "110000".to_owned(),
                },
            )
            .await
            .expect_err("no change");
            assert!(matches!(no_change, AppError::InvalidActivity { .. }));

            let mut manual = bank_input("Art", &walt, "4000");
            manual.primary_category = "property".to_owned();
            manual.secondary_category = "collectible".to_owned();
            let art = create_account(&state, manual).await.expect("manual");
            update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: art.id,
                    amount: "3500".to_owned(),
                },
            )
            .await
            .expect("manual valuation");
            assert_eq!(
                count(
                    &state,
                    "SELECT COUNT(*) FROM activities WHERE kind = 'manual_valuation'"
                )
                .await,
                1
            );

            let mut debt = bank_input("Card", &walt, "2000");
            debt.primary_category = "liability".to_owned();
            debt.secondary_category = "credit_card".to_owned();
            let card = create_account(&state, debt).await.expect("card");
            update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: card.id,
                    amount: "1500".to_owned(),
                },
            )
            .await
            .expect("debt adjustment");
            assert_eq!(
                count(
                    &state,
                    "SELECT COUNT(*) FROM activities WHERE kind = 'debt_adjustment'"
                )
                .await,
                1
            );
            cleanup(&path);
        });
    }

    #[test]
    fn cash_absolute_target_maps_to_deposit_or_withdrawal() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-cash-map").await;
            let walt = member_id(&state).await;
            let account = brokerage(&state, &walt).await;
            append_account_cash(
                &state,
                AppendAccountCashInput {
                    account_id: account.id.clone(),
                    amount: "5000".to_owned(),
                    currency: "USD".to_owned(),
                },
            )
            .await
            .expect("deposit");
            assert_eq!(text(&state, "SELECT kind FROM activities").await, "deposit");
            append_account_cash(
                &state,
                AppendAccountCashInput {
                    account_id: account.id.clone(),
                    amount: "3000".to_owned(),
                    currency: "USD".to_owned(),
                },
            )
            .await
            .expect("withdrawal");
            let kinds: Vec<String> =
                sqlx::query_scalar("SELECT kind FROM activities ORDER BY created_at, id")
                    .fetch_all(state.writable_db().expect("db"))
                    .await
                    .expect("kinds");
            assert_eq!(kinds, vec!["deposit".to_owned(), "withdrawal".to_owned()]);
            let no_change = append_account_cash(
                &state,
                AppendAccountCashInput {
                    account_id: account.id,
                    amount: "3000".to_owned(),
                    currency: "USD".to_owned(),
                },
            )
            .await
            .expect_err("no change");
            assert!(matches!(no_change, AppError::InvalidActivity { .. }));
            cleanup(&path);
        });
    }

    #[test]
    fn holding_quantity_posts_opening_and_position_adjustment() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-holding").await;
            let walt = member_id(&state).await;
            let account = brokerage(&state, &walt).await;
            let instrument = qqq(&state).await;
            let holding = create_holding(
                &state,
                CreateHoldingInput {
                    account_id: account.id,
                    instrument_id: instrument.id,
                    quantity: "3".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("create");
            assert_eq!(holding.quantity, "3");
            assert_eq!(
                text(&state, "SELECT kind FROM activities").await,
                "opening_adjustment"
            );
            let qty_activity: Option<String> =
                optional_text(&state, "SELECT activity_id FROM holding_quantity_values").await;
            assert!(qty_activity.is_some());
            update_holding(
                &state,
                UpdateHoldingInput {
                    id: holding.id.clone(),
                    quantity: "5".to_owned(),
                    note: Some("rebalanced".to_owned()),
                },
            )
            .await
            .expect("position");
            assert_eq!(
                count(
                    &state,
                    "SELECT COUNT(*) FROM activities WHERE kind = 'position_adjustment'"
                )
                .await,
                1
            );
            let synced: String = text(&state, "SELECT quantity FROM holdings").await;
            let observed: String = sqlx::query_scalar(
                "SELECT quantity FROM holding_quantity_values ORDER BY created_at DESC, id DESC LIMIT 1",
            )
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("observed");
            assert_eq!(synced, "5");
            assert_eq!(observed, "5");
            update_holding(
                &state,
                UpdateHoldingInput {
                    id: holding.id.clone(),
                    quantity: "5".to_owned(),
                    note: Some("note only".to_owned()),
                },
            )
            .await
            .expect("note only");
            assert_eq!(
                count(
                    &state,
                    "SELECT COUNT(*) FROM activities WHERE kind = 'position_adjustment'"
                )
                .await,
                1
            );
            cleanup(&path);
        });
    }

    #[test]
    fn invalid_endpoints_write_nothing() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-invalid-endpoint").await;
            let walt = member_id(&state).await;
            let bank = create_account(&state, bank_input("DBS", &walt, "100000"))
                .await
                .expect("bank");
            let before_activities = count(&state, "SELECT COUNT(*) FROM activities").await;
            let before_values = count(&state, "SELECT COUNT(*) FROM account_values").await;

            let unknown = update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: UNKNOWN_UUID.to_owned(),
                    amount: "1".to_owned(),
                },
            )
            .await
            .expect_err("unknown");
            assert!(matches!(unknown, AppError::NotFound { entity, .. } if entity == "account"));

            archive_account(&state, &bank.id).await.expect("archive");
            let archived = update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: bank.id.clone(),
                    amount: "90000".to_owned(),
                },
            )
            .await
            .expect_err("archived");
            assert!(matches!(archived, AppError::Validation { field, .. } if field == "accountId"));

            let mut manual = bank_input("Manual", &walt, "1000");
            manual.primary_category = "investment".to_owned();
            manual.secondary_category = "manual_investment".to_owned();
            manual.tracking_mode = Some("manual_value".to_owned());
            let manual = create_account(&state, manual).await.expect("manual");
            let database = state.writable_db().expect("db");
            let mut tx = begin_write_tx(database).await.expect("tx");
            let deposit_manual = post_in_tx(
                &mut tx,
                PostCommand::Deposit {
                    endpoint: balance_endpoint(&manual.id),
                    amount: Money::parse("10", CurrencyCode::CNY).expect("money"),
                },
                None,
                None,
            )
            .await
            .expect_err("manual deposit");
            finish_write_tx::<()>(tx, Err(deposit_manual.clone()))
                .await
                .expect_err("rollback");
            assert!(matches!(deposit_manual, AppError::InvalidActivity { .. }));

            let mut debt = bank_input("Loan", &walt, "100");
            debt.primary_category = "liability".to_owned();
            debt.secondary_category = "personal_debt".to_owned();
            let debt = create_account(&state, debt).await.expect("debt");
            let mut tx = begin_write_tx(database).await.expect("tx");
            let deposit_debt = post_in_tx(
                &mut tx,
                PostCommand::Deposit {
                    endpoint: balance_endpoint(&debt.id),
                    amount: Money::parse("10", CurrencyCode::CNY).expect("money"),
                },
                None,
                None,
            )
            .await
            .expect_err("liability deposit");
            finish_write_tx::<()>(tx, Err(deposit_debt.clone()))
                .await
                .expect_err("rollback");
            assert!(matches!(deposit_debt, AppError::InvalidActivity { .. }));

            let mut tx = begin_write_tx(database).await.expect("tx");
            let holdings_deposit = post_in_tx(
                &mut tx,
                PostCommand::Deposit {
                    endpoint: cash_endpoint(&manual.id),
                    amount: Money::parse("10", CurrencyCode::CNY).expect("money"),
                },
                None,
                None,
            )
            .await
            .expect_err("cash on balance");
            finish_write_tx::<()>(tx, Err(holdings_deposit.clone()))
                .await
                .expect_err("rollback");
            assert!(matches!(
                holdings_deposit,
                AppError::InvalidCategory { .. } | AppError::InvalidActivity { .. }
            ));

            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                before_activities + 2
            );
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM account_values").await,
                before_values + 2
            );
            cleanup(&path);
        });
    }

    #[test]
    fn sequential_withdrawals_validate_against_committed_result() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-serial").await;
            let walt = member_id(&state).await;
            let bank = create_account(&state, bank_input("DBS", &walt, "100"))
                .await
                .expect("bank");
            let database = state.writable_db().expect("db");
            let mut tx = begin_write_tx(database).await.expect("tx");
            post_in_tx(
                &mut tx,
                PostCommand::Withdrawal {
                    endpoint: balance_endpoint(&bank.id),
                    amount: Money::parse("60", CurrencyCode::CNY).expect("money"),
                },
                None,
                None,
            )
            .await
            .expect("first withdrawal");
            finish_write_tx(tx, Ok(())).await.expect("commit first");

            let mut tx = begin_write_tx(database).await.expect("tx");
            let second = post_in_tx(
                &mut tx,
                PostCommand::Withdrawal {
                    endpoint: balance_endpoint(&bank.id),
                    amount: Money::parse("50", CurrencyCode::CNY).expect("money"),
                },
                None,
                None,
            )
            .await
            .expect_err("insufficient");
            finish_write_tx::<()>(tx, Err(second.clone()))
                .await
                .expect_err("rollback second");
            assert!(matches!(second, AppError::InsufficientBalance));
            assert_eq!(
                second.clone().into_command_error().code,
                ErrorCode::InsufficientBalance
            );
            let latest: String = sqlx::query_scalar(
                "SELECT amount FROM account_values ORDER BY effective_at DESC, created_at DESC, id DESC LIMIT 1",
            )
            .fetch_one(database)
            .await
            .expect("latest");
            assert_eq!(latest, "40");
            assert_eq!(count(&state, "SELECT COUNT(*) FROM activities").await, 2);
            cleanup(&path);
        });
    }

    #[test]
    fn income_and_fee_update_projections_atomically() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-income-fee").await;
            let walt = member_id(&state).await;
            let bank = create_account(&state, bank_input("DBS", &walt, "100"))
                .await
                .expect("bank");
            let database = state.writable_db().expect("db");
            let mut tx = begin_write_tx(database).await.expect("tx");
            post_in_tx(
                &mut tx,
                PostCommand::Income {
                    endpoint: balance_endpoint(&bank.id),
                    amount: Money::parse("25", CurrencyCode::CNY).expect("money"),
                    kind: IncomeKind::Interest,
                    instrument_id: None,
                },
                None,
                None,
            )
            .await
            .expect("income");
            post_in_tx(
                &mut tx,
                PostCommand::Fee {
                    endpoint: balance_endpoint(&bank.id),
                    amount: Money::parse("5", CurrencyCode::CNY).expect("money"),
                    kind: FeeKind::BankFee,
                    instrument_id: None,
                },
                None,
                None,
            )
            .await
            .expect("fee");
            finish_write_tx(tx, Ok(())).await.expect("commit");
            let latest: String = sqlx::query_scalar(
                "SELECT amount FROM account_values ORDER BY effective_at DESC, created_at DESC, id DESC LIMIT 1",
            )
            .fetch_one(database)
            .await
            .expect("latest");
            assert_eq!(latest, "120");
            assert_eq!(
                count(
                    &state,
                    "SELECT COUNT(*) FROM activities WHERE kind = 'income'"
                )
                .await,
                1
            );
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities WHERE kind = 'fee'").await,
                1
            );
            cleanup(&path);
        });
    }

    #[test]
    fn persistence_failure_rolls_back_activity_and_timestamps() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-rollback").await;
            let walt = member_id(&state).await;
            let bank = create_account(&state, bank_input("DBS", &walt, "100"))
                .await
                .expect("bank");
            let before_activities = count(&state, "SELECT COUNT(*) FROM activities").await;
            let before_values = count(&state, "SELECT COUNT(*) FROM account_values").await;
            let before_updated = text(
                &state,
                &format!("SELECT updated_at FROM accounts WHERE id = '{}'", bank.id),
            )
            .await;
            let database = state.writable_db().expect("db");
            let mut tx = begin_write_tx(database).await.expect("tx");
            let activity_id = ActivityId::from_uuid(
                Uuid::parse_str("01900000-0000-7000-8000-000000000099").expect("id"),
            );
            let household = HouseholdId::parse(
                &sqlx::query_scalar::<_, String>("SELECT id FROM households")
                    .fetch_one(&mut *tx)
                    .await
                    .expect("household"),
            )
            .expect("household id");
            let leg = ActivityLeg::from_persisted(
                crate::domain::ActivityLegId::new(),
                activity_id,
                crate::domain::AccountId::parse(&bank.id).expect("account"),
                LegRole::Destination,
                Direction::Increase,
                LegComponent::AccountValue {
                    amount: Money::parse("1", CurrencyCode::CNY).expect("money"),
                },
                None,
                0,
            )
            .expect("leg");
            let activity = Activity::from_persisted(
                activity_id,
                household,
                ActivityKind::Deposit,
                Timestamp::parse("2026-06-01T12:00:00.000Z").expect("effective"),
                CalendarDate::parse("2026-06-01").expect("date"),
                Timestamp::parse("2026-06-01T12:00:01.000Z").expect("created"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                vec![leg],
            )
            .expect("activity");
            insert_activity(&mut tx, &activity)
                .await
                .expect("header should insert");
            let error = sqlx::query(
                "INSERT INTO account_values
                 (id, account_id, value_kind, amount, currency, effective_at, created_at, activity_id)
                 VALUES (?, ?, 'balance', '1', 'CNY', ?, ?, ?)",
            )
            .bind(crate::domain::AccountValueId::new().to_string())
            .bind(&bank.id)
            .bind("2026-06-01T12:00:00.000Z")
            .bind("2026-06-01T12:00:01.000Z")
            .bind("01900000-0000-7000-8000-ffffffffffff")
            .execute(&mut *tx)
            .await
            .expect_err("invalid activity fk");
            finish_write_tx::<()>(tx, Err(AppError::from(error)))
                .await
                .expect_err("rollback");
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM activities").await,
                before_activities
            );
            assert_eq!(
                count(&state, "SELECT COUNT(*) FROM account_values").await,
                before_values
            );
            assert_eq!(
                text(
                    &state,
                    &format!("SELECT updated_at FROM accounts WHERE id = '{}'", bank.id),
                )
                .await,
                before_updated
            );
            cleanup(&path);
        });
    }

    #[test]
    fn unconfirmed_timezone_blocks_posting() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-tz").await;
            let database = state.writable_db().expect("db");
            sqlx::query("UPDATE history_origins SET timezone_confirmed = 0, timezone = 'UTC'")
                .execute(database)
                .await
                .expect("unconfirm");
            let walt = member_id(&state).await;
            let error = create_account(&state, bank_input("DBS", &walt, "100"))
                .await
                .expect_err("blocked");
            assert!(matches!(
                error,
                AppError::HistoryTimezoneConfirmationRequired
            ));
            assert_eq!(
                error.into_command_error().code,
                ErrorCode::HistoryTimezoneConfirmationRequired
            );
            assert_eq!(count(&state, "SELECT COUNT(*) FROM accounts").await, 0);
            confirm_history_timezone(&state, "UTC")
                .await
                .expect("confirm");
            create_account(&state, bank_input("DBS", &walt, "100"))
                .await
                .expect("after confirm");
            cleanup(&path);
        });
    }

    #[test]
    fn post_origin_user_mutations_require_activity_id() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-linked").await;
            let walt = member_id(&state).await;
            let bank = create_account(&state, bank_input("DBS", &walt, "100"))
                .await
                .expect("bank");
            update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: bank.id.clone(),
                    amount: "110".to_owned(),
                },
            )
            .await
            .expect("update");
            let unlinked: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM account_values WHERE activity_id IS NULL AND amount != '0'",
            )
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("unlinked");
            assert_eq!(unlinked, 0);
            let linked: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM account_values WHERE activity_id IS NOT NULL",
            )
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("linked");
            assert_eq!(linked, 2);

            let holdings = brokerage(&state, &walt).await;
            append_account_cash(
                &state,
                AppendAccountCashInput {
                    account_id: holdings.id.clone(),
                    amount: "250".to_owned(),
                    currency: "USD".to_owned(),
                },
            )
            .await
            .expect("cash");
            let unlinked_cash: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM account_cash_values WHERE activity_id IS NULL AND amount != '0'",
            )
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("unlinked cash");
            assert_eq!(unlinked_cash, 0);
            let linked_cash: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM account_cash_values WHERE activity_id IS NOT NULL",
            )
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("linked cash");
            assert_eq!(linked_cash, 1);

            let instrument = qqq(&state).await;
            let holding = create_holding(
                &state,
                CreateHoldingInput {
                    account_id: holdings.id,
                    instrument_id: instrument.id,
                    quantity: "1".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("holding");
            update_holding(
                &state,
                UpdateHoldingInput {
                    id: holding.id,
                    quantity: "2".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("quantity");
            let unlinked_qty: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM holding_quantity_values WHERE activity_id IS NULL",
            )
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("unlinked quantity");
            assert_eq!(unlinked_qty, 0);
            let linked_qty: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM holding_quantity_values WHERE activity_id IS NOT NULL",
            )
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("linked quantity");
            assert_eq!(linked_qty, 2);

            let database = state.writable_db().expect("db");
            let mut tx = begin_write_tx(database).await.expect("tx");
            let value = crate::domain::AccountValue::initial(
                crate::domain::AccountId::parse(&bank.id).expect("id"),
                TrackingMode::Balance,
                Money::parse("1", CurrencyCode::CNY).expect("money"),
                Timestamp::now(),
            )
            .expect("value");
            let error = insert_value(&mut tx, &value, None)
                .await
                .expect_err("direct insert");
            finish_write_tx::<()>(tx, Err(error.clone()))
                .await
                .expect_err("rollback");
            assert!(matches!(error, AppError::InvalidActivity { .. }));
            cleanup(&path);
        });
    }

    #[test]
    fn migrated_legacy_observations_keep_null_activity_id() {
        tauri::async_runtime::block_on(async {
            let path = crate::test_support::test_path("phase3-v012", "legacy-null");
            let _ = std::fs::remove_file(&path);
            let pool = connect_writable(&path, true).await.expect("open");
            for version in [1_i64, 2] {
                let migration = MIGRATOR
                    .iter()
                    .find(|item| item.version == version)
                    .expect("migration")
                    .clone();
                let mut conn = pool.acquire().await.expect("conn");
                sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
                    .await
                    .expect("meta");
                sqlx::migrate::Migrate::apply(&mut *conn, &migration)
                    .await
                    .expect("apply");
            }
            sqlx::raw_sql(include_str!("../../test-fixtures/v0.1.2.sql"))
                .execute(&pool)
                .await
                .expect("fixture");
            pool.close().await;
            let state = crate::state::AppState::initialize(path.clone()).await;
            let null_values: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM account_values WHERE activity_id IS NULL")
                    .fetch_one(state.writable_db().expect("db"))
                    .await
                    .expect("null values");
            let total_values: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM account_values")
                .fetch_one(state.writable_db().expect("db"))
                .await
                .expect("total values");
            assert_eq!(null_values, total_values);
            assert!(null_values > 0);
            let overview = get_overview(&state).await.expect("overview");
            assert_eq!(overview.net_worth.amount, "63190");
            let portfolio = get_portfolio(&state).await.expect("portfolio");
            assert_eq!(portfolio.total.amount, "63190");
            cleanup(&path);
        });
    }

    #[test]
    fn activity_decimals_remain_canonical() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-canonical").await;
            let walt = member_id(&state).await;
            create_account(&state, bank_input("DBS", &walt, "125000.50"))
                .await
                .expect("create");
            assert_eq!(
                text(&state, "SELECT amount FROM activity_legs").await,
                "125000.5"
            );
            assert_eq!(
                text(&state, "SELECT amount FROM account_values").await,
                "125000.5"
            );
            cleanup(&path);
        });
    }

    #[test]
    fn derived_absolute_targets_cannot_go_negative() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("activity-nonneg").await;
            let walt = member_id(&state).await;
            let bank = create_account(&state, bank_input("DBS", &walt, "100"))
                .await
                .expect("bank");
            update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: bank.id.clone(),
                    amount: "0".to_owned(),
                },
            )
            .await
            .expect("zero target");
            assert_eq!(
                optional_text(
                    &state,
                    "SELECT amount FROM account_values ORDER BY created_at DESC, id DESC LIMIT 1"
                )
                .await
                .as_deref(),
                Some("0")
            );
            let negative = update_account_value(
                &state,
                UpdateAccountValueInput {
                    id: bank.id,
                    amount: "-1".to_owned(),
                },
            )
            .await
            .expect_err("negative");
            assert!(matches!(negative, AppError::InvalidMoney { .. }));
            cleanup(&path);
        });
    }
}

#[cfg(test)]
#[path = "activity_service_phase4.rs"]
mod phase4_tests;
