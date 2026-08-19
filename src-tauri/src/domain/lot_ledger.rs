//! Deterministic FIFO lot ledger.
//!
//! Replay is pure domain code: it never reads SQL, snapshots, quotes, or current
//! projection state. The application mapper later builds [`LedgerEvent`]s from
//! posted Activities.
//!
//! ## Lot identity
//!
//! `LotRef` is the opening event, not a generated UUID:
//!
//! ```text
//! LotRef = OriginHolding(HoldingId) | Acquisition(ActivityLegId)
//! ```
//!
//! `history_origin_holdings` is keyed by `(origin_id, holding_id)` and has no
//! origin-item UUID column, so origin lots use [`HoldingId`]. A transferred lot
//! keeps its original `LotRef`.
//!
//! ## Event order
//!
//! Replay sorts internally by
//! `effective_at ASC, created_at ASC, activity/origin id ASC, sort_order ASC,
//! holding/leg id ASC`. Origin baselines use the origin timestamp as both
//! `effective_at` and `created_at`, the origin id as the activity-id sentinel,
//! and the holding id as the leg-id sentinel.
//!
//! ## Consumption order
//!
//! Lots are consumed by
//! `acquired_at ASC, opening activity created_at ASC, opening leg id ASC`.
//! Origin lots use the origin timestamp as both `acquired_at` and the opening
//! `created_at` sentinel, and the holding id as the opening-leg-id sentinel.
//! Transferred lots keep their original acquisition time, not the transfer time.
//!
//! A reversed Activity and the Reversal that reversed it are both excluded.
//! Quantity shortfall is an integrity diagnostic: replay does not clamp, does
//! not apply the failing event, and refuses to produce gain totals.

use std::cmp::Ordering;
use std::collections::HashSet;

use rust_decimal::Decimal;
use uuid::Uuid;

use super::{
    decimal::{canonical_decimal, checked_add, checked_div, checked_mul, checked_sub},
    ids::{AccountId, ActivityId, ActivityLegId, HistoryOriginId, HoldingId, InstrumentId},
    money::Money,
    quantity::Quantity,
    time::Timestamp,
};
use crate::error::AppError;

