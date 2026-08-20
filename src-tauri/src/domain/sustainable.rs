use std::fmt;

use chrono::{Datelike, Days, Months, NaiveDate};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};

use super::{
    activity::{ActivityKind, FeeKind, IncomeKind},
    activity_leg::{ComponentKind, MonetaryEndpoint, QuantityEndpoint},
    decimal::{canonical_decimal, checked_mul, parse_canonical_decimal, DecimalSyntax},
    fx::FxRate,
    ids::{AccountId, HoldingId, InstrumentId},
    money::Money,
    quantity::Quantity,
    time::CalendarDate,
    unit_price::UnitPrice,
};
use crate::error::AppError;

pub const MAX_RECURRENCE_OCCURRENCES: usize = 366;
const BENCHMARK_LEVEL_MAX_INTEGER_DIGITS: usize = 18;
const BENCHMARK_LEVEL_MAX_FRACTIONAL_DIGITS: usize = 8;
const CANONICAL_IMPORT_PREFIX: &[u8] = b"nestworth-import-row\0v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchmarkLevel(Decimal);

impl BenchmarkLevel {
    pub fn parse(input: &str) -> Result<Self, AppError> {
        let value = parse_canonical_decimal(
            input,
            DecimalSyntax {
                max_integer_digits: BENCHMARK_LEVEL_MAX_INTEGER_DIGITS,
                max_fractional_digits: BENCHMARK_LEVEL_MAX_FRACTIONAL_DIGITS,
                allow_zero: false,
            },
        )
        .map_err(|_| AppError::invalid_benchmark("Benchmark level must be positive."))?;
        if value.is_sign_negative() || value.is_zero() {
            return Err(AppError::invalid_benchmark(
                "Benchmark level must be positive.",
            ));
        }
        Ok(Self(value))
    }

    pub fn from_decimal(value: Decimal) -> Result<Self, AppError> {
        if value.is_sign_negative() || value.is_zero() {
            return Err(AppError::invalid_benchmark(
                "Benchmark level must be positive.",
            ));
        }
        Self::parse(&canonical_decimal(value))
    }

    #[must_use]
    pub fn amount(self) -> Decimal {
        self.0
    }

    #[must_use]
    pub fn canonical(self) -> String {
        canonical_decimal(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScheduleCadence {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl ScheduleCadence {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "daily" => Ok(Self::Daily),
            "weekly" => Ok(Self::Weekly),
            "monthly" => Ok(Self::Monthly),
            "yearly" => Ok(Self::Yearly),
            _ => Err(AppError::invalid_recurring_rule(
                "Schedule cadence is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Yearly => "yearly",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScheduleInterval {
    cadence: ScheduleCadence,
    value: u16,
}

impl ScheduleInterval {
    pub fn new(cadence: ScheduleCadence, value: u16) -> Result<Self, AppError> {
        let valid = match cadence {
            ScheduleCadence::Daily => (1..=365).contains(&value),
            ScheduleCadence::Weekly => (1..=52).contains(&value),
            ScheduleCadence::Monthly => (1..=24).contains(&value),
            ScheduleCadence::Yearly => (1..=10).contains(&value),
        };
        if !valid {
            return Err(AppError::invalid_recurring_rule(
                "Schedule interval is outside the supported bound.",
            ));
        }
        Ok(Self { cadence, value })
    }

    #[must_use]
    pub fn cadence(self) -> ScheduleCadence {
        self.cadence
    }

    #[must_use]
    pub fn value(self) -> u16 {
        self.value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CarryWindow(u8);

impl CarryWindow {
    pub fn new(days: u8) -> Result<Self, AppError> {
        if days > 31 {
            return Err(AppError::invalid_benchmark(
                "Benchmark carry window must be between 0 and 31 days.",
            ));
        }
        Ok(Self(days))
    }

    #[must_use]
    pub fn days(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    cadence: ScheduleCadence,
    interval: ScheduleInterval,
    start: CalendarDate,
    anchor: CalendarDate,
    end: Option<CalendarDate>,
}

impl Schedule {
    pub fn new(
        cadence: ScheduleCadence,
        interval: ScheduleInterval,
        start: CalendarDate,
        end: Option<CalendarDate>,
    ) -> Result<Self, AppError> {
        Self::with_anchor(cadence, interval, start, start, end)
    }

    pub fn with_anchor(
        cadence: ScheduleCadence,
        interval: ScheduleInterval,
        start: CalendarDate,
        anchor: CalendarDate,
        end: Option<CalendarDate>,
    ) -> Result<Self, AppError> {
        if cadence != interval.cadence() {
            return Err(AppError::invalid_recurring_rule(
                "Schedule cadence and interval do not match.",
            ));
        }
        if anchor > start {
            return Err(AppError::invalid_recurring_rule(
                "Schedule anchor cannot be after the start date.",
            ));
        }
        if end.is_some_and(|value| value < start) {
            return Err(AppError::invalid_recurring_rule(
                "Schedule end date cannot be before the start date.",
            ));
        }
        Ok(Self {
            cadence,
            interval,
            start,
            anchor,
            end,
        })
    }

    #[must_use]
    pub fn cadence(self) -> ScheduleCadence {
        self.cadence
    }

    #[must_use]
    pub fn interval(self) -> ScheduleInterval {
        self.interval
    }

    #[must_use]
    pub fn start(self) -> CalendarDate {
        self.start
    }

    #[must_use]
    pub fn anchor(self) -> CalendarDate {
        self.anchor
    }

    #[must_use]
    pub fn end(self) -> Option<CalendarDate> {
        self.end
    }

    pub fn occurrences_through(
        self,
        through: CalendarDate,
        requested_limit: usize,
    ) -> Result<RecurrenceResult, AppError> {
        if requested_limit == 0 {
            return Err(AppError::invalid_recurring_rule(
                "Recurrence generation limit must be positive.",
            ));
        }
        let limit = requested_limit.min(MAX_RECURRENCE_OCCURRENCES);
        let upper_bound = self
            .end
            .map_or(through, |end| if end < through { end } else { through });
        if upper_bound < self.start {
            return Ok(RecurrenceResult {
                dates: Vec::new(),
                has_more: false,
            });
        }

        let mut index = self.first_index_at_or_after(self.start)?;
        let mut dates = Vec::with_capacity(limit);
        loop {
            let Some(date) = self.occurrence_at(index)? else {
                return Ok(RecurrenceResult {
                    dates,
                    has_more: false,
                });
            };
            if date > upper_bound {
                return Ok(RecurrenceResult {
                    dates,
                    has_more: false,
                });
            }
            if dates.len() == limit {
                return Ok(RecurrenceResult {
                    dates,
                    has_more: true,
                });
            }
            dates.push(date);
            index = index
                .checked_add(1)
                .ok_or_else(|| AppError::invalid_recurring_rule("Recurrence overflowed."))?;
        }
    }

    pub fn occurrences_after(
        self,
        after: CalendarDate,
        through: CalendarDate,
        requested_limit: usize,
    ) -> Result<RecurrenceResult, AppError> {
        if requested_limit == 0 {
            return Err(AppError::invalid_recurring_rule(
                "Recurrence generation limit must be positive.",
            ));
        }
        let limit = requested_limit.min(MAX_RECURRENCE_OCCURRENCES);
        let upper_bound = self
            .end
            .map_or(through, |end| if end < through { end } else { through });
        if upper_bound <= after {
            return Ok(RecurrenceResult {
                dates: Vec::new(),
                has_more: false,
            });
        }

        let mut index = self.first_index_at_or_after(after)?;
        let mut dates = Vec::with_capacity(limit);
        loop {
            let Some(date) = self.occurrence_at(index)? else {
                return Ok(RecurrenceResult {
                    dates,
                    has_more: false,
                });
            };
            if date <= after {
                index = index
                    .checked_add(1)
                    .ok_or_else(|| AppError::invalid_recurring_rule("Recurrence overflowed."))?;
                continue;
            }
            if date > upper_bound {
                return Ok(RecurrenceResult {
                    dates,
                    has_more: false,
                });
            }
            if dates.len() == limit {
                return Ok(RecurrenceResult {
                    dates,
                    has_more: true,
                });
            }
            dates.push(date);
            index = index
                .checked_add(1)
                .ok_or_else(|| AppError::invalid_recurring_rule("Recurrence overflowed."))?;
        }
    }

    fn first_index_at_or_after(self, target: CalendarDate) -> Result<u64, AppError> {
        if self.anchor >= target {
            return Ok(0);
        }
        let difference = target.as_naive_date() - self.anchor.as_naive_date();
        let rough = match self.cadence {
            ScheduleCadence::Daily => {
                ceil_div(difference.num_days(), i64::from(self.interval.value))
            }
            ScheduleCadence::Weekly => {
                ceil_div(difference.num_days(), i64::from(self.interval.value) * 7)
            }
            ScheduleCadence::Monthly | ScheduleCadence::Yearly => {
                let months = month_difference(self.anchor.as_naive_date(), target.as_naive_date());
                let step = i64::from(self.interval.value)
                    * if self.cadence == ScheduleCadence::Yearly {
                        12
                    } else {
                        1
                    };
                ceil_div(months, step)
            }
        };
        let mut index = u64::try_from(rough.max(0))
            .map_err(|_| AppError::invalid_recurring_rule("Recurrence index overflowed."))?;
        while self.occurrence_at(index)?.is_some_and(|date| date < target) {
            index = index
                .checked_add(1)
                .ok_or_else(|| AppError::invalid_recurring_rule("Recurrence overflowed."))?;
        }
        while index > 0
            && self
                .occurrence_at(index - 1)?
                .is_some_and(|date| date >= target)
        {
            index -= 1;
        }
        Ok(index)
    }

    fn occurrence_at(self, index: u64) -> Result<Option<CalendarDate>, AppError> {
        let units = index
            .checked_mul(u64::from(self.interval.value))
            .ok_or_else(|| AppError::invalid_recurring_rule("Recurrence interval overflowed."))?;
        match self.cadence {
            ScheduleCadence::Daily => {
                let days = i64::try_from(units)
                    .map_err(|_| AppError::invalid_recurring_rule("Recurrence date overflowed."))?;
                Ok(self.anchor.checked_add_days(days))
            }
            ScheduleCadence::Weekly => {
                let days = units
                    .checked_mul(7)
                    .and_then(|value| i64::try_from(value).ok())
                    .ok_or_else(|| {
                        AppError::invalid_recurring_rule("Recurrence date overflowed.")
                    })?;
                Ok(self.anchor.checked_add_days(days))
            }
            ScheduleCadence::Monthly => {
                let months = u32::try_from(units)
                    .map_err(|_| AppError::invalid_recurring_rule("Recurrence date overflowed."))?;
                month_occurrence(self.anchor, months)
                    .map_err(|_| AppError::invalid_recurring_rule("Recurrence date overflowed."))
            }
            ScheduleCadence::Yearly => {
                let months = units
                    .checked_mul(12)
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| {
                        AppError::invalid_recurring_rule("Recurrence date overflowed.")
                    })?;
                month_occurrence(self.anchor, months)
                    .map_err(|_| AppError::invalid_recurring_rule("Recurrence date overflowed."))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecurrenceResult {
    pub dates: Vec<CalendarDate>,
    pub has_more: bool,
}

fn ceil_div(numerator: i64, denominator: i64) -> i64 {
    if numerator <= 0 {
        0
    } else {
        (numerator + denominator - 1) / denominator
    }
}

fn month_difference(from: NaiveDate, to: NaiveDate) -> i64 {
    (i64::from(to.year()) - i64::from(from.year())) * 12 + i64::from(to.month())
        - i64::from(from.month())
}

fn month_occurrence(anchor: CalendarDate, months: u32) -> Result<Option<CalendarDate>, ()> {
    let date = anchor.as_naive_date();
    let first = NaiveDate::from_ymd_opt(date.year(), date.month(), 1).ok_or(())?;
    let target = first.checked_add_months(Months::new(months)).ok_or(())?;
    let next = target.checked_add_months(Months::new(1)).ok_or(())?;
    let last_day = next.checked_sub_days(Days::new(1)).ok_or(())?;
    let day = date.day().min(last_day.day());
    target
        .with_day(day)
        .map(CalendarDate::from_naive_date)
        .ok_or(())
        .map(Some)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PendingActivityKind {
    Deposit,
    Withdrawal,
    Transfer,
    PositionTransfer,
    Buy,
    Sell,
    Income,
    Fee,
    DebtDraw,
    DebtPayment,
}

impl PendingActivityKind {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "deposit" => Ok(Self::Deposit),
            "withdrawal" => Ok(Self::Withdrawal),
            "transfer" => Ok(Self::Transfer),
            "position_transfer" => Ok(Self::PositionTransfer),
            "buy" => Ok(Self::Buy),
            "sell" => Ok(Self::Sell),
            "income" => Ok(Self::Income),
            "fee" => Ok(Self::Fee),
            "debt_draw" => Ok(Self::DebtDraw),
            "debt_payment" => Ok(Self::DebtPayment),
            _ => Err(AppError::invalid_pending_activity(
                "Pending Activity kind is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deposit => "deposit",
            Self::Withdrawal => "withdrawal",
            Self::Transfer => "transfer",
            Self::PositionTransfer => "position_transfer",
            Self::Buy => "buy",
            Self::Sell => "sell",
            Self::Income => "income",
            Self::Fee => "fee",
            Self::DebtDraw => "debt_draw",
            Self::DebtPayment => "debt_payment",
        }
    }
}

impl TryFrom<ActivityKind> for PendingActivityKind {
    type Error = AppError;

    fn try_from(value: ActivityKind) -> Result<Self, Self::Error> {
        match value {
            ActivityKind::Deposit => Ok(Self::Deposit),
            ActivityKind::Withdrawal => Ok(Self::Withdrawal),
            ActivityKind::Transfer => Ok(Self::Transfer),
            ActivityKind::Buy => Ok(Self::Buy),
            ActivityKind::Sell => Ok(Self::Sell),
            ActivityKind::Income => Ok(Self::Income),
            ActivityKind::Fee => Ok(Self::Fee),
            ActivityKind::DebtDraw => Ok(Self::DebtDraw),
            ActivityKind::DebtPayment => Ok(Self::DebtPayment),
            ActivityKind::OpeningAdjustment
            | ActivityKind::BalanceAdjustment
            | ActivityKind::PositionAdjustment
            | ActivityKind::DebtAdjustment
            | ActivityKind::ManualValuation
            | ActivityKind::Reversal => Err(AppError::invalid_pending_activity(
                "This Activity kind cannot be stored as a pending proposal.",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingActivityPayload {
    Deposit {
        endpoint: MonetaryEndpoint,
        amount: Money,
    },
    Withdrawal {
        endpoint: MonetaryEndpoint,
        amount: Money,
    },
    Transfer {
        source: MonetaryEndpoint,
        source_amount: Money,
        destination: MonetaryEndpoint,
        destination_amount: Money,
        fee: Option<Money>,
        fee_kind: Option<FeeKind>,
    },
    PositionTransfer {
        source: QuantityEndpoint,
        destination: QuantityEndpoint,
        quantity: Quantity,
    },
    Buy {
        holding_id: HoldingId,
        instrument_id: InstrumentId,
        quantity: Quantity,
        unit_price: UnitPrice,
        gross_amount: Money,
        fee: Option<Money>,
        confirm_zero_unit_price: bool,
    },
    Sell {
        holding_id: HoldingId,
        instrument_id: InstrumentId,
        quantity: Quantity,
        unit_price: UnitPrice,
        gross_amount: Money,
        fee: Option<Money>,
        confirm_zero_unit_price: bool,
    },
    Income {
        endpoint: MonetaryEndpoint,
        amount: Money,
        income_kind: IncomeKind,
        instrument_id: Option<InstrumentId>,
    },
    Fee {
        endpoint: MonetaryEndpoint,
        amount: Money,
        fee_kind: FeeKind,
        instrument_id: Option<InstrumentId>,
    },
    DebtDraw {
        liability_account_id: AccountId,
        principal: Money,
        cash: Option<(MonetaryEndpoint, Money)>,
        fx_rate: Option<FxRate>,
    },
    DebtPayment {
        liability_account_id: AccountId,
        principal: Money,
        cash: (MonetaryEndpoint, Money),
        fee: Option<(Money, FeeKind)>,
        fx_rate: Option<FxRate>,
    },
}

impl PendingActivityPayload {
    pub fn validate(&self) -> Result<PendingActivityKind, AppError> {
        let kind = match self {
            Self::Deposit { amount, .. } => {
                require_positive_money(*amount)?;
                PendingActivityKind::Deposit
            }
            Self::Withdrawal { amount, .. } => {
                require_positive_money(*amount)?;
                PendingActivityKind::Withdrawal
            }
            Self::Transfer {
                source,
                source_amount,
                destination,
                destination_amount,
                fee,
                fee_kind,
                ..
            } => {
                require_positive_money(*source_amount)?;
                require_positive_money(*destination_amount)?;
                if source == destination
                    && source_amount.currency() == destination_amount.currency()
                {
                    return Err(AppError::invalid_pending_activity(
                        "Transfer source and destination must be different endpoints.",
                    ));
                }
                if source_amount.currency() == destination_amount.currency()
                    && source_amount != destination_amount
                {
                    return Err(AppError::invalid_pending_activity(
                        "Same-currency transfer amounts must match exactly.",
                    ));
                }
                if let Some(fee_amount) = fee {
                    if fee_amount.currency() != source_amount.currency() {
                        return Err(AppError::invalid_pending_activity(
                            "A transfer fee must use the source currency.",
                        ));
                    }
                }
                validate_fee(*fee, *fee_kind)?;
                PendingActivityKind::Transfer
            }
            Self::PositionTransfer {
                source,
                destination,
                quantity,
            } => {
                if source.instrument_id != destination.instrument_id {
                    return Err(AppError::invalid_pending_activity(
                        "Position Transfer endpoints must use the same Instrument.",
                    ));
                }
                if source.holding_id == destination.holding_id {
                    return Err(AppError::invalid_pending_activity(
                        "Position Transfer source and destination holdings must differ.",
                    ));
                }
                require_positive_quantity(*quantity)?;
                PendingActivityKind::PositionTransfer
            }
            Self::Buy {
                quantity,
                unit_price,
                gross_amount,
                fee,
                confirm_zero_unit_price,
                ..
            }
            | Self::Sell {
                quantity,
                unit_price,
                gross_amount,
                fee,
                confirm_zero_unit_price,
                ..
            } => {
                require_positive_quantity(*quantity)?;
                if unit_price.is_zero() && !confirm_zero_unit_price {
                    return Err(AppError::invalid_pending_activity(
                        "A zero unit price requires explicit confirmation.",
                    ));
                }
                let expected = Money::from_canonical(
                    checked_mul(quantity.amount(), unit_price.amount()).map_err(|_| {
                        AppError::invalid_pending_activity(
                            "Trade total is outside the supported range.",
                        )
                    })?,
                    gross_amount.currency(),
                )
                .map_err(|_| {
                    AppError::invalid_pending_activity(
                        "Trade total is outside the supported range.",
                    )
                })?;
                if expected != *gross_amount {
                    return Err(AppError::invalid_pending_activity(
                        "Gross amount must equal quantity multiplied by unit price.",
                    ));
                }
                validate_optional_money(*fee)?;
                if matches!(self, Self::Buy { .. }) {
                    PendingActivityKind::Buy
                } else {
                    PendingActivityKind::Sell
                }
            }
            Self::Income { amount, .. } => {
                require_positive_money(*amount)?;
                PendingActivityKind::Income
            }
            Self::Fee { amount, .. } => {
                require_positive_money(*amount)?;
                PendingActivityKind::Fee
            }
            Self::DebtDraw {
                principal, cash, ..
            } => {
                require_positive_money(*principal)?;
                if let Some((_, amount)) = cash {
                    require_positive_money(*amount)?;
                }
                PendingActivityKind::DebtDraw
            }
            Self::DebtPayment {
                principal,
                cash,
                fee,
                ..
            } => {
                require_positive_money(*principal)?;
                require_positive_money(cash.1)?;
                if let Some((amount, _)) = fee {
                    require_positive_money(*amount)?;
                    if amount.currency() != cash.1.currency() {
                        return Err(AppError::invalid_pending_activity(
                            "A debt payment fee must use the cash currency.",
                        ));
                    }
                }
                PendingActivityKind::DebtPayment
            }
        };
        Ok(kind)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecurringActivityKind {
    Deposit,
    Withdrawal,
    Transfer,
    Income,
    Fee,
    DebtDraw,
    DebtPayment,
}

impl RecurringActivityKind {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "deposit" => Ok(Self::Deposit),
            "withdrawal" => Ok(Self::Withdrawal),
            "transfer" => Ok(Self::Transfer),
            "income" => Ok(Self::Income),
            "fee" => Ok(Self::Fee),
            "debt_draw" => Ok(Self::DebtDraw),
            "debt_payment" => Ok(Self::DebtPayment),
            _ => Err(AppError::invalid_recurring_rule(
                "Recurring Activity kind is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deposit => "deposit",
            Self::Withdrawal => "withdrawal",
            Self::Transfer => "transfer",
            Self::Income => "income",
            Self::Fee => "fee",
            Self::DebtDraw => "debt_draw",
            Self::DebtPayment => "debt_payment",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecurringActivityPayload(PendingActivityPayload);

impl RecurringActivityPayload {
    pub fn new(payload: PendingActivityPayload) -> Result<Self, AppError> {
        let kind = payload.validate()?;
        match (&payload, kind) {
            (PendingActivityPayload::Deposit { .. }, PendingActivityKind::Deposit)
            | (PendingActivityPayload::Withdrawal { .. }, PendingActivityKind::Withdrawal)
            | (PendingActivityPayload::Income { .. }, PendingActivityKind::Income)
            | (PendingActivityPayload::Fee { .. }, PendingActivityKind::Fee)
            | (PendingActivityPayload::DebtDraw { .. }, PendingActivityKind::DebtDraw)
            | (PendingActivityPayload::DebtPayment { .. }, PendingActivityKind::DebtPayment) => {
                Ok(Self(payload))
            }
            (
                PendingActivityPayload::Transfer {
                    source_amount,
                    destination_amount,
                    ..
                },
                PendingActivityKind::Transfer,
            ) if source_amount.currency() == destination_amount.currency() => Ok(Self(payload)),
            _ => Err(AppError::invalid_recurring_rule(
                "This Activity payload is not supported by recurring rules.",
            )),
        }
    }

    #[must_use]
    pub fn kind(&self) -> RecurringActivityKind {
        match &self.0 {
            PendingActivityPayload::Deposit { .. } => RecurringActivityKind::Deposit,
            PendingActivityPayload::Withdrawal { .. } => RecurringActivityKind::Withdrawal,
            PendingActivityPayload::Transfer { .. } => RecurringActivityKind::Transfer,
            PendingActivityPayload::Income { .. } => RecurringActivityKind::Income,
            PendingActivityPayload::Fee { .. } => RecurringActivityKind::Fee,
            PendingActivityPayload::DebtDraw { .. } => RecurringActivityKind::DebtDraw,
            PendingActivityPayload::DebtPayment { .. } => RecurringActivityKind::DebtPayment,
            PendingActivityPayload::PositionTransfer { .. }
            | PendingActivityPayload::Buy { .. }
            | PendingActivityPayload::Sell { .. } => {
                unreachable!("recurring payload constructor rejects this variant")
            }
        }
    }

    #[must_use]
    pub fn as_pending(&self) -> &PendingActivityPayload {
        &self.0
    }
}

fn require_positive_money(amount: Money) -> Result<(), AppError> {
    if amount.is_zero() {
        Err(AppError::invalid_pending_activity(
            "Pending financial amounts must be positive.",
        ))
    } else {
        Ok(())
    }
}

fn validate_optional_money(amount: Option<Money>) -> Result<(), AppError> {
    if let Some(amount) = amount {
        require_positive_money(amount)?;
    }
    Ok(())
}

fn validate_fee(fee: Option<Money>, fee_kind: Option<FeeKind>) -> Result<(), AppError> {
    if fee.is_some() != fee_kind.is_some() {
        return Err(AppError::invalid_pending_activity(
            "Fee amount and fee kind must be supplied together.",
        ));
    }
    validate_optional_money(fee)
}

fn require_positive_quantity(quantity: Quantity) -> Result<(), AppError> {
    if quantity.is_zero() {
        Err(AppError::invalid_pending_activity(
            "Pending quantities must be positive.",
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportTemplate {
    ActivityV1,
    QuoteV1,
    BenchmarkV1,
}

impl ImportTemplate {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "# nestworth-csv:activity:v1" => Ok(Self::ActivityV1),
            "# nestworth-csv:quote:v1" => Ok(Self::QuoteV1),
            "# nestworth-csv:benchmark:v1" => Ok(Self::BenchmarkV1),
            _ => Err(AppError::invalid_import_row(
                "CSV template version is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ActivityV1 => "# nestworth-csv:activity:v1",
            Self::QuoteV1 => "# nestworth-csv:quote:v1",
            Self::BenchmarkV1 => "# nestworth-csv:benchmark:v1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackupFormatVersion {
    V1,
}

impl BackupFormatVersion {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "1" => Ok(Self::V1),
            _ => Err(AppError::backup_unsupported_version()),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "1",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceNamespace(String);

impl SourceNamespace {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        let valid_length = (1..=80).contains(&value.len());
        let valid_chars = value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        });
        if !valid_length || !valid_chars || !value.as_bytes()[0].is_ascii_alphanumeric() {
            return Err(AppError::invalid_import_row(
                "Source namespace must match the lowercase namespace contract.",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExternalId(String);

impl ExternalId {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        let value = value.trim();
        if !(1..=200).contains(&value.chars().count())
            || value.chars().any(|character| character.is_control())
        {
            return Err(AppError::invalid_import_row(
                "External ID must contain 1 to 200 Unicode scalar values without controls.",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ExternalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportField {
    Missing,
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalImportRow {
    template: ImportTemplate,
    fields: Vec<ImportField>,
}

impl CanonicalImportRow {
    pub fn new(template: ImportTemplate, fields: Vec<ImportField>) -> Result<Self, AppError> {
        for field in &fields {
            if let ImportField::Text(value) = field {
                if value.as_bytes().contains(&0) {
                    return Err(AppError::invalid_import_row(
                        "Canonical import fields cannot contain NUL bytes.",
                    ));
                }
            }
        }
        Ok(Self { template, fields })
    }

    #[must_use]
    pub fn template(&self) -> ImportTemplate {
        self.template
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, AppError> {
        let mut bytes = Vec::from(CANONICAL_IMPORT_PREFIX);
        for field in &self.fields {
            match field {
                ImportField::Missing => bytes.extend_from_slice(&u32::MAX.to_be_bytes()),
                ImportField::Text(value) => {
                    let length = u32::try_from(value.len()).map_err(|_| {
                        AppError::invalid_import_row("Canonical import field is too large.")
                    })?;
                    bytes.extend_from_slice(&length.to_be_bytes());
                    bytes.extend_from_slice(value.as_bytes());
                }
            }
        }
        Ok(bytes)
    }

    pub fn fingerprint(&self) -> Result<ImportFingerprint, AppError> {
        Ok(ImportFingerprint(Checksum::sha256(
            &self.canonical_bytes()?,
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityImportRow {
    pub source_namespace: SourceNamespace,
    pub external_id: ExternalId,
    pub kind: String,
    pub effective_local_date: String,
    pub effective_local_time: String,
    pub ambiguous_offset: Option<String>,
    pub account_id: String,
    pub component_kind: ComponentKind,
    pub amount: Option<String>,
    pub currency: Option<String>,
    pub note: Option<String>,
    pub account_label: Option<String>,
}

impl ActivityImportRow {
    pub fn canonical_row(&self) -> Result<CanonicalImportRow, AppError> {
        CanonicalImportRow::new(
            ImportTemplate::ActivityV1,
            vec![
                ImportField::Text("activity".to_owned()),
                ImportField::Text(self.source_namespace.as_str().to_owned()),
                ImportField::Text(self.external_id.as_str().to_owned()),
                ImportField::Text(self.kind.clone()),
                ImportField::Text(self.effective_local_date.clone()),
                ImportField::Text(self.effective_local_time.clone()),
                optional_field(&self.ambiguous_offset),
                ImportField::Text(self.account_id.clone()),
                ImportField::Text(self.component_kind.as_str().to_owned()),
                optional_field(&self.amount),
                optional_field(&self.currency),
                optional_field(&self.note),
            ],
        )
    }

    pub fn fingerprint(&self) -> Result<ImportFingerprint, AppError> {
        self.canonical_row()?.fingerprint()
    }
}

fn optional_field(value: &Option<String>) -> ImportField {
    value.as_ref().map_or(ImportField::Missing, |value| {
        ImportField::Text(value.clone())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Checksum([u8; 32]);

impl Checksum {
    #[must_use]
    pub fn sha256(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Self(hasher.finalize().into())
    }

    pub fn parse_hex(value: &str) -> Result<Self, AppError> {
        if value.len() != 64 {
            return Err(AppError::invalid_import_row(
                "SHA-256 checksum must contain 64 hexadecimal characters.",
            ));
        }
        let mut bytes = [0_u8; 32];
        for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
            let text = std::str::from_utf8(chunk)
                .map_err(|_| AppError::invalid_import_row("Checksum is not valid hexadecimal."))?;
            bytes[index] = u8::from_str_radix(text, 16)
                .map_err(|_| AppError::invalid_import_row("Checksum is not valid hexadecimal."))?;
        }
        Ok(Self(bytes))
    }

    #[must_use]
    pub fn as_bytes(self) -> [u8; 32] {
        self.0
    }

    #[must_use]
    pub fn hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Display for Checksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImportFingerprint(Checksum);

impl ImportFingerprint {
    #[must_use]
    pub fn checksum(self) -> Checksum {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        AccountId, ActivityKind, CalendarDate, CurrencyCode, HistoryTimezone, Timestamp,
    };

    fn date(value: &str) -> CalendarDate {
        CalendarDate::parse(value).expect("valid date")
    }

    fn activity_row(account_label: Option<&str>) -> ActivityImportRow {
        ActivityImportRow {
            source_namespace: SourceNamespace::parse("acct.example").expect("namespace"),
            external_id: ExternalId::parse("salary-2026-01").expect("external id"),
            kind: "deposit".to_owned(),
            effective_local_date: "2026-01-31".to_owned(),
            effective_local_time: "09:00".to_owned(),
            ambiguous_offset: None,
            account_id: "99999999-9999-4999-8999-999999999999".to_owned(),
            component_kind: ComponentKind::AccountValue,
            amount: Some("5000".to_owned()),
            currency: Some("SGD".to_owned()),
            note: Some("January salary".to_owned()),
            account_label: account_label.map(str::to_owned),
        }
    }

    #[test]
    fn benchmark_level_is_positive_bounded_decimal() {
        assert_eq!(
            BenchmarkLevel::parse("100.00000000")
                .expect("level")
                .canonical(),
            "100"
        );
        assert!(BenchmarkLevel::parse("0").is_err());
        assert!(BenchmarkLevel::parse("-1").is_err());
        assert!(BenchmarkLevel::parse("1.000000001").is_err());
        assert!(BenchmarkLevel::parse("1000000000000000000").is_err());
    }

    #[test]
    fn schedule_intervals_and_month_end_clamping_match_phase_zero_goldens() {
        let monthly = Schedule::new(
            ScheduleCadence::Monthly,
            ScheduleInterval::new(ScheduleCadence::Monthly, 1).expect("interval"),
            date("2026-01-31"),
            Some(date("2026-04-30")),
        )
        .expect("schedule");
        let result = monthly
            .occurrences_through(date("2026-04-30"), 20)
            .expect("occurrences");
        assert_eq!(
            result
                .dates
                .iter()
                .map(|value| value.to_ymd())
                .collect::<Vec<_>>(),
            vec!["2026-01-31", "2026-02-28", "2026-03-31", "2026-04-30"]
        );
        assert!(!result.has_more);
        assert!(ScheduleInterval::new(ScheduleCadence::Daily, 0).is_err());
        assert!(ScheduleInterval::new(ScheduleCadence::Yearly, 11).is_err());
        assert_eq!(CarryWindow::new(31).expect("carry window").days(), 31);
        assert!(CarryWindow::new(32).is_err());

        let daily = Schedule::new(
            ScheduleCadence::Daily,
            ScheduleInterval::new(ScheduleCadence::Daily, 2).expect("interval"),
            date("2026-01-30"),
            Some(date("2026-02-05")),
        )
        .expect("daily schedule")
        .occurrences_through(date("2026-02-05"), 10)
        .expect("daily occurrences");
        assert_eq!(
            daily
                .dates
                .iter()
                .map(|value| value.to_ymd())
                .collect::<Vec<_>>(),
            vec!["2026-01-30", "2026-02-01", "2026-02-03", "2026-02-05"]
        );

        let weekly = Schedule::new(
            ScheduleCadence::Weekly,
            ScheduleInterval::new(ScheduleCadence::Weekly, 2).expect("interval"),
            date("2026-01-30"),
            Some(date("2026-02-20")),
        )
        .expect("weekly schedule")
        .occurrences_through(date("2026-02-20"), 10)
        .expect("weekly occurrences");
        assert_eq!(
            weekly
                .dates
                .iter()
                .map(|value| value.to_ymd())
                .collect::<Vec<_>>(),
            vec!["2026-01-30", "2026-02-13"]
        );

        let leap_year = Schedule::new(
            ScheduleCadence::Yearly,
            ScheduleInterval::new(ScheduleCadence::Yearly, 1).expect("interval"),
            date("2028-02-29"),
            Some(date("2032-02-29")),
        )
        .expect("yearly schedule")
        .occurrences_through(date("2032-02-29"), 10)
        .expect("yearly occurrences");
        assert_eq!(
            leap_year
                .dates
                .iter()
                .map(|value| value.to_ymd())
                .collect::<Vec<_>>(),
            vec![
                "2028-02-29",
                "2029-02-28",
                "2030-02-28",
                "2031-02-28",
                "2032-02-29"
            ]
        );

        let before_end_of_month = Schedule::new(
            ScheduleCadence::Monthly,
            ScheduleInterval::new(ScheduleCadence::Monthly, 1).expect("interval"),
            date("2026-01-31"),
            Some(date("2026-02-15")),
        )
        .expect("end date")
        .occurrences_through(date("2026-12-31"), 10)
        .expect("end-date occurrences");
        assert_eq!(
            before_end_of_month
                .dates
                .iter()
                .map(|value| value.to_ymd())
                .collect::<Vec<_>>(),
            vec!["2026-01-31"]
        );
    }

    #[test]
    fn recurrence_uses_history_timezone_for_local_today_without_utc_occurrence_math() {
        let timezone = HistoryTimezone::parse("America/New_York").expect("timezone");
        let local_today = timezone.local_date(
            &Timestamp::parse("2026-03-08T07:00:00Z").expect("DST transition timestamp"),
        );
        let schedule = Schedule::new(
            ScheduleCadence::Daily,
            ScheduleInterval::new(ScheduleCadence::Daily, 1).expect("interval"),
            date("2026-03-07"),
            None,
        )
        .expect("schedule");
        let result = schedule
            .occurrences_through(local_today, 10)
            .expect("local dates");
        assert_eq!(
            result
                .dates
                .iter()
                .map(|value| value.to_ymd())
                .collect::<Vec<_>>(),
            vec!["2026-03-07", "2026-03-08"]
        );
    }

    #[test]
    fn recurrence_continuation_starts_after_monthly_clamped_date() {
        let monthly = Schedule::new(
            ScheduleCadence::Monthly,
            ScheduleInterval::new(ScheduleCadence::Monthly, 1).expect("interval"),
            date("2026-01-31"),
            Some(date("2026-04-30")),
        )
        .expect("schedule");
        let result = monthly
            .occurrences_after(date("2026-02-28"), date("2026-04-30"), 20)
            .expect("continuation");
        assert_eq!(
            result
                .dates
                .iter()
                .map(|value| value.to_ymd())
                .collect::<Vec<_>>(),
            vec!["2026-03-31", "2026-04-30"]
        );
        assert!(!result.has_more);
    }

    #[test]
    fn recurrence_limit_reports_continuation_without_exceeding_366() {
        let daily = Schedule::new(
            ScheduleCadence::Daily,
            ScheduleInterval::new(ScheduleCadence::Daily, 1).expect("interval"),
            date("2026-01-01"),
            None,
        )
        .expect("schedule");
        let result = daily
            .occurrences_through(date("2027-12-31"), usize::MAX)
            .expect("occurrences");
        assert_eq!(result.dates.len(), MAX_RECURRENCE_OCCURRENCES);
        assert!(result.has_more);
    }

    #[test]
    fn pending_and_recurring_allowlists_reject_unsafe_activity_kinds() {
        for kind in [
            ActivityKind::OpeningAdjustment,
            ActivityKind::PositionAdjustment,
            ActivityKind::DebtAdjustment,
            ActivityKind::ManualValuation,
            ActivityKind::Reversal,
        ] {
            assert!(PendingActivityKind::try_from(kind).is_err());
        }
        assert_eq!(
            PendingActivityKind::parse("position_transfer").expect("pending kind"),
            PendingActivityKind::PositionTransfer
        );
        assert_eq!(
            RecurringActivityKind::parse("deposit").expect("recurring kind"),
            RecurringActivityKind::Deposit
        );
        assert!(RecurringActivityKind::parse("buy").is_err());
        let pending = PendingActivityPayload::Buy {
            holding_id: super::HoldingId::new(),
            instrument_id: super::InstrumentId::new(),
            quantity: Quantity::parse("1").expect("quantity"),
            unit_price: UnitPrice::parse("100").expect("price"),
            gross_amount: Money::parse("100", CurrencyCode::USD).expect("gross"),
            fee: None,
            confirm_zero_unit_price: false,
        };
        assert!(RecurringActivityPayload::new(pending).is_err());
        let _ = AccountId::new();
    }

    #[test]
    fn canonical_import_fingerprint_matches_phase_zero_vector_and_ignores_label() {
        let row = activity_row(Some("Salary Account"));
        let fingerprint = row.fingerprint().expect("fingerprint");
        assert_eq!(
            fingerprint.checksum().hex(),
            "69c6d989cf0cf8196496a59f949eb870842adcc461f67475c83322e2f9532139"
        );
        assert_eq!(
            row.canonical_row()
                .expect("row")
                .canonical_bytes()
                .expect("bytes")
                .len(),
            198
        );
        let mutated = ActivityImportRow {
            amount: Some("5100".to_owned()),
            ..row.clone()
        };
        assert_eq!(
            mutated
                .fingerprint()
                .expect("mutated fingerprint")
                .checksum()
                .hex(),
            "e3e13c998580c2056abcbd50c54088b4aa358e4b4a7823ca8dfbff955b6b6cf5"
        );
        assert_eq!(
            fingerprint.checksum(),
            activity_row(Some("Changed display label"))
                .fingerprint()
                .expect("fingerprint")
                .checksum()
        );
    }

    #[test]
    fn namespace_external_id_template_and_checksum_are_closed() {
        assert!(SourceNamespace::parse("Acct").is_err());
        assert!(SourceNamespace::parse("a".repeat(81).as_str()).is_err());
        assert!(ExternalId::parse("\u{0}").is_err());
        assert_eq!(
            ImportTemplate::parse("# nestworth-csv:activity:v1")
                .expect("template")
                .as_str(),
            "# nestworth-csv:activity:v1"
        );
        assert_eq!(
            BackupFormatVersion::parse("1").expect("version").as_str(),
            "1"
        );
        let checksum = Checksum::sha256(b"fixture");
        assert_eq!(
            Checksum::parse_hex(&checksum.hex()).expect("round trip"),
            checksum
        );
    }
}