/// Derived identity of the event that opened a lot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LotRef {
    OriginHolding(HoldingId),
    Acquisition(ActivityLegId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BasisStatus {
    Known,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumptionKind {
    Realized,
    UnexplainedDisposal,
    Transfer,
}

/// One lot-affecting (or explicitly non-affecting) ledger fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerEvent {
    OriginBaseline {
        origin_id: HistoryOriginId,
        holding_id: HoldingId,
        instrument_id: InstrumentId,
        account_id: AccountId,
        quantity: Quantity,
        origin_at: Timestamp,
    },
    Activity(ActivityLedgerEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityLedgerEvent {
    pub activity_id: ActivityId,
    pub created_at: Timestamp,
    pub effective_at: Timestamp,
    /// When this Activity is a Reversal, the Activity it reverses.
    pub reverses: Option<ActivityId>,
    /// When this Activity has been reversed, the Reversal Activity id.
    pub reversed_by: Option<ActivityId>,
    pub sort_order: i64,
    pub effect: LotEffect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LotEffect {
    Buy {
        holding_leg_id: ActivityLegId,
        instrument_id: InstrumentId,
        account_id: AccountId,
        quantity: Quantity,
        /// Persisted gross settlement. `None` is a confirmed zero-gross Buy:
        /// cost `0`, basis known.
        gross_settlement: Option<Money>,
        acquisition_fee: Option<Money>,
    },
    Sell {
        holding_leg_id: ActivityLegId,
        instrument_id: InstrumentId,
        account_id: AccountId,
        quantity: Quantity,
        proceeds_gross: Option<Money>,
        disposal_fee: Option<Money>,
    },
    OpeningIncrease {
        holding_leg_id: ActivityLegId,
        instrument_id: InstrumentId,
        account_id: AccountId,
        quantity: Quantity,
    },
    PositionIncrease {
        holding_leg_id: ActivityLegId,
        instrument_id: InstrumentId,
        account_id: AccountId,
        quantity: Quantity,
    },
    PositionDecrease {
        holding_leg_id: ActivityLegId,
        instrument_id: InstrumentId,
        account_id: AccountId,
        quantity: Quantity,
    },
    PositionTransfer {
        source_leg_id: ActivityLegId,
        destination_leg_id: ActivityLegId,
        instrument_id: InstrumentId,
        source_account_id: AccountId,
        destination_account_id: AccountId,
        quantity: Quantity,
    },
    None,
}

/// Identity of a lot at the moment it was opened, including later-consumed lots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LotOpening {
    lot_ref: LotRef,
    instrument_id: InstrumentId,
    acquired_at: Timestamp,
    basis: BasisStatus,
}

impl LotOpening {
    #[must_use]
    pub fn lot_ref(&self) -> LotRef {
        self.lot_ref
    }

    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[must_use]
    pub fn acquired_at(&self) -> &Timestamp {
        &self.acquired_at
    }

    #[must_use]
    pub fn basis(&self) -> BasisStatus {
        self.basis
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenLot {
    lot_ref: LotRef,
    quantity_remaining: Quantity,
    original_quantity: Quantity,
    cost_remaining: Option<Decimal>,
    original_cost: Option<Decimal>,
    acquisition_fee_remaining: Option<Decimal>,
    original_acquisition_fee: Option<Decimal>,
    cost_currency: Option<super::currency::CurrencyCode>,
    acquired_at: Timestamp,
    account_id: AccountId,
    instrument_id: InstrumentId,
    basis: BasisStatus,
}

impl OpenLot {
    #[must_use]
    pub fn lot_ref(&self) -> LotRef {
        self.lot_ref
    }

    #[must_use]
    pub fn quantity_remaining(&self) -> Quantity {
        self.quantity_remaining
    }

    #[must_use]
    pub fn original_quantity(&self) -> Quantity {
        self.original_quantity
    }

    #[must_use]
    pub fn cost_remaining_canonical(&self) -> Option<String> {
        self.cost_remaining.map(canonical_decimal)
    }

    #[must_use]
    pub fn original_cost_canonical(&self) -> Option<String> {
        self.original_cost.map(canonical_decimal)
    }

    #[must_use]
    pub fn acquisition_fee_remaining_canonical(&self) -> Option<String> {
        self.acquisition_fee_remaining.map(canonical_decimal)
    }

    #[must_use]
    pub fn acquired_at(&self) -> &Timestamp {
        &self.acquired_at
    }

    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[must_use]
    pub fn basis(&self) -> BasisStatus {
        self.basis
    }

    #[must_use]
    pub fn cost_remaining(&self) -> Option<Decimal> {
        self.cost_remaining
    }

    #[must_use]
    pub fn original_cost(&self) -> Option<Decimal> {
        self.original_cost
    }

    #[must_use]
    pub fn original_acquisition_fee(&self) -> Option<Decimal> {
        self.original_acquisition_fee
    }

    #[must_use]
    pub fn cost_currency(&self) -> Option<super::currency::CurrencyCode> {
        self.cost_currency
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LotConsumption {
    lot_ref: LotRef,
    quantity_consumed: Quantity,
    consumed_cost: Option<Decimal>,
    allocated_acquisition_fee: Option<Decimal>,
    proceeds_share: Option<Decimal>,
    allocated_disposal_fee: Option<Decimal>,
    kind: ConsumptionKind,
    activity_id: ActivityId,
    instrument_id: InstrumentId,
    account_id: AccountId,
}

impl LotConsumption {
    #[must_use]
    pub fn lot_ref(&self) -> LotRef {
        self.lot_ref
    }

    #[must_use]
    pub fn quantity_consumed(&self) -> Quantity {
        self.quantity_consumed
    }

    #[must_use]
    pub fn consumed_cost_canonical(&self) -> Option<String> {
        self.consumed_cost.map(canonical_decimal)
    }

    #[must_use]
    pub fn allocated_acquisition_fee_canonical(&self) -> Option<String> {
        self.allocated_acquisition_fee.map(canonical_decimal)
    }

    #[must_use]
    pub fn proceeds_share_canonical(&self) -> Option<String> {
        self.proceeds_share.map(canonical_decimal)
    }

    #[must_use]
    pub fn allocated_disposal_fee_canonical(&self) -> Option<String> {
        self.allocated_disposal_fee.map(canonical_decimal)
    }

    #[must_use]
    pub fn kind(&self) -> ConsumptionKind {
        self.kind
    }

    #[must_use]
    pub fn consumed_cost(&self) -> Option<Decimal> {
        self.consumed_cost
    }

    #[must_use]
    pub fn allocated_acquisition_fee(&self) -> Option<Decimal> {
        self.allocated_acquisition_fee
    }

    #[must_use]
    pub fn proceeds_share(&self) -> Option<Decimal> {
        self.proceeds_share
    }

    #[must_use]
    pub fn allocated_disposal_fee(&self) -> Option<Decimal> {
        self.allocated_disposal_fee
    }

    #[must_use]
    pub fn activity_id(&self) -> ActivityId {
        self.activity_id
    }

    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerDiagnostic {
    QuantityShortfall {
        activity_id: ActivityId,
        instrument_id: InstrumentId,
        account_id: AccountId,
        requested: Quantity,
        available: Quantity,
    },
    UnexplainedDisposal {
        activity_id: ActivityId,
        lot_ref: LotRef,
        quantity: Quantity,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealizedGainTotals {
    pub consumed_cost: Decimal,
    pub proceeds: Decimal,
    pub realized_gain_gross: Decimal,
    pub allocated_fees: Decimal,
    pub realized_gain_net: Decimal,
}

impl RealizedGainTotals {
    #[must_use]
    pub fn consumed_cost_canonical(&self) -> String {
        canonical_decimal(self.consumed_cost)
    }

    #[must_use]
    pub fn proceeds_canonical(&self) -> String {
        canonical_decimal(self.proceeds)
    }

    #[must_use]
    pub fn realized_gain_gross_canonical(&self) -> String {
        canonical_decimal(self.realized_gain_gross)
    }

    #[must_use]
    pub fn allocated_fees_canonical(&self) -> String {
        canonical_decimal(self.allocated_fees)
    }

    #[must_use]
    pub fn realized_gain_net_canonical(&self) -> String {
        canonical_decimal(self.realized_gain_net)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LotLedger {
    open_lots: Vec<OpenLot>,
    openings: Vec<LotOpening>,
    consumptions: Vec<LotConsumption>,
    diagnostics: Vec<LedgerDiagnostic>,
    quantity_shortfall: bool,
}

impl LotLedger {
    #[must_use]
    pub fn open_lots(&self) -> &[OpenLot] {
        &self.open_lots
    }

    #[must_use]
    pub fn openings(&self) -> &[LotOpening] {
        &self.openings
    }

    #[must_use]
    pub fn opening(&self, lot_ref: LotRef) -> Option<&LotOpening> {
        self.openings
            .iter()
            .find(|opening| opening.lot_ref == lot_ref)
    }

    #[must_use]
    pub fn consumptions(&self) -> &[LotConsumption] {
        &self.consumptions
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[LedgerDiagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn has_quantity_shortfall(&self) -> bool {
        self.quantity_shortfall
    }

    pub fn realized_gain_totals(&self) -> Result<Option<RealizedGainTotals>, AppError> {
        if self.quantity_shortfall {
            return Ok(None);
        }
        let mut consumed_cost = Decimal::ZERO;
        let mut proceeds = Decimal::ZERO;
        let mut allocated_fees = Decimal::ZERO;
        let mut saw_realized = false;
        for consumption in &self.consumptions {
            if consumption.kind != ConsumptionKind::Realized {
                continue;
            }
            saw_realized = true;
            if let Some(cost) = consumption.consumed_cost {
                consumed_cost = checked_add(consumed_cost, cost)?;
            }
            if let Some(share) = consumption.proceeds_share {
                proceeds = checked_add(proceeds, share)?;
            }
            if let Some(fee) = consumption.allocated_acquisition_fee {
                allocated_fees = checked_add(allocated_fees, fee)?;
            }
            if let Some(fee) = consumption.allocated_disposal_fee {
                allocated_fees = checked_add(allocated_fees, fee)?;
            }
        }
        if !saw_realized {
            return Ok(None);
        }
        let realized_gain_gross = checked_sub(proceeds, consumed_cost)?;
        let realized_gain_net = checked_sub(realized_gain_gross, allocated_fees)?;
        Ok(Some(RealizedGainTotals {
            consumed_cost,
            proceeds,
            realized_gain_gross,
            allocated_fees,
            realized_gain_net,
        }))
    }
}

struct InternalLot {
    lot_ref: LotRef,
    remaining_qty: Decimal,
    original_qty: Decimal,
    remaining_cost: Option<Decimal>,
    original_cost: Option<Decimal>,
    remaining_acq_fee: Option<Decimal>,
    original_acq_fee: Option<Decimal>,
    cost_currency: Option<super::currency::CurrencyCode>,
    acquired_at: Timestamp,
    opening_created_at: Timestamp,
    opening_leg_id: Uuid,
    account_id: AccountId,
    instrument_id: InstrumentId,
    basis: BasisStatus,
}

struct PlannedSlice {
    index: usize,
    qty: Decimal,
}

struct AllocatedSlice {
    lot_ref: LotRef,
    qty: Decimal,
    consumed_cost: Option<Decimal>,
    allocated_acq_fee: Option<Decimal>,
    proceeds_share: Option<Decimal>,
    allocated_disposal_fee: Option<Decimal>,
    acquired_at: Timestamp,
    opening_created_at: Timestamp,
    opening_leg_id: Uuid,
    original_qty: Decimal,
    original_cost: Option<Decimal>,
    original_acq_fee: Option<Decimal>,
    cost_currency: Option<super::currency::CurrencyCode>,
    basis: BasisStatus,
    instrument_id: InstrumentId,
}

/// Replay `events` into open lots, consumptions, and diagnostics.
pub fn replay(events: impl IntoIterator<Item = LedgerEvent>) -> Result<LotLedger, AppError> {
    let mut events: Vec<LedgerEvent> = events.into_iter().collect();
    let excluded = excluded_activity_ids(&events);
    events.retain(|event| match event {
        LedgerEvent::OriginBaseline { .. } => true,
        LedgerEvent::Activity(activity) => !excluded.contains(&activity.activity_id),
    });
    events.sort_by_key(event_sort_key);

    let mut lots: Vec<InternalLot> = Vec::new();
    let mut openings: Vec<LotOpening> = Vec::new();
    let mut consumptions: Vec<LotConsumption> = Vec::new();
    let mut diagnostics: Vec<LedgerDiagnostic> = Vec::new();
    let mut quantity_shortfall = false;

    for event in events {
        if quantity_shortfall {
            break;
        }
        match event {
            LedgerEvent::OriginBaseline {
                holding_id,
                instrument_id,
                account_id,
                quantity,
                origin_at,
                ..
            } => {
                open_unknown_lot(
                    &mut lots,
                    &mut openings,
                    LotRef::OriginHolding(holding_id),
                    quantity,
                    origin_at.clone(),
                    origin_at,
                    holding_id.as_uuid(),
                    account_id,
                    instrument_id,
                );
            }
            LedgerEvent::Activity(activity) => {
                apply_activity(
                    &mut lots,
                    &mut openings,
                    &mut consumptions,
                    &mut diagnostics,
                    &mut quantity_shortfall,
                    activity,
                )?;
            }
        }
    }

    let mut open_lots = Vec::new();
    for lot in lots {
        if lot.remaining_qty.is_zero() {
            continue;
        }
        open_lots.push(OpenLot {
            lot_ref: lot.lot_ref,
            quantity_remaining: Quantity::from_canonical(lot.remaining_qty)?,
            original_quantity: Quantity::from_canonical(lot.original_qty)?,
            cost_remaining: lot.remaining_cost,
            original_cost: lot.original_cost,
            acquisition_fee_remaining: lot.remaining_acq_fee,
            original_acquisition_fee: lot.original_acq_fee,
            cost_currency: lot.cost_currency,
            acquired_at: lot.acquired_at,
            account_id: lot.account_id,
            instrument_id: lot.instrument_id,
            basis: lot.basis,
        });
    }
    open_lots.sort_by(|left, right| {
        left.acquired_at
            .cmp(&right.acquired_at)
            .then(left.lot_ref.cmp(&right.lot_ref))
            .then(left.account_id.as_uuid().cmp(&right.account_id.as_uuid()))
    });

    openings.sort_by(|left, right| {
        left.acquired_at
            .cmp(&right.acquired_at)
            .then(left.lot_ref.cmp(&right.lot_ref))
    });

    Ok(LotLedger {
        open_lots,
        openings,
        consumptions,
        diagnostics,
        quantity_shortfall,
    })
}

fn excluded_activity_ids(events: &[LedgerEvent]) -> HashSet<ActivityId> {
    let mut excluded = HashSet::new();
    for event in events {
        if let LedgerEvent::Activity(activity) = event {
            if let Some(original) = activity.reverses {
                excluded.insert(activity.activity_id);
                excluded.insert(original);
            }
            if let Some(reversal) = activity.reversed_by {
                excluded.insert(activity.activity_id);
                excluded.insert(reversal);
            }
        }
    }
    excluded
}

fn event_sort_key(event: &LedgerEvent) -> (Timestamp, Timestamp, Uuid, i64, Uuid) {
    match event {
        LedgerEvent::OriginBaseline {
            origin_id,
            holding_id,
            origin_at,
            ..
        } => (
            origin_at.clone(),
            origin_at.clone(),
            origin_id.as_uuid(),
            0,
            holding_id.as_uuid(),
        ),
        LedgerEvent::Activity(activity) => {
            let component = match &activity.effect {
                LotEffect::Buy { holding_leg_id, .. }
                | LotEffect::Sell { holding_leg_id, .. }
                | LotEffect::OpeningIncrease { holding_leg_id, .. }
                | LotEffect::PositionIncrease { holding_leg_id, .. }
                | LotEffect::PositionDecrease { holding_leg_id, .. } => holding_leg_id.as_uuid(),
                LotEffect::PositionTransfer { source_leg_id, .. } => source_leg_id.as_uuid(),
                LotEffect::None => Uuid::nil(),
            };
            (
                activity.effective_at.clone(),
                activity.created_at.clone(),
                activity.activity_id.as_uuid(),
                activity.sort_order,
                component,
            )
        }
    }
}

fn apply_activity(
    lots: &mut Vec<InternalLot>,
    openings: &mut Vec<LotOpening>,
    consumptions: &mut Vec<LotConsumption>,
    diagnostics: &mut Vec<LedgerDiagnostic>,
    quantity_shortfall: &mut bool,
    activity: ActivityLedgerEvent,
) -> Result<(), AppError> {
    match activity.effect {
        LotEffect::None => Ok(()),
        LotEffect::Buy {
            holding_leg_id,
            instrument_id,
            account_id,
            quantity,
            gross_settlement,
            acquisition_fee,
        } => {
            open_known_lot(
                lots,
                openings,
                LotRef::Acquisition(holding_leg_id),
                quantity,
                activity.effective_at,
                activity.created_at,
                holding_leg_id.as_uuid(),
                account_id,
                instrument_id,
                gross_settlement,
                acquisition_fee,
            )?;
            Ok(())
        }
        LotEffect::OpeningIncrease {
            holding_leg_id,
            instrument_id,
            account_id,
            quantity,
        }
        | LotEffect::PositionIncrease {
            holding_leg_id,
            instrument_id,
            account_id,
            quantity,
        } => {
            open_unknown_lot(
                lots,
                openings,
                LotRef::Acquisition(holding_leg_id),
                quantity,
                activity.effective_at,
                activity.created_at,
                holding_leg_id.as_uuid(),
                account_id,
                instrument_id,
            );
            Ok(())
        }
        LotEffect::Sell {
            instrument_id,
            account_id,
            quantity,
            proceeds_gross,
            disposal_fee,
            ..
        } => consume_disposal(
            lots,
            consumptions,
            diagnostics,
            quantity_shortfall,
            activity.activity_id,
            instrument_id,
            account_id,
            quantity,
            proceeds_gross,
            disposal_fee,
            ConsumptionKind::Realized,
        ),
        LotEffect::PositionDecrease {
            instrument_id,
            account_id,
            quantity,
            ..
        } => consume_disposal(
            lots,
            consumptions,
            diagnostics,
            quantity_shortfall,
            activity.activity_id,
            instrument_id,
            account_id,
            quantity,
            None,
            None,
            ConsumptionKind::UnexplainedDisposal,
        ),
        LotEffect::PositionTransfer {
            instrument_id,
            source_account_id,
            destination_account_id,
            quantity,
            ..
        } => apply_transfer(
            lots,
            consumptions,
            diagnostics,
            quantity_shortfall,
            activity.activity_id,
            instrument_id,
            source_account_id,
            destination_account_id,
            quantity,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn open_known_lot(
    lots: &mut Vec<InternalLot>,
    openings: &mut Vec<LotOpening>,
    lot_ref: LotRef,
    quantity: Quantity,
    acquired_at: Timestamp,
    opening_created_at: Timestamp,
    opening_leg_id: Uuid,
    account_id: AccountId,
    instrument_id: InstrumentId,
    gross_settlement: Option<Money>,
    acquisition_fee: Option<Money>,
) -> Result<(), AppError> {
    if quantity.is_zero() {
        return Ok(());
    }
    let (cost, cost_currency) = match gross_settlement {
        Some(money) => (money.amount(), Some(money.currency())),
        None => (
            Decimal::ZERO,
            acquisition_fee.map(super::money::Money::currency),
        ),
    };
    let fee = match acquisition_fee {
        Some(money) => {
            if let Some(currency) = cost_currency {
                if money.currency() != currency {
                    return Err(AppError::validation(
                        "currency",
                        "Acquisition fee currency must match the settlement currency.",
                    ));
                }
            }
            money.amount()
        }
        None => Decimal::ZERO,
    };
    openings.push(LotOpening {
        lot_ref,
        instrument_id,
        acquired_at: acquired_at.clone(),
        basis: BasisStatus::Known,
    });
    lots.push(InternalLot {
        lot_ref,
        remaining_qty: quantity.amount(),
        original_qty: quantity.amount(),
        remaining_cost: Some(cost),
        original_cost: Some(cost),
        remaining_acq_fee: Some(fee),
        original_acq_fee: Some(fee),
        cost_currency,
        acquired_at,
        opening_created_at,
        opening_leg_id,
        account_id,
        instrument_id,
        basis: BasisStatus::Known,
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn open_unknown_lot(
    lots: &mut Vec<InternalLot>,
    openings: &mut Vec<LotOpening>,
    lot_ref: LotRef,
    quantity: Quantity,
    acquired_at: Timestamp,
    opening_created_at: Timestamp,
    opening_leg_id: Uuid,
    account_id: AccountId,
    instrument_id: InstrumentId,
) {
    if quantity.is_zero() {
        return;
    }
    openings.push(LotOpening {
        lot_ref,
        instrument_id,
        acquired_at: acquired_at.clone(),
        basis: BasisStatus::Unknown,
    });
    lots.push(InternalLot {
        lot_ref,
        remaining_qty: quantity.amount(),
        original_qty: quantity.amount(),
        remaining_cost: None,
        original_cost: None,
        remaining_acq_fee: None,
        original_acq_fee: None,
        cost_currency: None,
        acquired_at,
        opening_created_at,
        opening_leg_id,
        account_id,
        instrument_id,
        basis: BasisStatus::Unknown,
    });
}

#[allow(clippy::too_many_arguments)]
fn consume_disposal(
    lots: &mut [InternalLot],
    consumptions: &mut Vec<LotConsumption>,
    diagnostics: &mut Vec<LedgerDiagnostic>,
    quantity_shortfall: &mut bool,
    activity_id: ActivityId,
    instrument_id: InstrumentId,
    account_id: AccountId,
    quantity: Quantity,
    proceeds_gross: Option<Money>,
    disposal_fee: Option<Money>,
    kind: ConsumptionKind,
) -> Result<(), AppError> {
    let slices = match take_fifo(
        lots,
        instrument_id,
        account_id,
        quantity,
        proceeds_gross,
        disposal_fee,
        kind,
    )? {
        Ok(slices) => slices,
        Err(available) => {
            *quantity_shortfall = true;
            diagnostics.push(LedgerDiagnostic::QuantityShortfall {
                activity_id,
                instrument_id,
                account_id,
                requested: quantity,
                available: Quantity::from_canonical(available)?,
            });
            return Ok(());
        }
    };
    for slice in slices {
        if kind == ConsumptionKind::UnexplainedDisposal {
            diagnostics.push(LedgerDiagnostic::UnexplainedDisposal {
                activity_id,
                lot_ref: slice.lot_ref,
                quantity: Quantity::from_canonical(slice.qty)?,
            });
        }
        consumptions.push(consumption_from_slice(
            &slice,
            kind,
            activity_id,
            instrument_id,
            account_id,
        )?);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_transfer(
    lots: &mut Vec<InternalLot>,
    consumptions: &mut Vec<LotConsumption>,
    diagnostics: &mut Vec<LedgerDiagnostic>,
    quantity_shortfall: &mut bool,
    activity_id: ActivityId,
    instrument_id: InstrumentId,
    source_account_id: AccountId,
    destination_account_id: AccountId,
    quantity: Quantity,
) -> Result<(), AppError> {
    let slices = match take_fifo(
        lots,
        instrument_id,
        source_account_id,
        quantity,
        None,
        None,
        ConsumptionKind::Transfer,
    )? {
        Ok(slices) => slices,
        Err(available) => {
            *quantity_shortfall = true;
            diagnostics.push(LedgerDiagnostic::QuantityShortfall {
                activity_id,
                instrument_id,
                account_id: source_account_id,
                requested: quantity,
                available: Quantity::from_canonical(available)?,
            });
            return Ok(());
        }
    };
    for slice in slices {
        reopen_at(lots, destination_account_id, &slice)?;
        consumptions.push(consumption_from_slice(
            &slice,
            ConsumptionKind::Transfer,
            activity_id,
            instrument_id,
            source_account_id,
        )?);
    }
    Ok(())
}

fn consumption_from_slice(
    slice: &AllocatedSlice,
    kind: ConsumptionKind,
    activity_id: ActivityId,
    instrument_id: InstrumentId,
    account_id: AccountId,
) -> Result<LotConsumption, AppError> {
    Ok(LotConsumption {
        lot_ref: slice.lot_ref,
        quantity_consumed: Quantity::from_canonical(slice.qty)?,
        consumed_cost: slice.consumed_cost,
        allocated_acquisition_fee: slice.allocated_acq_fee,
        proceeds_share: slice.proceeds_share,
        allocated_disposal_fee: slice.allocated_disposal_fee,
        kind,
        activity_id,
        instrument_id,
        account_id,
    })
}

#[allow(clippy::too_many_arguments)]
fn take_fifo(
    lots: &mut [InternalLot],
    instrument_id: InstrumentId,
    account_id: AccountId,
    quantity: Quantity,
    proceeds_gross: Option<Money>,
    disposal_fee: Option<Money>,
    kind: ConsumptionKind,
) -> Result<Result<Vec<AllocatedSlice>, Decimal>, AppError> {
    let requested = quantity.amount();
    if requested.is_zero() {
        return Ok(Ok(Vec::new()));
    }
    let planned = match plan_consumption(lots, instrument_id, account_id, requested) {
        Ok(planned) => planned,
        Err(available) => return Ok(Err(available)),
    };

    let proceeds_total = proceeds_gross.map(Money::amount).unwrap_or(Decimal::ZERO);
    let disposal_fee_total = disposal_fee.map(Money::amount).unwrap_or(Decimal::ZERO);
    let mut remaining_proceeds = proceeds_total;
    let mut remaining_disposal_fee = disposal_fee_total;
    let mut allocated = Vec::new();

    for (index, slice) in planned.iter().enumerate() {
        let last_of_disposal = index + 1 == planned.len();
        let lot = &lots[slice.index];
        let last_of_lot = slice.qty == lot.remaining_qty;
        let consumed_cost = allocate_lot_amount(
            lot.basis,
            lot.original_cost,
            lot.remaining_cost,
            slice.qty,
            lot.original_qty,
            last_of_lot,
        )?;
        let allocated_acq_fee = allocate_lot_amount(
            lot.basis,
            lot.original_acq_fee,
            lot.remaining_acq_fee,
            slice.qty,
            lot.original_qty,
            last_of_lot,
        )?;
        let include_proceeds = kind == ConsumptionKind::Realized;
        let proceeds_share = if include_proceeds {
            Some(if last_of_disposal {
                remaining_proceeds
            } else {
                allocate_share(proceeds_total, slice.qty, requested)?
            })
        } else {
            None
        };
        let allocated_disposal_fee = if include_proceeds {
            Some(if last_of_disposal {
                remaining_disposal_fee
            } else {
                allocate_share(disposal_fee_total, slice.qty, requested)?
            })
        } else {
            None
        };
        if let Some(share) = proceeds_share {
            remaining_proceeds = checked_sub(remaining_proceeds, share)?;
        }
        if let Some(fee) = allocated_disposal_fee {
            remaining_disposal_fee = checked_sub(remaining_disposal_fee, fee)?;
        }
        allocated.push(AllocatedSlice {
            lot_ref: lot.lot_ref,
            qty: slice.qty,
            consumed_cost,
            allocated_acq_fee,
            proceeds_share,
            allocated_disposal_fee,
            acquired_at: lot.acquired_at.clone(),
            opening_created_at: lot.opening_created_at.clone(),
            opening_leg_id: lot.opening_leg_id,
            original_qty: lot.original_qty,
            original_cost: lot.original_cost,
            original_acq_fee: lot.original_acq_fee,
            cost_currency: lot.cost_currency,
            basis: lot.basis,
            instrument_id: lot.instrument_id,
        });
    }

    for (planned_slice, allocated_slice) in planned.iter().zip(allocated.iter()) {
        apply_slice(&mut lots[planned_slice.index], allocated_slice)?;
    }
    Ok(Ok(allocated))
}

fn plan_consumption(
    lots: &[InternalLot],
    instrument_id: InstrumentId,
    account_id: AccountId,
    requested: Decimal,
) -> Result<Vec<PlannedSlice>, Decimal> {
    let mut indices: Vec<usize> = lots
        .iter()
        .enumerate()
        .filter(|(_, lot)| {
            lot.instrument_id == instrument_id
                && lot.account_id == account_id
                && !lot.remaining_qty.is_zero()
        })
        .map(|(index, _)| index)
        .collect();
    indices.sort_by(|&left, &right| fifo_cmp(&lots[left], &lots[right]));

    let available = indices
        .iter()
        .try_fold(Decimal::ZERO, |sum, &index| {
            checked_add(sum, lots[index].remaining_qty)
        })
        .unwrap_or(Decimal::ZERO);
    if available < requested {
        return Err(available);
    }

    let mut remaining = requested;
    let mut planned = Vec::new();
    for index in indices {
        if remaining.is_zero() {
            break;
        }
        let take = lots[index].remaining_qty.min(remaining);
        planned.push(PlannedSlice { index, qty: take });
        remaining = checked_sub(remaining, take).unwrap_or(Decimal::ZERO);
    }
    Ok(planned)
}

fn fifo_cmp(left: &InternalLot, right: &InternalLot) -> Ordering {
    left.acquired_at
        .cmp(&right.acquired_at)
        .then(left.opening_created_at.cmp(&right.opening_created_at))
        .then(left.opening_leg_id.cmp(&right.opening_leg_id))
        .then(left.lot_ref.cmp(&right.lot_ref))
}

fn allocate_share(total: Decimal, share: Decimal, whole: Decimal) -> Result<Decimal, AppError> {
    checked_div(checked_mul(total, share)?, whole)
}

fn allocate_lot_amount(
    basis: BasisStatus,
    original: Option<Decimal>,
    remaining: Option<Decimal>,
    qty: Decimal,
    original_qty: Decimal,
    last_of_lot: bool,
) -> Result<Option<Decimal>, AppError> {
    if basis == BasisStatus::Unknown {
        return Ok(None);
    }
    if last_of_lot {
        return Ok(remaining);
    }
    let original = original.unwrap_or(Decimal::ZERO);
    Ok(Some(allocate_share(original, qty, original_qty)?))
}

fn apply_slice(lot: &mut InternalLot, slice: &AllocatedSlice) -> Result<(), AppError> {
    lot.remaining_qty = checked_sub(lot.remaining_qty, slice.qty)?;
    if let (Some(remaining), Some(consumed)) = (lot.remaining_cost, slice.consumed_cost) {
        lot.remaining_cost = Some(checked_sub(remaining, consumed)?);
    }
    if let (Some(remaining), Some(consumed)) = (lot.remaining_acq_fee, slice.allocated_acq_fee) {
        lot.remaining_acq_fee = Some(checked_sub(remaining, consumed)?);
    }
    if lot.remaining_qty.is_zero() && lot.basis == BasisStatus::Known {
        lot.remaining_cost = Some(Decimal::ZERO);
        lot.remaining_acq_fee = Some(Decimal::ZERO);
    }
    Ok(())
}

fn reopen_at(
    lots: &mut Vec<InternalLot>,
    destination_account_id: AccountId,
    slice: &AllocatedSlice,
) -> Result<(), AppError> {
    if let Some(existing) = lots.iter_mut().find(|lot| {
        lot.lot_ref == slice.lot_ref
            && lot.account_id == destination_account_id
            && !lot.remaining_qty.is_zero()
    }) {
        existing.remaining_qty = checked_add(existing.remaining_qty, slice.qty)?;
        if let (Some(remaining), Some(added)) = (existing.remaining_cost, slice.consumed_cost) {
            existing.remaining_cost = Some(checked_add(remaining, added)?);
        }
        if let (Some(remaining), Some(added)) =
            (existing.remaining_acq_fee, slice.allocated_acq_fee)
        {
            existing.remaining_acq_fee = Some(checked_add(remaining, added)?);
        }
        return Ok(());
    }
    lots.push(InternalLot {
        lot_ref: slice.lot_ref,
        remaining_qty: slice.qty,
        original_qty: slice.original_qty,
        remaining_cost: slice.consumed_cost,
        original_cost: slice.original_cost,
        remaining_acq_fee: slice.allocated_acq_fee,
        original_acq_fee: slice.original_acq_fee,
        cost_currency: slice.cost_currency,
        acquired_at: slice.acquired_at.clone(),
        opening_created_at: slice.opening_created_at.clone(),
        opening_leg_id: slice.opening_leg_id,
        account_id: destination_account_id,
        instrument_id: slice.instrument_id,
        basis: slice.basis,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        replay, ActivityLedgerEvent, BasisStatus, ConsumptionKind, LedgerDiagnostic, LedgerEvent,
        LotEffect, LotOpening, LotRef,
    };
    use crate::domain::currency::CurrencyCode;
    use crate::domain::ids::{
        AccountId, ActivityId, ActivityLegId, HistoryOriginId, HoldingId, InstrumentId,
    };
    use crate::domain::money::Money;
    use crate::domain::quantity::Quantity;
    use crate::domain::time::Timestamp;

    fn ts(value: &str) -> Timestamp {
        Timestamp::parse(value).expect("timestamp")
    }

    fn qty(value: &str) -> Quantity {
        Quantity::parse(value).expect("quantity")
    }

    fn usd(value: &str) -> Money {
        Money::parse(value, CurrencyCode::USD).expect("money")
    }

    fn activity_id(value: &str) -> ActivityId {
        ActivityId::parse(value).expect("activity id")
    }

    fn leg_id(value: &str) -> ActivityLegId {
        ActivityLegId::parse(value).expect("leg id")
    }

    fn account_id(value: &str) -> AccountId {
        AccountId::parse(value).expect("account id")
    }

    fn instrument_id(value: &str) -> InstrumentId {
        InstrumentId::parse(value).expect("instrument id")
    }

    fn holding_id(value: &str) -> HoldingId {
        HoldingId::parse(value).expect("holding id")
    }

    const BROKERAGE: &str = "99999999-9999-4999-8999-999999999999";
    const VOO: &str = "25252525-2525-4252-8252-252525252525";
    const BUY1_ACTIVITY: &str = "01a0188f-861c-7b20-8609-535e345b7c42";
    const BUY1_LEG: &str = "01a0188f-861c-7b20-8609-5363bbc99c48";
    const BUY2_ACTIVITY: &str = "01a0188f-861e-7e70-930b-5f4e2d6cda2d";
    const BUY2_LEG: &str = "01a0188f-861e-7e70-930b-5f578c9baeea";
    const SELL_ACTIVITY: &str = "01a0188f-861f-7c20-83d1-4abb57f8ddc0";
    const SELL_LEG: &str = "01a0188f-861f-7c20-83d1-4ac8ea0f6396";

    fn activity_event(
        activity: &str,
        created_at: &str,
        effective_at: &str,
        effect: LotEffect,
    ) -> LedgerEvent {
        LedgerEvent::Activity(ActivityLedgerEvent {
            activity_id: activity_id(activity),
            created_at: ts(created_at),
            effective_at: ts(effective_at),
            reverses: None,
            reversed_by: None,
            sort_order: 0,
            effect,
        })
    }

    fn golden_fifo_events() -> Vec<LedgerEvent> {
        vec![
            activity_event(
                BUY1_ACTIVITY,
                "2026-01-04T02:00:00.000Z",
                "2026-01-04T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY1_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    gross_settlement: Some(usd("200")),
                    acquisition_fee: Some(usd("5")),
                },
            ),
            activity_event(
                BUY2_ACTIVITY,
                "2026-01-05T02:00:00.000Z",
                "2026-01-05T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY2_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("4"),
                    gross_settlement: Some(usd("480")),
                    acquisition_fee: Some(usd("6")),
                },
            ),
            activity_event(
                SELL_ACTIVITY,
                "2026-01-06T02:00:00.000Z",
                "2026-01-06T02:00:00.000Z",
                LotEffect::Sell {
                    holding_leg_id: leg_id(SELL_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("4"),
                    proceeds_gross: Some(usd("600")),
                    disposal_fee: Some(usd("6")),
                },
            ),
        ]
    }

    #[test]
    fn golden_fifo_realized_gain() {
        let ledger = replay(golden_fifo_events()).expect("replay");
        let gain = ledger
            .realized_gain_totals()
            .expect("totals")
            .expect("realized");
        assert_eq!(gain.consumed_cost_canonical(), "440");
        assert_eq!(gain.proceeds_canonical(), "600");
        assert_eq!(gain.realized_gain_gross_canonical(), "160");
        assert_eq!(gain.allocated_fees_canonical(), "14");
        assert_eq!(gain.realized_gain_net_canonical(), "146");
        assert_eq!(ledger.open_lots().len(), 1);
        let open = &ledger.open_lots()[0];
        assert_eq!(open.lot_ref(), LotRef::Acquisition(leg_id(BUY2_LEG)));
        assert_eq!(open.quantity_remaining().canonical(), "2");
        assert_eq!(open.cost_remaining_canonical().as_deref(), Some("240"));
        assert_eq!(open.basis(), BasisStatus::Known);
        assert_eq!(ledger.consumptions().len(), 2);
        assert_eq!(
            ledger.consumptions()[0]
                .consumed_cost_canonical()
                .as_deref(),
            Some("200")
        );
        assert_eq!(
            ledger.consumptions()[1]
                .consumed_cost_canonical()
                .as_deref(),
            Some("240")
        );
        assert_eq!(
            ledger.consumptions()[0]
                .allocated_acquisition_fee_canonical()
                .as_deref(),
            Some("5")
        );
        assert_eq!(
            ledger.consumptions()[1]
                .allocated_acquisition_fee_canonical()
                .as_deref(),
            Some("3")
        );
    }

    #[test]
    fn replay_is_deterministic_across_repeats_and_shuffled_input() {
        let events = golden_fifo_events();
        let first = replay(events.clone()).expect("first");
        let second = replay(events.clone()).expect("second");
        assert_eq!(first, second);
        let mut shuffled = events;
        shuffled.reverse();
        let shuffled_ledger = replay(shuffled).expect("shuffled");
        assert_eq!(first, shuffled_ledger);
    }

    #[test]
    fn reversed_buy_and_reversal_leave_ledger_untouched() {
        let original = "01a0188f-8622-7d80-a656-dc56157ac0e8";
        let reversal = "01a0188f-8623-7ba0-b1ee-7a03d98263c6";
        let events = vec![
            LedgerEvent::Activity(ActivityLedgerEvent {
                activity_id: activity_id(original),
                created_at: ts("2026-01-07T02:00:00.000Z"),
                effective_at: ts("2026-01-07T02:00:00.000Z"),
                reverses: None,
                reversed_by: Some(activity_id(reversal)),
                sort_order: 0,
                effect: LotEffect::Buy {
                    holding_leg_id: leg_id("01a0188f-8622-7d80-a656-dc56157ac0e9"),
                    instrument_id: instrument_id("23232323-2323-4232-8232-232323232323"),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("1"),
                    gross_settlement: Some(usd("100")),
                    acquisition_fee: None,
                },
            }),
            LedgerEvent::Activity(ActivityLedgerEvent {
                activity_id: activity_id(reversal),
                created_at: ts("2026-01-07T03:00:00.000Z"),
                effective_at: ts("2026-01-07T03:00:00.000Z"),
                reverses: Some(activity_id(original)),
                reversed_by: None,
                sort_order: 0,
                effect: LotEffect::None,
            }),
        ];
        let ledger = replay(events).expect("replay");
        assert!(ledger.open_lots().is_empty());
        assert!(ledger.consumptions().is_empty());
    }

    #[test]
    fn correction_replacement_opens_the_only_lot() {
        let original = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1";
        let reversal = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2";
        let replacement = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3";
        let replacement_leg = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb3";
        let events = vec![
            LedgerEvent::Activity(ActivityLedgerEvent {
                activity_id: activity_id(original),
                created_at: ts("2026-01-08T02:00:00.000Z"),
                effective_at: ts("2026-01-08T02:00:00.000Z"),
                reverses: None,
                reversed_by: Some(activity_id(reversal)),
                sort_order: 0,
                effect: LotEffect::Buy {
                    holding_leg_id: leg_id("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb1"),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    gross_settlement: Some(usd("200")),
                    acquisition_fee: None,
                },
            }),
            LedgerEvent::Activity(ActivityLedgerEvent {
                activity_id: activity_id(reversal),
                created_at: ts("2026-01-08T03:00:00.000Z"),
                effective_at: ts("2026-01-08T03:00:00.000Z"),
                reverses: Some(activity_id(original)),
                reversed_by: None,
                sort_order: 0,
                effect: LotEffect::None,
            }),
            activity_event(
                replacement,
                "2026-01-08T04:00:00.000Z",
                "2026-01-08T04:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(replacement_leg),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    gross_settlement: Some(usd("220")),
                    acquisition_fee: None,
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        assert_eq!(ledger.open_lots().len(), 1);
        assert_eq!(
            ledger.open_lots()[0].lot_ref(),
            LotRef::Acquisition(leg_id(replacement_leg))
        );
        assert_eq!(
            ledger.open_lots()[0].cost_remaining_canonical().as_deref(),
            Some("220")
        );
    }

    #[test]
    fn position_transfer_preserves_lot_ref_cost_time_basis_and_zero_gain() {
        let buy_leg = "cccccccc-cccc-4ccc-8ccc-ccccccccccc1";
        let dest = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
        let acquired = "2026-01-09T02:00:00.000Z";
        let events = vec![
            activity_event(
                "cccccccc-cccc-4ccc-8ccc-cccccccccca1",
                acquired,
                acquired,
                LotEffect::Buy {
                    holding_leg_id: leg_id(buy_leg),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    gross_settlement: Some(usd("200")),
                    acquisition_fee: Some(usd("5")),
                },
            ),
            activity_event(
                "cccccccc-cccc-4ccc-8ccc-cccccccccca2",
                "2026-01-10T02:00:00.000Z",
                "2026-01-10T02:00:00.000Z",
                LotEffect::PositionTransfer {
                    source_leg_id: leg_id("cccccccc-cccc-4ccc-8ccc-ccccccccccc2"),
                    destination_leg_id: leg_id("cccccccc-cccc-4ccc-8ccc-ccccccccccc3"),
                    instrument_id: instrument_id(VOO),
                    source_account_id: account_id(BROKERAGE),
                    destination_account_id: account_id(dest),
                    quantity: qty("2"),
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        assert_eq!(ledger.open_lots().len(), 1);
        let open = &ledger.open_lots()[0];
        assert_eq!(open.lot_ref(), LotRef::Acquisition(leg_id(buy_leg)));
        assert_eq!(open.account_id(), account_id(dest));
        assert_eq!(open.cost_remaining_canonical().as_deref(), Some("200"));
        assert_eq!(open.acquired_at(), &ts(acquired));
        assert_eq!(open.basis(), BasisStatus::Known);
        assert_eq!(ledger.consumptions().len(), 1);
        assert_eq!(ledger.consumptions()[0].kind(), ConsumptionKind::Transfer);
        assert!(ledger.realized_gain_totals().expect("totals").is_none());
    }

    #[test]
    fn origin_opening_and_position_increase_open_unknown_basis_lots() {
        let origin_holding = "30303030-3030-4303-8303-303030303030";
        let opening_leg = "dddddddd-dddd-4ddd-8ddd-ddddddddddd1";
        let increase_leg = "dddddddd-dddd-4ddd-8ddd-ddddddddddd2";
        let qqq = "20202020-2020-4202-8202-202020202020";
        let events = vec![
            LedgerEvent::OriginBaseline {
                origin_id: HistoryOriginId::parse("a0a0a0a0-a0a0-4a0a-8a0a-a0a0a0a0a0a0")
                    .expect("origin"),
                holding_id: holding_id(origin_holding),
                instrument_id: instrument_id(qqq),
                account_id: account_id(BROKERAGE),
                quantity: qty("3"),
                origin_at: ts("2026-01-02T00:00:00.000Z"),
            },
            activity_event(
                "dddddddd-dddd-4ddd-8ddd-dddddddddda1",
                "2026-01-03T02:00:00.000Z",
                "2026-01-03T02:00:00.000Z",
                LotEffect::OpeningIncrease {
                    holding_leg_id: leg_id(opening_leg),
                    instrument_id: instrument_id(qqq),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("1"),
                },
            ),
            activity_event(
                "dddddddd-dddd-4ddd-8ddd-dddddddddda2",
                "2026-01-04T02:00:00.000Z",
                "2026-01-04T02:00:00.000Z",
                LotEffect::PositionIncrease {
                    holding_leg_id: leg_id(increase_leg),
                    instrument_id: instrument_id(qqq),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        assert_eq!(ledger.open_lots().len(), 3);
        assert!(ledger
            .open_lots()
            .iter()
            .all(|lot| lot.basis() == BasisStatus::Unknown && lot.cost_remaining().is_none()));
        assert_eq!(
            ledger.open_lots()[0].lot_ref(),
            LotRef::OriginHolding(holding_id(origin_holding))
        );
        assert_eq!(
            ledger.open_lots()[1].lot_ref(),
            LotRef::Acquisition(leg_id(opening_leg))
        );
        assert_eq!(
            ledger.open_lots()[2].lot_ref(),
            LotRef::Acquisition(leg_id(increase_leg))
        );
        assert_eq!(ledger.openings().len(), 3);
        assert_eq!(
            ledger
                .opening(LotRef::OriginHolding(holding_id(origin_holding)))
                .map(LotOpening::basis),
            Some(BasisStatus::Unknown)
        );
    }

    #[test]
    fn position_adjustment_decrease_is_unexplained_disposal_with_zero_realized_gain() {
        let buy_leg = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeee1";
        let events = vec![
            activity_event(
                "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeea1",
                "2026-01-11T02:00:00.000Z",
                "2026-01-11T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(buy_leg),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    gross_settlement: Some(usd("200")),
                    acquisition_fee: Some(usd("5")),
                },
            ),
            activity_event(
                "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeea2",
                "2026-01-12T02:00:00.000Z",
                "2026-01-12T02:00:00.000Z",
                LotEffect::PositionDecrease {
                    holding_leg_id: leg_id("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeee2"),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        assert!(ledger.open_lots().is_empty());
        assert_eq!(ledger.consumptions().len(), 1);
        assert_eq!(
            ledger
                .opening(LotRef::Acquisition(leg_id(buy_leg)))
                .map(LotOpening::basis),
            Some(BasisStatus::Known)
        );
        assert_eq!(
            ledger.consumptions()[0].kind(),
            ConsumptionKind::UnexplainedDisposal
        );
        assert_eq!(
            ledger.consumptions()[0]
                .consumed_cost_canonical()
                .as_deref(),
            Some("200")
        );
        assert!(ledger.consumptions()[0].proceeds_share().is_none());
        assert!(matches!(
            ledger.diagnostics()[0],
            LedgerDiagnostic::UnexplainedDisposal { .. }
        ));
        assert!(ledger.realized_gain_totals().expect("totals").is_none());
    }

    #[test]
    fn zero_gross_buy_opens_known_basis_lot_with_cost_zero() {
        let zero_leg = "01a0188f-8621-7a61-a206-bf66455312f8";
        let events = vec![activity_event(
            "01a0188f-8621-7a61-a206-bf5800173c36",
            "2026-01-13T02:00:00.000Z",
            "2026-01-13T02:00:00.000Z",
            LotEffect::Buy {
                holding_leg_id: leg_id(zero_leg),
                instrument_id: instrument_id("26262626-2626-4262-8262-262626262626"),
                account_id: account_id(BROKERAGE),
                quantity: qty("1"),
                gross_settlement: None,
                acquisition_fee: None,
            },
        )];
        let ledger = replay(events).expect("replay");
        assert_eq!(ledger.open_lots().len(), 1);
        let open = &ledger.open_lots()[0];
        assert_eq!(open.basis(), BasisStatus::Known);
        assert_eq!(open.cost_remaining_canonical().as_deref(), Some("0"));
        assert_ne!(open.basis(), BasisStatus::Unknown);
        assert_eq!(open.lot_ref(), LotRef::Acquisition(leg_id(zero_leg)));
    }

    #[test]
    fn consumption_order_ties_break_by_acquired_at_created_at_leg_id() {
        let early_leg = "00000000-0000-4000-8000-000000000003";
        let mid_leg = "00000000-0000-4000-8000-000000000001";
        let late_leg = "00000000-0000-4000-8000-000000000002";
        let events = vec![
            activity_event(
                "00000000-0000-4000-8000-0000000000a3",
                "2026-01-14T13:00:00.000Z",
                "2026-01-14T09:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(early_leg),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("1"),
                    gross_settlement: Some(usd("10")),
                    acquisition_fee: None,
                },
            ),
            activity_event(
                "00000000-0000-4000-8000-0000000000a2",
                "2026-01-14T12:00:00.000Z",
                "2026-01-14T10:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(late_leg),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("1"),
                    gross_settlement: Some(usd("30")),
                    acquisition_fee: None,
                },
            ),
            activity_event(
                "00000000-0000-4000-8000-0000000000a1",
                "2026-01-14T11:00:00.000Z",
                "2026-01-14T10:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(mid_leg),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("1"),
                    gross_settlement: Some(usd("20")),
                    acquisition_fee: None,
                },
            ),
            activity_event(
                "00000000-0000-4000-8000-0000000000a4",
                "2026-01-15T02:00:00.000Z",
                "2026-01-15T02:00:00.000Z",
                LotEffect::Sell {
                    holding_leg_id: leg_id("00000000-0000-4000-8000-0000000000a5"),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("3"),
                    proceeds_gross: Some(usd("90")),
                    disposal_fee: None,
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        let refs: Vec<_> = ledger
            .consumptions()
            .iter()
            .map(super::LotConsumption::lot_ref)
            .collect();
        assert_eq!(
            refs,
            vec![
                LotRef::Acquisition(leg_id(early_leg)),
                LotRef::Acquisition(leg_id(mid_leg)),
                LotRef::Acquisition(leg_id(late_leg)),
            ]
        );
    }

    #[test]
    fn quantity_shortfall_returns_diagnostic_and_blocks_gain() {
        let events = vec![
            activity_event(
                BUY1_ACTIVITY,
                "2026-01-04T02:00:00.000Z",
                "2026-01-04T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY1_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("1"),
                    gross_settlement: Some(usd("100")),
                    acquisition_fee: None,
                },
            ),
            activity_event(
                SELL_ACTIVITY,
                "2026-01-06T02:00:00.000Z",
                "2026-01-06T02:00:00.000Z",
                LotEffect::Sell {
                    holding_leg_id: leg_id(SELL_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    proceeds_gross: Some(usd("200")),
                    disposal_fee: None,
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        assert!(ledger.has_quantity_shortfall());
        assert!(ledger.realized_gain_totals().expect("totals").is_none());
        assert!(matches!(
            ledger.diagnostics()[0],
            LedgerDiagnostic::QuantityShortfall { .. }
        ));
        assert_eq!(ledger.open_lots().len(), 1);
        assert_eq!(ledger.open_lots()[0].quantity_remaining().canonical(), "1");
        assert!(ledger.consumptions().is_empty());
    }

    #[test]
    fn income_fee_and_deposit_have_no_lot_effect() {
        let events = vec![
            activity_event(
                "ffffffff-ffff-4fff-8fff-fffffffffff1",
                "2026-01-16T02:00:00.000Z",
                "2026-01-16T02:00:00.000Z",
                LotEffect::None,
            ),
            activity_event(
                BUY1_ACTIVITY,
                "2026-01-04T02:00:00.000Z",
                "2026-01-04T02:00:00.000Z",
                LotEffect::Buy {
                    holding_leg_id: leg_id(BUY1_LEG),
                    instrument_id: instrument_id(VOO),
                    account_id: account_id(BROKERAGE),
                    quantity: qty("2"),
                    gross_settlement: Some(usd("200")),
                    acquisition_fee: None,
                },
            ),
        ];
        let ledger = replay(events).expect("replay");
        assert_eq!(ledger.open_lots().len(), 1);
        assert!(ledger.consumptions().is_empty());
    }
}
