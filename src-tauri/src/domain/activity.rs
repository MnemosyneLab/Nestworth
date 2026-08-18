//! Activity domain types, kind-specific construction, classification, and reversal.
//!
//! Income kinds (`IncomeKind`) are the closed household set:
//! `salary`, `bonus`, `dividend`, `interest`, `rental`, `pension`, `gift`, `refund`, `other`.
//!
//! Fee kinds (`FeeKind`) are the closed household set:
//! `bank_fee`, `account_fee`, `brokerage_commission`, `management_fee`,
//! `foreign_exchange_fee`, `interest`, `tax`, `other`.
//!
//! Classification is derived by [`classify`] from kind and role only. Callers cannot
//! supply an arbitrary class on an Activity.

use super::{
    activity_leg::{
        ActivityLeg, Direction, LegComponent, LegRole, MonetaryEndpoint, QuantityEndpoint,
    },
    currency::CurrencyCode,
    decimal::{checked_mul, checked_sub},
    fx::{convert_with_direct_rate, FxRate},
    ids::{AccountId, ActivityId, HouseholdId, InstrumentId},
    money::Money,
    quantity::Quantity,
    text::parse_optional_note,
    time::{CalendarDate, Timestamp},
    unit_price::UnitPrice,
};
use crate::error::AppError;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivityKind {
    OpeningAdjustment,
    BalanceAdjustment,
    PositionAdjustment,
    Deposit,
    Withdrawal,
    Transfer,
    Buy,
    Sell,
    Income,
    Fee,
    DebtDraw,
    DebtPayment,
    DebtAdjustment,
    ManualValuation,
    Reversal,
}

impl ActivityKind {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "opening_adjustment" => Ok(Self::OpeningAdjustment),
            "balance_adjustment" => Ok(Self::BalanceAdjustment),
            "position_adjustment" => Ok(Self::PositionAdjustment),
            "deposit" => Ok(Self::Deposit),
            "withdrawal" => Ok(Self::Withdrawal),
            "transfer" => Ok(Self::Transfer),
            "buy" => Ok(Self::Buy),
            "sell" => Ok(Self::Sell),
            "income" => Ok(Self::Income),
            "fee" => Ok(Self::Fee),
            "debt_draw" => Ok(Self::DebtDraw),
            "debt_payment" => Ok(Self::DebtPayment),
            "debt_adjustment" => Ok(Self::DebtAdjustment),
            "manual_valuation" => Ok(Self::ManualValuation),
            "reversal" => Ok(Self::Reversal),
            _ => Err(AppError::validation(
                "activityKind",
                "Activity kind is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpeningAdjustment => "opening_adjustment",
            Self::BalanceAdjustment => "balance_adjustment",
            Self::PositionAdjustment => "position_adjustment",
            Self::Deposit => "deposit",
            Self::Withdrawal => "withdrawal",
            Self::Transfer => "transfer",
            Self::Buy => "buy",
            Self::Sell => "sell",
            Self::Income => "income",
            Self::Fee => "fee",
            Self::DebtDraw => "debt_draw",
            Self::DebtPayment => "debt_payment",
            Self::DebtAdjustment => "debt_adjustment",
            Self::ManualValuation => "manual_valuation",
            Self::Reversal => "reversal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Classification {
    ExternalInflow,
    ExternalOutflow,
    InternalTransfer,
    TradePrincipal,
    Income,
    Fee,
    DebtPrincipal,
    Remeasurement,
}

impl Classification {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "external_inflow" => Ok(Self::ExternalInflow),
            "external_outflow" => Ok(Self::ExternalOutflow),
            "internal_transfer" => Ok(Self::InternalTransfer),
            "trade_principal" => Ok(Self::TradePrincipal),
            "income" => Ok(Self::Income),
            "fee" => Ok(Self::Fee),
            "debt_principal" => Ok(Self::DebtPrincipal),
            "remeasurement" => Ok(Self::Remeasurement),
            _ => Err(AppError::validation(
                "classification",
                "Activity classification is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExternalInflow => "external_inflow",
            Self::ExternalOutflow => "external_outflow",
            Self::InternalTransfer => "internal_transfer",
            Self::TradePrincipal => "trade_principal",
            Self::Income => "income",
            Self::Fee => "fee",
            Self::DebtPrincipal => "debt_principal",
            Self::Remeasurement => "remeasurement",
        }
    }
}

/// Closed household income kinds. Unknown persisted values are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IncomeKind {
    Salary,
    Bonus,
    Dividend,
    Interest,
    Rental,
    Pension,
    Gift,
    Refund,
    Other,
}

impl IncomeKind {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "salary" => Ok(Self::Salary),
            "bonus" => Ok(Self::Bonus),
            "dividend" => Ok(Self::Dividend),
            "interest" => Ok(Self::Interest),
            "rental" => Ok(Self::Rental),
            "pension" => Ok(Self::Pension),
            "gift" => Ok(Self::Gift),
            "refund" => Ok(Self::Refund),
            "other" => Ok(Self::Other),
            _ => Err(AppError::validation(
                "incomeKind",
                "Income kind is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Salary => "salary",
            Self::Bonus => "bonus",
            Self::Dividend => "dividend",
            Self::Interest => "interest",
            Self::Rental => "rental",
            Self::Pension => "pension",
            Self::Gift => "gift",
            Self::Refund => "refund",
            Self::Other => "other",
        }
    }
}

/// Closed household fee kinds. Unknown persisted values are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeeKind {
    BankFee,
    AccountFee,
    BrokerageCommission,
    ManagementFee,
    ForeignExchangeFee,
    Interest,
    Tax,
    Other,
}

impl FeeKind {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "bank_fee" => Ok(Self::BankFee),
            "account_fee" => Ok(Self::AccountFee),
            "brokerage_commission" => Ok(Self::BrokerageCommission),
            "management_fee" => Ok(Self::ManagementFee),
            "foreign_exchange_fee" => Ok(Self::ForeignExchangeFee),
            "interest" => Ok(Self::Interest),
            "tax" => Ok(Self::Tax),
            "other" => Ok(Self::Other),
            _ => Err(AppError::validation(
                "feeKind",
                "Fee kind is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BankFee => "bank_fee",
            Self::AccountFee => "account_fee",
            Self::BrokerageCommission => "brokerage_commission",
            Self::ManagementFee => "management_fee",
            Self::ForeignExchangeFee => "foreign_exchange_fee",
            Self::Interest => "interest",
            Self::Tax => "tax",
            Self::Other => "other",
        }
    }
}

/// Derives durable classification from Activity kind and leg role.
/// Callers cannot supply an arbitrary class.
#[must_use]
pub fn classify(kind: ActivityKind, role: LegRole) -> Classification {
    match (kind, role) {
        (ActivityKind::Deposit, _) => Classification::ExternalInflow,
        (ActivityKind::Withdrawal, _) => Classification::ExternalOutflow,
        (ActivityKind::Transfer, _) => Classification::InternalTransfer,
        (ActivityKind::Buy | ActivityKind::Sell, LegRole::Fee) => Classification::Fee,
        (ActivityKind::Buy | ActivityKind::Sell, _) => Classification::TradePrincipal,
        (ActivityKind::Income, _) => Classification::Income,
        (ActivityKind::Fee, _) => Classification::Fee,
        (ActivityKind::DebtDraw | ActivityKind::DebtPayment, LegRole::Fee) => Classification::Fee,
        (ActivityKind::DebtDraw | ActivityKind::DebtPayment, LegRole::Liability) => {
            Classification::DebtPrincipal
        }
        (ActivityKind::DebtDraw | ActivityKind::DebtPayment, _) => Classification::InternalTransfer,
        (
            ActivityKind::OpeningAdjustment
            | ActivityKind::BalanceAdjustment
            | ActivityKind::PositionAdjustment
            | ActivityKind::DebtAdjustment
            | ActivityKind::ManualValuation,
            _,
        ) => Classification::Remeasurement,
        (ActivityKind::Reversal, LegRole::Fee) => Classification::Fee,
        (ActivityKind::Reversal, LegRole::Income) => Classification::Income,
        (ActivityKind::Reversal, LegRole::Liability) => Classification::DebtPrincipal,
        (ActivityKind::Reversal, LegRole::Holding | LegRole::Settlement) => {
            Classification::TradePrincipal
        }
        (ActivityKind::Reversal, LegRole::Adjustment) => Classification::Remeasurement,
        (ActivityKind::Reversal, LegRole::Source | LegRole::Destination) => {
            Classification::InternalTransfer
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstructActivity {
    Posted(Activity),
    NoActivity,
}

pub struct ActivityRecordParams<'a> {
    pub household_id: HouseholdId,
    pub effective_at: Timestamp,
    pub effective_local_date: CalendarDate,
    pub created_at: Timestamp,
    pub note: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentOpening {
    AccountValue {
        account_id: AccountId,
        amount: Money,
    },
    HoldingsCash {
        account_id: AccountId,
        amount: Money,
    },
    HoldingQuantity {
        account_id: AccountId,
        holding_id: super::ids::HoldingId,
        instrument_id: InstrumentId,
        quantity: Quantity,
    },
}

impl ComponentOpening {
    fn is_zero(&self) -> bool {
        match self {
            Self::AccountValue { amount, .. } | Self::HoldingsCash { amount, .. } => {
                amount.is_zero()
            }
            Self::HoldingQuantity { quantity, .. } => quantity.is_zero(),
        }
    }

    fn into_leg_parts(self) -> (AccountId, LegComponent) {
        match self {
            Self::AccountValue { account_id, amount } => {
                (account_id, LegComponent::AccountValue { amount })
            }
            Self::HoldingsCash { account_id, amount } => {
                (account_id, LegComponent::HoldingsCash { amount })
            }
            Self::HoldingQuantity {
                account_id,
                holding_id,
                instrument_id,
                quantity,
            } => (
                account_id,
                LegComponent::HoldingQuantity {
                    instrument_id,
                    holding_id,
                    quantity,
                },
            ),
        }
    }
}

pub struct TradeSpec {
    pub account_id: AccountId,
    pub holding_id: super::ids::HoldingId,
    pub instrument_id: InstrumentId,
    pub quantity: Quantity,
    pub unit_price: UnitPrice,
    pub quote_currency: CurrencyCode,
    pub gross_amount: Money,
    pub settlement_currency: CurrencyCode,
    pub fee: Option<Money>,
    pub confirm_zero_unit_price: bool,
}

pub struct DebtCashLink {
    pub endpoint: MonetaryEndpoint,
    pub amount: Money,
    pub fx_rate: Option<FxRate>,
}

pub struct DebtDrawSpec {
    pub liability_account_id: AccountId,
    pub principal: Money,
    pub cash: Option<DebtCashLink>,
}

pub struct DebtPaymentSpec {
    pub liability_account_id: AccountId,
    pub principal: Money,
    pub cash: DebtCashLink,
    pub fee: Option<Money>,
    pub fee_kind: Option<FeeKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    id: ActivityId,
    household_id: HouseholdId,
    kind: ActivityKind,
    effective_at: Timestamp,
    effective_local_date: CalendarDate,
    created_at: Timestamp,
    note: Option<String>,
    reverses: Option<ActivityId>,
    corrects: Option<ActivityId>,
    correction_group: Option<uuid::Uuid>,
    income_kind: Option<IncomeKind>,
    fee_kind: Option<FeeKind>,
    related_instrument_id: Option<InstrumentId>,
    legs: Vec<ActivityLeg>,
}

impl Activity {
    #[must_use]
    pub fn id(&self) -> ActivityId {
        self.id
    }

    #[must_use]
    pub fn household_id(&self) -> HouseholdId {
        self.household_id
    }

    #[must_use]
    pub fn kind(&self) -> ActivityKind {
        self.kind
    }

    #[must_use]
    pub fn effective_at(&self) -> &Timestamp {
        &self.effective_at
    }

    #[must_use]
    pub fn effective_local_date(&self) -> CalendarDate {
        self.effective_local_date
    }

    #[must_use]
    pub fn created_at(&self) -> &Timestamp {
        &self.created_at
    }

    #[must_use]
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    #[must_use]
    pub fn reverses(&self) -> Option<ActivityId> {
        self.reverses
    }

    #[must_use]
    pub fn corrects(&self) -> Option<ActivityId> {
        self.corrects
    }

    #[must_use]
    pub fn correction_group(&self) -> Option<uuid::Uuid> {
        self.correction_group
    }

    #[must_use]
    pub fn income_kind(&self) -> Option<IncomeKind> {
        self.income_kind
    }

    #[must_use]
    pub fn fee_kind(&self) -> Option<FeeKind> {
        self.fee_kind
    }

    #[must_use]
    pub fn related_instrument_id(&self) -> Option<InstrumentId> {
        self.related_instrument_id
    }

    #[must_use]
    pub fn legs(&self) -> &[ActivityLeg] {
        &self.legs
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_persisted(
        id: ActivityId,
        household_id: HouseholdId,
        kind: ActivityKind,
        effective_at: Timestamp,
        effective_local_date: CalendarDate,
        created_at: Timestamp,
        note: Option<String>,
        reverses: Option<ActivityId>,
        corrects: Option<ActivityId>,
        correction_group: Option<uuid::Uuid>,
        income_kind: Option<IncomeKind>,
        fee_kind: Option<FeeKind>,
        related_instrument_id: Option<InstrumentId>,
        legs: Vec<ActivityLeg>,
    ) -> Result<Self, AppError> {
        if legs.is_empty() {
            return Err(AppError::invalid_activity_legs(
                "An activity must include at least one leg.",
            ));
        }
        Ok(Self {
            id,
            household_id,
            kind,
            effective_at,
            effective_local_date,
            created_at,
            note,
            reverses,
            corrects,
            correction_group,
            income_kind,
            fee_kind,
            related_instrument_id,
            legs,
        })
    }

    #[must_use]
    pub fn classification_for(&self, leg: &ActivityLeg) -> Classification {
        classify(self.kind, leg.role())
    }

    /// Deterministic listing/application tie-break:
    /// `effective_at DESC, created_at DESC, id DESC`.
    #[must_use]
    pub fn cmp_desc(&self, other: &Self) -> Ordering {
        self.effective_at
            .cmp(&other.effective_at)
            .then(self.created_at.cmp(&other.created_at))
            .then(self.id.cmp(&other.id))
            .reverse()
    }

    pub fn opening_adjustment(
        params: &ActivityRecordParams<'_>,
        opening: ComponentOpening,
    ) -> Result<ConstructActivity, AppError> {
        if opening.is_zero() {
            return Ok(ConstructActivity::NoActivity);
        }
        let id = ActivityId::new();
        let (account_id, component) = opening.into_leg_parts();
        let leg = ActivityLeg::new(
            id,
            account_id,
            LegRole::Adjustment,
            Direction::Increase,
            component,
            None,
            0,
        )?;
        Ok(ConstructActivity::Posted(assemble(
            id,
            params,
            ActivityKind::OpeningAdjustment,
            vec![leg],
            ActivityExtras::none(),
        )?))
    }

    pub fn balance_adjustment(
        params: &ActivityRecordParams<'_>,
        endpoint: MonetaryEndpoint,
        current: Money,
        target: Money,
    ) -> Result<Self, AppError> {
        absolute_money_activity(
            params,
            ActivityKind::BalanceAdjustment,
            endpoint,
            current,
            target,
        )
    }

    pub fn debt_adjustment(
        params: &ActivityRecordParams<'_>,
        account_id: AccountId,
        current: Money,
        target: Money,
    ) -> Result<Self, AppError> {
        absolute_money_activity(
            params,
            ActivityKind::DebtAdjustment,
            MonetaryEndpoint {
                account_id,
                component: super::activity_leg::MonetaryComponent::AccountValue,
            },
            current,
            target,
        )
    }

    pub fn manual_valuation(
        params: &ActivityRecordParams<'_>,
        account_id: AccountId,
        current: Money,
        target: Money,
    ) -> Result<Self, AppError> {
        absolute_money_activity(
            params,
            ActivityKind::ManualValuation,
            MonetaryEndpoint {
                account_id,
                component: super::activity_leg::MonetaryComponent::AccountValue,
            },
            current,
            target,
        )
    }

    pub fn position_adjustment(
        params: &ActivityRecordParams<'_>,
        endpoint: QuantityEndpoint,
        current: Quantity,
        target: Quantity,
    ) -> Result<Self, AppError> {
        let (direction, quantity) = derive_quantity_delta(current, target)?;
        let id = ActivityId::new();
        let leg = ActivityLeg::new(
            id,
            endpoint.account_id,
            LegRole::Adjustment,
            direction,
            LegComponent::HoldingQuantity {
                instrument_id: endpoint.instrument_id,
                holding_id: endpoint.holding_id,
                quantity,
            },
            None,
            0,
        )?;
        assemble(
            id,
            params,
            ActivityKind::PositionAdjustment,
            vec![leg],
            ActivityExtras::none(),
        )
    }

    pub fn deposit(
        params: &ActivityRecordParams<'_>,
        endpoint: MonetaryEndpoint,
        amount: Money,
    ) -> Result<Self, AppError> {
        monetary_single(
            params,
            ActivityKind::Deposit,
            endpoint,
            LegRole::Destination,
            Direction::Increase,
            amount,
            ActivityExtras::none(),
        )
    }

    pub fn withdrawal(
        params: &ActivityRecordParams<'_>,
        endpoint: MonetaryEndpoint,
        amount: Money,
    ) -> Result<Self, AppError> {
        monetary_single(
            params,
            ActivityKind::Withdrawal,
            endpoint,
            LegRole::Source,
            Direction::Decrease,
            amount,
            ActivityExtras::none(),
        )
    }

    pub fn income(
        params: &ActivityRecordParams<'_>,
        endpoint: MonetaryEndpoint,
        amount: Money,
        kind: IncomeKind,
        instrument_id: Option<InstrumentId>,
    ) -> Result<Self, AppError> {
        monetary_single(
            params,
            ActivityKind::Income,
            endpoint,
            LegRole::Income,
            Direction::Increase,
            amount,
            ActivityExtras {
                income_kind: Some(kind),
                related_instrument_id: instrument_id,
                ..ActivityExtras::none()
            },
        )
    }

    pub fn fee(
        params: &ActivityRecordParams<'_>,
        endpoint: MonetaryEndpoint,
        amount: Money,
        kind: FeeKind,
        instrument_id: Option<InstrumentId>,
    ) -> Result<Self, AppError> {
        monetary_single(
            params,
            ActivityKind::Fee,
            endpoint,
            LegRole::Fee,
            Direction::Decrease,
            amount,
            ActivityExtras {
                fee_kind: Some(kind),
                related_instrument_id: instrument_id,
                ..ActivityExtras::none()
            },
        )
    }

    pub fn cash_transfer(
        params: &ActivityRecordParams<'_>,
        source: MonetaryEndpoint,
        destination: MonetaryEndpoint,
        source_amount: Money,
        destination_amount: Money,
        fx_rate: Option<FxRate>,
    ) -> Result<Self, AppError> {
        require_positive_money(source_amount)?;
        require_positive_money(destination_amount)?;
        if source.account_id == destination.account_id
            && source.component == destination.component
            && source_amount.currency() == destination_amount.currency()
        {
            return Err(AppError::invalid_activity_legs(
                "Transfer source and destination must be different endpoints.",
            ));
        }
        let fx_rate = match_transfer_amounts(source_amount, destination_amount, fx_rate)?;
        let id = ActivityId::new();
        let source_leg = ActivityLeg::new(
            id,
            source.account_id,
            LegRole::Source,
            Direction::Decrease,
            source.component.into_leg_component(source_amount),
            fx_rate,
            0,
        )?;
        let destination_leg = ActivityLeg::new(
            id,
            destination.account_id,
            LegRole::Destination,
            Direction::Increase,
            destination.component.into_leg_component(destination_amount),
            fx_rate,
            1,
        )?;
        assemble(
            id,
            params,
            ActivityKind::Transfer,
            vec![source_leg, destination_leg],
            ActivityExtras::none(),
        )
    }

    pub fn position_transfer(
        params: &ActivityRecordParams<'_>,
        source: QuantityEndpoint,
        destination: QuantityEndpoint,
        quantity: Quantity,
    ) -> Result<Self, AppError> {
        if quantity.is_zero() {
            return Err(AppError::invalid_activity(
                "Position transfers require a positive quantity.",
            ));
        }
        if source.instrument_id != destination.instrument_id {
            return Err(AppError::invalid_activity_legs(
                "Position transfers must use the same instrument.",
            ));
        }
        if source.holding_id == destination.holding_id {
            return Err(AppError::invalid_activity_legs(
                "Position transfer source and destination holdings must differ.",
            ));
        }
        let id = ActivityId::new();
        let source_leg = ActivityLeg::new(
            id,
            source.account_id,
            LegRole::Source,
            Direction::Decrease,
            LegComponent::HoldingQuantity {
                instrument_id: source.instrument_id,
                holding_id: source.holding_id,
                quantity,
            },
            None,
            0,
        )?;
        let destination_leg = ActivityLeg::new(
            id,
            destination.account_id,
            LegRole::Destination,
            Direction::Increase,
            LegComponent::HoldingQuantity {
                instrument_id: destination.instrument_id,
                holding_id: destination.holding_id,
                quantity,
            },
            None,
            1,
        )?;
        assemble(
            id,
            params,
            ActivityKind::Transfer,
            vec![source_leg, destination_leg],
            ActivityExtras::none(),
        )
    }

    pub fn buy(params: &ActivityRecordParams<'_>, spec: TradeSpec) -> Result<Self, AppError> {
        trade(params, ActivityKind::Buy, spec)
    }

    pub fn sell(params: &ActivityRecordParams<'_>, spec: TradeSpec) -> Result<Self, AppError> {
        trade(params, ActivityKind::Sell, spec)
    }

    pub fn debt_draw(
        params: &ActivityRecordParams<'_>,
        spec: DebtDrawSpec,
    ) -> Result<Self, AppError> {
        require_positive_money(spec.principal)?;
        let id = ActivityId::new();
        let mut legs = vec![ActivityLeg::new(
            id,
            spec.liability_account_id,
            LegRole::Liability,
            Direction::Increase,
            LegComponent::AccountValue {
                amount: spec.principal,
            },
            None,
            0,
        )?];
        if let Some(cash) = spec.cash {
            require_positive_money(cash.amount)?;
            let fx_rate = match_transfer_amounts(spec.principal, cash.amount, cash.fx_rate)?;
            legs[0] = ActivityLeg::new(
                id,
                spec.liability_account_id,
                LegRole::Liability,
                Direction::Increase,
                LegComponent::AccountValue {
                    amount: spec.principal,
                },
                fx_rate,
                0,
            )?;
            legs.push(ActivityLeg::new(
                id,
                cash.endpoint.account_id,
                LegRole::Destination,
                Direction::Increase,
                cash.endpoint.component.into_leg_component(cash.amount),
                fx_rate,
                1,
            )?);
        }
        assemble(
            id,
            params,
            ActivityKind::DebtDraw,
            legs,
            ActivityExtras::none(),
        )
    }

    pub fn debt_payment(
        params: &ActivityRecordParams<'_>,
        spec: DebtPaymentSpec,
    ) -> Result<Self, AppError> {
        require_positive_money(spec.principal)?;
        require_positive_money(spec.cash.amount)?;
        if let Some(fee) = spec.fee {
            require_positive_money(fee)?;
        }
        let fx_rate = match_transfer_amounts(spec.principal, spec.cash.amount, spec.cash.fx_rate)?;
        let id = ActivityId::new();
        let mut legs = vec![
            ActivityLeg::new(
                id,
                spec.liability_account_id,
                LegRole::Liability,
                Direction::Decrease,
                LegComponent::AccountValue {
                    amount: spec.principal,
                },
                fx_rate,
                0,
            )?,
            ActivityLeg::new(
                id,
                spec.cash.endpoint.account_id,
                LegRole::Source,
                Direction::Decrease,
                spec.cash
                    .endpoint
                    .component
                    .into_leg_component(spec.cash.amount),
                fx_rate,
                1,
            )?,
        ];
        if let Some(fee) = spec.fee {
            if fee.currency() != spec.cash.amount.currency() {
                return Err(AppError::invalid_activity_legs(
                    "A debt payment fee must use the cash currency.",
                ));
            }
            legs.push(ActivityLeg::new(
                id,
                spec.cash.endpoint.account_id,
                LegRole::Fee,
                Direction::Decrease,
                spec.cash.endpoint.component.into_leg_component(fee),
                None,
                2,
            )?);
        }
        assemble(
            id,
            params,
            ActivityKind::DebtPayment,
            legs,
            ActivityExtras {
                fee_kind: spec.fee_kind,
                ..ActivityExtras::none()
            },
        )
    }

    pub fn reversal(
        params: &ActivityRecordParams<'_>,
        original: &Activity,
    ) -> Result<Self, AppError> {
        let id = ActivityId::new();
        let legs = inverse_legs(original)
            .into_iter()
            .map(|leg| leg.with_activity_id(id))
            .collect();
        assemble(
            id,
            params,
            ActivityKind::Reversal,
            legs,
            ActivityExtras {
                reverses: Some(original.id),
                ..ActivityExtras::none()
            },
        )
    }

    #[must_use]
    pub fn with_correction_group(mut self, group: uuid::Uuid) -> Self {
        self.correction_group = Some(group);
        self
    }

    #[must_use]
    pub fn with_corrects(mut self, original_id: ActivityId, group: uuid::Uuid) -> Self {
        self.corrects = Some(original_id);
        self.correction_group = Some(group);
        self
    }
}

/// Exact inverse facts: swapped direction, same magnitudes, accounts, instruments, and FX.
#[must_use]
pub fn inverse_legs(activity: &Activity) -> Vec<ActivityLeg> {
    activity.legs.iter().map(ActivityLeg::inverse).collect()
}

struct ActivityExtras {
    income_kind: Option<IncomeKind>,
    fee_kind: Option<FeeKind>,
    related_instrument_id: Option<InstrumentId>,
    reverses: Option<ActivityId>,
}

impl ActivityExtras {
    fn none() -> Self {
        Self {
            income_kind: None,
            fee_kind: None,
            related_instrument_id: None,
            reverses: None,
        }
    }
}

fn assemble(
    id: ActivityId,
    params: &ActivityRecordParams<'_>,
    kind: ActivityKind,
    legs: Vec<ActivityLeg>,
    extras: ActivityExtras,
) -> Result<Activity, AppError> {
    if legs.is_empty() {
        return Err(AppError::invalid_activity_legs(
            "An activity must include at least one leg.",
        ));
    }
    Ok(Activity {
        id,
        household_id: params.household_id,
        kind,
        effective_at: params.effective_at.clone(),
        effective_local_date: params.effective_local_date,
        created_at: params.created_at.clone(),
        note: parse_optional_note(params.note)?,
        reverses: extras.reverses,
        corrects: None,
        correction_group: None,
        income_kind: extras.income_kind,
        fee_kind: extras.fee_kind,
        related_instrument_id: extras.related_instrument_id,
        legs,
    })
}

fn monetary_single(
    params: &ActivityRecordParams<'_>,
    kind: ActivityKind,
    endpoint: MonetaryEndpoint,
    role: LegRole,
    direction: Direction,
    amount: Money,
    extras: ActivityExtras,
) -> Result<Activity, AppError> {
    require_positive_money(amount)?;
    let id = ActivityId::new();
    let leg = ActivityLeg::new(
        id,
        endpoint.account_id,
        role,
        direction,
        endpoint.component.into_leg_component(amount),
        None,
        0,
    )?;
    assemble(id, params, kind, vec![leg], extras)
}

fn absolute_money_activity(
    params: &ActivityRecordParams<'_>,
    kind: ActivityKind,
    endpoint: MonetaryEndpoint,
    current: Money,
    target: Money,
) -> Result<Activity, AppError> {
    let (direction, amount) = derive_money_delta(current, target)?;
    let id = ActivityId::new();
    let leg = ActivityLeg::new(
        id,
        endpoint.account_id,
        LegRole::Adjustment,
        direction,
        endpoint.component.into_leg_component(amount),
        None,
        0,
    )?;
    assemble(id, params, kind, vec![leg], ActivityExtras::none())
}

fn trade(
    params: &ActivityRecordParams<'_>,
    kind: ActivityKind,
    spec: TradeSpec,
) -> Result<Activity, AppError> {
    if spec.quantity.is_zero() {
        return Err(AppError::invalid_activity(
            "Trade quantity must be greater than zero.",
        ));
    }
    if spec.unit_price.is_zero() && !spec.confirm_zero_unit_price {
        return Err(AppError::invalid_activity(
            "A zero unit price trade requires explicit confirmation.",
        ));
    }
    if spec.settlement_currency != spec.quote_currency {
        return Err(AppError::invalid_activity(
            "Settlement currency must equal the instrument quote currency.",
        ));
    }
    if spec.gross_amount.currency() != spec.settlement_currency {
        return Err(AppError::trade_total_mismatch(
            "Gross amount currency must equal the settlement currency.",
        ));
    }
    let expected = expected_gross(spec.quantity, spec.unit_price, spec.settlement_currency)?;
    if expected != spec.gross_amount {
        return Err(AppError::trade_total_mismatch(
            "Gross amount must equal quantity multiplied by unit price.",
        ));
    }
    if let Some(fee) = spec.fee {
        require_positive_money(fee)?;
        if fee.currency() != spec.settlement_currency {
            return Err(AppError::invalid_activity_legs(
                "An explicit trade fee must use the settlement currency.",
            ));
        }
    }
    let id = ActivityId::new();
    let (holding_direction, settlement_direction) = match kind {
        ActivityKind::Buy => (Direction::Increase, Direction::Decrease),
        ActivityKind::Sell => (Direction::Decrease, Direction::Increase),
        _ => {
            return Err(AppError::invalid_activity(
                "Trade construction requires buy or sell.",
            ));
        }
    };
    let mut legs = vec![ActivityLeg::new(
        id,
        spec.account_id,
        LegRole::Holding,
        holding_direction,
        LegComponent::HoldingQuantity {
            instrument_id: spec.instrument_id,
            holding_id: spec.holding_id,
            quantity: spec.quantity,
        },
        None,
        0,
    )?];
    if !spec.gross_amount.is_zero() {
        legs.push(ActivityLeg::new(
            id,
            spec.account_id,
            LegRole::Settlement,
            settlement_direction,
            LegComponent::HoldingsCash {
                amount: spec.gross_amount,
            },
            None,
            1,
        )?);
    }
    if let Some(fee) = spec.fee {
        legs.push(ActivityLeg::new(
            id,
            spec.account_id,
            LegRole::Fee,
            Direction::Decrease,
            LegComponent::HoldingsCash { amount: fee },
            None,
            2,
        )?);
    }
    assemble(
        id,
        params,
        kind,
        legs,
        ActivityExtras {
            related_instrument_id: Some(spec.instrument_id),
            ..ActivityExtras::none()
        },
    )
}

fn expected_gross(
    quantity: Quantity,
    unit_price: UnitPrice,
    currency: CurrencyCode,
) -> Result<Money, AppError> {
    let product = checked_mul(quantity.amount(), unit_price.amount())?;
    Money::from_canonical(product, currency)
}

fn match_transfer_amounts(
    source: Money,
    destination: Money,
    fx_rate: Option<FxRate>,
) -> Result<Option<FxRate>, AppError> {
    if source.currency() == destination.currency() {
        if fx_rate.is_some() {
            return Err(AppError::invalid_activity_legs(
                "Same-currency transfers cannot include an FX rate.",
            ));
        }
        if source != destination {
            return Err(AppError::transfer_mismatch(
                "Same-currency transfer amounts must match exactly.",
            ));
        }
        return Ok(None);
    }
    let Some(rate) = fx_rate else {
        return Err(AppError::transfer_mismatch(
            "Cross-currency transfers require an explicit FX rate.",
        ));
    };
    let converted = convert_with_direct_rate(source.amount(), rate)?;
    let rounded = Money::from_canonical(converted, destination.currency())?;
    if rounded != destination {
        return Err(AppError::transfer_mismatch(
            "The destination amount must equal the source amount converted at the recorded FX rate.",
        ));
    }
    Ok(Some(rate))
}

fn derive_money_delta(current: Money, target: Money) -> Result<(Direction, Money), AppError> {
    if current.currency() != target.currency() {
        return Err(AppError::invalid_activity(
            "The target currency must match the current currency.",
        ));
    }
    if current == target {
        return Err(AppError::invalid_activity("The target is unchanged."));
    }
    if target.amount() > current.amount() {
        let delta = checked_sub(target.amount(), current.amount())?;
        Ok((
            Direction::Increase,
            Money::from_canonical(delta, current.currency())?,
        ))
    } else {
        let delta = checked_sub(current.amount(), target.amount())?;
        Ok((
            Direction::Decrease,
            Money::from_canonical(delta, current.currency())?,
        ))
    }
}

fn derive_quantity_delta(
    current: Quantity,
    target: Quantity,
) -> Result<(Direction, Quantity), AppError> {
    if current == target {
        return Err(AppError::invalid_activity("The target is unchanged."));
    }
    if target.amount() > current.amount() {
        let delta = checked_sub(target.amount(), current.amount())?;
        Ok((Direction::Increase, Quantity::from_canonical(delta)?))
    } else {
        let delta = checked_sub(current.amount(), target.amount())?;
        Ok((Direction::Decrease, Quantity::from_canonical(delta)?))
    }
}

fn require_positive_money(amount: Money) -> Result<Money, AppError> {
    if amount.is_zero() {
        return Err(AppError::invalid_activity("The amount must be positive."));
    }
    Ok(amount)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::activity_leg::{
        apply_money_legs, apply_quantity_legs, ComponentKind, MonetaryComponent,
    };
    use crate::domain::ids::{HoldingId, HouseholdId};
    use crate::error::{AppError, ErrorCode};
    use uuid::Uuid;

    fn params() -> ActivityRecordParams<'static> {
        ActivityRecordParams {
            household_id: HouseholdId::new(),
            effective_at: Timestamp::parse("2026-06-01T12:00:00.000Z").expect("effective"),
            effective_local_date: CalendarDate::parse("2026-06-01").expect("date"),
            created_at: Timestamp::parse("2026-06-01T12:00:01.000Z").expect("created"),
            note: None,
        }
    }

    fn usd(amount: &str) -> Money {
        Money::parse(amount, CurrencyCode::USD).expect("usd")
    }

    fn cny(amount: &str) -> Money {
        Money::parse(amount, CurrencyCode::CNY).expect("cny")
    }

    fn qty(amount: &str) -> Quantity {
        Quantity::parse(amount).expect("qty")
    }

    fn price(amount: &str) -> UnitPrice {
        UnitPrice::parse(amount).expect("price")
    }

    fn cash_endpoint(account_id: AccountId) -> MonetaryEndpoint {
        MonetaryEndpoint {
            account_id,
            component: MonetaryComponent::HoldingsCash,
        }
    }

    fn balance_endpoint(account_id: AccountId) -> MonetaryEndpoint {
        MonetaryEndpoint {
            account_id,
            component: MonetaryComponent::AccountValue,
        }
    }

    fn usd_trade(
        account_id: AccountId,
        holding_id: HoldingId,
        instrument_id: InstrumentId,
    ) -> TradeSpec {
        TradeSpec {
            account_id,
            holding_id,
            instrument_id,
            quantity: qty("2"),
            unit_price: price("100"),
            quote_currency: CurrencyCode::USD,
            gross_amount: usd("200"),
            settlement_currency: CurrencyCode::USD,
            fee: None,
            confirm_zero_unit_price: false,
        }
    }

    fn assert_closed_enum<T, F, G>(parse: F, as_str: G, known: &[&str])
    where
        F: Fn(&str) -> Result<T, AppError>,
        G: Fn(T) -> &'static str,
        T: Copy,
    {
        for value in known {
            let parsed = parse(value).unwrap_or_else(|_| panic!("parse {value}"));
            assert_eq!(as_str(parsed), *value, "{value}");
        }
        assert!(parse("unknown_value").is_err());
        assert!(parse("").is_err());
        assert!(parse("OPENING_ADJUSTMENT").is_err());
    }

    #[test]
    fn closed_enums_parse_snake_case_and_reject_unknown_values() {
        assert_closed_enum(
            ActivityKind::parse,
            ActivityKind::as_str,
            &[
                "opening_adjustment",
                "balance_adjustment",
                "position_adjustment",
                "deposit",
                "withdrawal",
                "transfer",
                "buy",
                "sell",
                "income",
                "fee",
                "debt_draw",
                "debt_payment",
                "debt_adjustment",
                "manual_valuation",
                "reversal",
            ],
        );
        assert_closed_enum(
            LegRole::parse,
            LegRole::as_str,
            &[
                "source",
                "destination",
                "holding",
                "settlement",
                "fee",
                "income",
                "liability",
                "adjustment",
            ],
        );
        assert_closed_enum(
            ComponentKind::parse,
            ComponentKind::as_str,
            &["account_value", "holdings_cash", "holding_quantity"],
        );
        assert_closed_enum(
            Direction::parse,
            Direction::as_str,
            &["increase", "decrease"],
        );
        assert_closed_enum(
            Classification::parse,
            Classification::as_str,
            &[
                "external_inflow",
                "external_outflow",
                "internal_transfer",
                "trade_principal",
                "income",
                "fee",
                "debt_principal",
                "remeasurement",
            ],
        );
        assert_closed_enum(
            IncomeKind::parse,
            IncomeKind::as_str,
            &[
                "salary", "bonus", "dividend", "interest", "rental", "pension", "gift", "refund",
                "other",
            ],
        );
        assert_closed_enum(
            FeeKind::parse,
            FeeKind::as_str,
            &[
                "bank_fee",
                "account_fee",
                "brokerage_commission",
                "management_fee",
                "foreign_exchange_fee",
                "interest",
                "tax",
                "other",
            ],
        );
    }

    #[test]
    fn derived_classification_cannot_be_caller_controlled() {
        let source = AccountId::new();
        let destination = AccountId::new();
        let transfer = Activity::cash_transfer(
            &params(),
            balance_endpoint(source),
            balance_endpoint(destination),
            cny("3000"),
            cny("3000"),
            None,
        )
        .expect("transfer");
        assert!(transfer
            .legs()
            .iter()
            .all(|leg| transfer.classification_for(leg) == Classification::InternalTransfer));
        let wanted_external = Classification::parse("external_inflow").expect("class");
        assert_ne!(
            wanted_external,
            transfer.classification_for(&transfer.legs()[0])
        );
        assert_eq!(
            classify(ActivityKind::Deposit, LegRole::Source),
            Classification::ExternalInflow
        );
        assert_eq!(
            classify(ActivityKind::Buy, LegRole::Fee),
            Classification::Fee
        );
        assert_eq!(
            classify(ActivityKind::Buy, LegRole::Settlement),
            Classification::TradePrincipal
        );
        assert_eq!(
            classify(ActivityKind::DebtPayment, LegRole::Liability),
            Classification::DebtPrincipal
        );
        assert_eq!(
            classify(ActivityKind::BalanceAdjustment, LegRole::Adjustment),
            Classification::Remeasurement
        );
    }

    #[test]
    fn same_currency_transfer_requires_exact_equality() {
        let source = AccountId::new();
        let destination = AccountId::new();
        let activity = Activity::cash_transfer(
            &params(),
            balance_endpoint(source),
            balance_endpoint(destination),
            cny("3000"),
            cny("3000"),
            None,
        )
        .expect("equal transfer");
        assert_eq!(activity.kind(), ActivityKind::Transfer);
        assert_eq!(activity.legs().len(), 2);
        assert_eq!(activity.legs()[0].direction(), Direction::Decrease);
        assert_eq!(activity.legs()[1].direction(), Direction::Increase);
        assert_eq!(
            activity.legs()[0]
                .component()
                .money()
                .expect("src")
                .canonical_amount(),
            "3000"
        );
        assert_eq!(
            activity.legs()[1]
                .component()
                .money()
                .expect("dst")
                .canonical_amount(),
            "3000"
        );
        let mismatch = Activity::cash_transfer(
            &params(),
            balance_endpoint(source),
            balance_endpoint(destination),
            cny("3000"),
            cny("3001"),
            None,
        )
        .expect_err("mismatch");
        assert!(matches!(mismatch, AppError::TransferMismatch { .. }));
        assert_eq!(
            mismatch.into_command_error().code,
            ErrorCode::TransferMismatch
        );
    }

    #[test]
    fn cross_currency_transfer_uses_one_time_money_rounding() {
        let source = AccountId::new();
        let destination = AccountId::new();
        let rate = FxRate::parse("6.9").expect("rate");
        let activity = Activity::cash_transfer(
            &params(),
            cash_endpoint(source),
            cash_endpoint(destination),
            usd("100"),
            cny("690"),
            Some(rate),
        )
        .expect("golden fx transfer");
        assert_eq!(activity.legs()[0].direction(), Direction::Decrease);
        assert_eq!(activity.legs()[1].direction(), Direction::Increase);
        assert_eq!(
            activity.legs()[0]
                .component()
                .money()
                .expect("src")
                .canonical_amount(),
            "100"
        );
        assert_eq!(
            activity.legs()[1]
                .component()
                .money()
                .expect("dst")
                .canonical_amount(),
            "690"
        );
        assert_eq!(
            activity.legs()[0].fx_rate().expect("src fx").canonical(),
            "6.9"
        );
        assert_eq!(
            activity.legs()[1].fx_rate().expect("dst fx").canonical(),
            "6.9"
        );
        assert!(activity
            .legs()
            .iter()
            .all(|leg| activity.classification_for(leg) == Classification::InternalTransfer));

        let mismatch = Activity::cash_transfer(
            &params(),
            cash_endpoint(source),
            cash_endpoint(destination),
            usd("100"),
            cny("691"),
            Some(rate),
        )
        .expect_err("rate mismatch");
        assert!(matches!(mismatch, AppError::TransferMismatch { .. }));

        let rounded = Activity::cash_transfer(
            &params(),
            cash_endpoint(source),
            cash_endpoint(destination),
            usd("1"),
            cny("1.3333"),
            Some(FxRate::parse("1.33333").expect("round rate")),
        )
        .expect("rounded once to money scale");
        assert_eq!(
            rounded.legs()[1]
                .component()
                .money()
                .expect("rounded dest")
                .canonical_amount(),
            "1.3333"
        );
        assert!(Activity::cash_transfer(
            &params(),
            cash_endpoint(source),
            cash_endpoint(destination),
            usd("1"),
            cny("1.3334"),
            Some(FxRate::parse("1.33333").expect("round rate")),
        )
        .is_err());
    }

    #[test]
    fn position_transfer_requires_same_instrument_and_exact_quantity() {
        let instrument = InstrumentId::new();
        let other = InstrumentId::new();
        let source = QuantityEndpoint {
            account_id: AccountId::new(),
            holding_id: HoldingId::new(),
            instrument_id: instrument,
        };
        let destination = QuantityEndpoint {
            account_id: AccountId::new(),
            holding_id: HoldingId::new(),
            instrument_id: instrument,
        };
        let activity =
            Activity::position_transfer(&params(), source, destination, qty("3")).expect("qty");
        assert_eq!(activity.legs().len(), 2);
        assert_eq!(
            activity.legs()[0]
                .component()
                .quantity()
                .expect("src")
                .canonical(),
            "3"
        );
        assert_eq!(
            activity.legs()[1]
                .component()
                .quantity()
                .expect("dst")
                .canonical(),
            "3"
        );
        let wrong_instrument = QuantityEndpoint {
            instrument_id: other,
            ..destination
        };
        assert!(
            Activity::position_transfer(&params(), source, wrong_instrument, qty("3")).is_err()
        );
        assert!(Activity::position_transfer(&params(), source, destination, qty("0")).is_err());
    }

    #[test]
    fn golden_buy_updates_cash_quantity_principal_and_fee() {
        let account = AccountId::new();
        let holding = HoldingId::new();
        let instrument = InstrumentId::new();
        let buy = Activity::buy(
            &params(),
            TradeSpec {
                fee: Some(usd("5")),
                ..usd_trade(account, holding, instrument)
            },
        )
        .expect("golden buy");
        assert_eq!(buy.kind(), ActivityKind::Buy);
        let cash = apply_money_legs(
            usd("1000"),
            buy.legs()
                .iter()
                .filter(|leg| matches!(leg.component(), LegComponent::HoldingsCash { .. }))
                .cloned()
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .expect("cash");
        assert_eq!(cash.canonical_amount(), "795");
        let quantity = apply_quantity_legs(
            qty("0"),
            buy.legs()
                .iter()
                .filter(|leg| matches!(leg.component(), LegComponent::HoldingQuantity { .. }))
                .cloned()
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .expect("qty");
        assert_eq!(quantity.canonical(), "2");
        let principal: Vec<_> = buy
            .legs()
            .iter()
            .filter(|leg| buy.classification_for(leg) == Classification::TradePrincipal)
            .collect();
        assert_eq!(principal.len(), 2);
        assert_eq!(
            buy.legs()
                .iter()
                .find(|leg| leg.role() == LegRole::Settlement)
                .expect("settlement")
                .component()
                .money()
                .expect("gross")
                .canonical_amount(),
            "200"
        );
        assert_eq!(
            buy.legs()
                .iter()
                .find(|leg| buy.classification_for(leg) == Classification::Fee)
                .expect("fee")
                .component()
                .money()
                .expect("fee")
                .canonical_amount(),
            "5"
        );
    }

    #[test]
    fn buy_and_sell_validate_gross_settlement_zero_quantity_and_state() {
        let account = AccountId::new();
        let holding = HoldingId::new();
        let instrument = InstrumentId::new();
        let mismatch = Activity::buy(
            &params(),
            TradeSpec {
                gross_amount: usd("201"),
                ..usd_trade(account, holding, instrument)
            },
        )
        .expect_err("gross");
        assert!(matches!(mismatch, AppError::TradeTotalMismatch { .. }));
        assert_eq!(
            mismatch.into_command_error().code,
            ErrorCode::TradeTotalMismatch
        );

        let mut wrong_settlement = usd_trade(account, holding, instrument);
        wrong_settlement.settlement_currency = CurrencyCode::CNY;
        assert!(Activity::buy(&params(), wrong_settlement).is_err());

        assert!(Activity::buy(
            &params(),
            TradeSpec {
                quantity: qty("0"),
                gross_amount: usd("0"),
                ..usd_trade(account, holding, instrument)
            },
        )
        .is_err());

        assert!(Activity::buy(
            &params(),
            TradeSpec {
                unit_price: price("0"),
                gross_amount: usd("0"),
                ..usd_trade(account, holding, instrument)
            },
        )
        .is_err());
        Activity::buy(
            &params(),
            TradeSpec {
                unit_price: price("0"),
                gross_amount: usd("0"),
                confirm_zero_unit_price: true,
                ..usd_trade(account, holding, instrument)
            },
        )
        .expect("confirmed zero price");

        let sell = Activity::sell(
            &params(),
            TradeSpec {
                fee: Some(usd("5")),
                ..usd_trade(account, holding, instrument)
            },
        )
        .expect("sell");
        let cash = apply_money_legs(
            usd("0"),
            sell.legs()
                .iter()
                .filter(|leg| matches!(leg.component(), LegComponent::HoldingsCash { .. }))
                .cloned()
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .expect("sell cash");
        assert_eq!(cash.canonical_amount(), "195");
        let insufficient_qty = apply_quantity_legs(qty("1"), &sell.legs()[0..1]).expect_err("qty");
        assert!(matches!(insufficient_qty, AppError::InsufficientQuantity));
        assert_eq!(
            insufficient_qty.into_command_error().code,
            ErrorCode::InsufficientQuantity
        );

        let buy = Activity::buy(
            &params(),
            TradeSpec {
                fee: Some(usd("5")),
                ..usd_trade(account, holding, instrument)
            },
        )
        .expect("buy");
        let insufficient_cash = apply_money_legs(
            usd("100"),
            buy.legs()
                .iter()
                .filter(|leg| matches!(leg.component(), LegComponent::HoldingsCash { .. }))
                .cloned()
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .expect_err("cash");
        assert!(matches!(insufficient_cash, AppError::InsufficientBalance));
        assert_eq!(
            insufficient_cash.into_command_error().code,
            ErrorCode::InsufficientBalance
        );

        let overflow_buy = Activity::buy(
            &params(),
            TradeSpec {
                quantity: qty("999999999999"),
                unit_price: price("999999999999"),
                gross_amount: usd("1"),
                ..usd_trade(account, holding, instrument)
            },
        );
        assert!(matches!(
            overflow_buy.expect_err("overflow"),
            AppError::DecimalOverflow
                | AppError::TradeTotalMismatch { .. }
                | AppError::InvalidMoney { .. }
        ));
        let overflow_apply = Activity::deposit(&params(), cash_endpoint(account), usd("0.0001"))
            .expect("tiny deposit")
            .legs()[0]
            .apply_to_money(usd("999999999999.9999"))
            .expect_err("money overflow");
        assert!(matches!(overflow_apply, AppError::DecimalOverflow));
    }

    #[test]
    fn debt_draw_and_payment_separate_principal_from_fee() {
        let liability = AccountId::new();
        let cash_account = AccountId::new();
        let draw = Activity::debt_draw(
            &params(),
            DebtDrawSpec {
                liability_account_id: liability,
                principal: usd("1000"),
                cash: Some(DebtCashLink {
                    endpoint: cash_endpoint(cash_account),
                    amount: usd("1000"),
                    fx_rate: None,
                }),
            },
        )
        .expect("draw");
        assert_eq!(
            draw.classification_for(&draw.legs()[0]),
            Classification::DebtPrincipal
        );
        assert_eq!(
            draw.classification_for(&draw.legs()[1]),
            Classification::InternalTransfer
        );
        assert_eq!(draw.legs()[0].role(), LegRole::Liability);
        assert_eq!(draw.legs()[1].role(), LegRole::Destination);

        let payment = Activity::debt_payment(
            &params(),
            DebtPaymentSpec {
                liability_account_id: liability,
                principal: usd("1000"),
                cash: DebtCashLink {
                    endpoint: cash_endpoint(cash_account),
                    amount: usd("1000"),
                    fx_rate: None,
                },
                fee: Some(usd("15")),
                fee_kind: Some(FeeKind::Interest),
            },
        )
        .expect("payment");
        assert_eq!(payment.legs().len(), 3);
        assert_eq!(
            payment.classification_for(&payment.legs()[0]),
            Classification::DebtPrincipal
        );
        assert_eq!(
            payment.classification_for(&payment.legs()[1]),
            Classification::InternalTransfer
        );
        assert_eq!(
            payment.classification_for(&payment.legs()[2]),
            Classification::Fee
        );
        assert_eq!(payment.fee_kind(), Some(FeeKind::Interest));
        let cash = apply_money_legs(
            usd("2000"),
            payment
                .legs()
                .iter()
                .filter(|leg| matches!(leg.component(), LegComponent::HoldingsCash { .. }))
                .cloned()
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .expect("cash after payment");
        assert_eq!(cash.canonical_amount(), "985");
        let principal = apply_money_legs(usd("1000"), &payment.legs()[0..1]).expect("liability");
        assert_eq!(principal.canonical_amount(), "0");
    }

    #[test]
    fn absolute_targets_derive_one_additive_leg() {
        let account = AccountId::new();
        let increase = Activity::balance_adjustment(
            &params(),
            balance_endpoint(account),
            cny("10000"),
            cny("13000"),
        )
        .expect("increase");
        assert_eq!(increase.legs().len(), 1);
        assert_eq!(increase.legs()[0].direction(), Direction::Increase);
        assert_eq!(
            increase.legs()[0]
                .component()
                .money()
                .expect("delta")
                .canonical_amount(),
            "3000"
        );
        assert_eq!(
            increase.classification_for(&increase.legs()[0]),
            Classification::Remeasurement
        );
        assert!(!increase
            .legs()
            .iter()
            .any(|leg| leg.direction().as_str() == "set" || leg.role().as_str() == "absolute"));

        let decrease = Activity::manual_valuation(&params(), account, cny("1000"), cny("400"))
            .expect("valuation");
        assert_eq!(decrease.legs()[0].direction(), Direction::Decrease);
        assert_eq!(
            decrease.legs()[0]
                .component()
                .money()
                .expect("delta")
                .canonical_amount(),
            "600"
        );

        let position = Activity::position_adjustment(
            &params(),
            QuantityEndpoint {
                account_id: account,
                holding_id: HoldingId::new(),
                instrument_id: InstrumentId::new(),
            },
            qty("3"),
            qty("5"),
        )
        .expect("position");
        assert_eq!(position.legs()[0].direction(), Direction::Increase);
        assert_eq!(
            position.legs()[0]
                .component()
                .quantity()
                .expect("delta")
                .canonical(),
            "2"
        );
        assert!(Activity::balance_adjustment(
            &params(),
            balance_endpoint(account),
            cny("100"),
            cny("100"),
        )
        .is_err());
        assert!(Activity::manual_valuation(&params(), account, cny("5"), cny("5")).is_err());
        assert!(Activity::debt_adjustment(&params(), account, usd("20"), usd("20")).is_err());
    }

    #[test]
    fn zero_state_component_creation_writes_no_activity() {
        let account = AccountId::new();
        match Activity::opening_adjustment(
            &params(),
            ComponentOpening::AccountValue {
                account_id: account,
                amount: cny("0"),
            },
        )
        .expect("zero account")
        {
            ConstructActivity::NoActivity => {}
            ConstructActivity::Posted(_) => panic!("zero state must not write an activity"),
        }
        match Activity::opening_adjustment(
            &params(),
            ComponentOpening::HoldingQuantity {
                account_id: account,
                holding_id: HoldingId::new(),
                instrument_id: InstrumentId::new(),
                quantity: qty("0"),
            },
        )
        .expect("zero holding")
        {
            ConstructActivity::NoActivity => {}
            ConstructActivity::Posted(_) => panic!("zero holding must not write an activity"),
        }
        match Activity::opening_adjustment(
            &params(),
            ComponentOpening::HoldingsCash {
                account_id: account,
                amount: usd("500"),
            },
        )
        .expect("positive opening")
        {
            ConstructActivity::Posted(activity) => {
                assert_eq!(activity.kind(), ActivityKind::OpeningAdjustment);
                assert_eq!(activity.legs()[0].direction(), Direction::Increase);
            }
            ConstructActivity::NoActivity => panic!("positive opening must post"),
        }
    }

    #[test]
    fn reversal_produces_exact_inverse_facts() {
        let source = AccountId::new();
        let destination = AccountId::new();
        let original = Activity::cash_transfer(
            &params(),
            cash_endpoint(source),
            cash_endpoint(destination),
            usd("100"),
            cny("690"),
            Some(FxRate::parse("6.9").expect("rate")),
        )
        .expect("original");
        let inverse = inverse_legs(&original);
        assert_eq!(inverse.len(), original.legs().len());
        for (left, right) in original.legs().iter().zip(inverse.iter()) {
            assert_eq!(left.account_id(), right.account_id());
            assert_eq!(left.role(), right.role());
            assert_eq!(left.component(), right.component());
            assert_eq!(left.fx_rate(), right.fx_rate());
            assert_eq!(left.sort_order(), right.sort_order());
            assert_eq!(right.direction(), left.direction().inverse());
            assert_eq!(
                left.component().money().expect("m").canonical_amount(),
                right.component().money().expect("m").canonical_amount()
            );
        }
        let restored = inverse_legs(&Activity {
            legs: inverse.clone(),
            ..original.clone()
        });
        for (left, right) in original.legs().iter().zip(restored.iter()) {
            assert_eq!(left.direction(), right.direction());
            assert_eq!(left.component(), right.component());
        }
        let after = apply_money_legs(usd("1000"), &original.legs()[0..1]).expect("src after");
        assert_eq!(after.canonical_amount(), "900");
        let undone = apply_money_legs(after, &inverse[0..1]).expect("src restored");
        assert_eq!(undone.canonical_amount(), "1000");
        let reversal = Activity::reversal(&params(), &original).expect("reversal activity");
        assert_eq!(reversal.kind(), ActivityKind::Reversal);
        assert_eq!(reversal.reverses(), Some(original.id()));
    }

    #[test]
    fn equal_time_ordering_is_deterministic() {
        let account = AccountId::new();
        let mut first =
            Activity::deposit(&params(), balance_endpoint(account), cny("1")).expect("a");
        let mut second =
            Activity::deposit(&params(), balance_endpoint(account), cny("2")).expect("b");
        let effective = Timestamp::parse("2026-06-01T12:00:00.000Z").expect("effective");
        let earlier_created = Timestamp::parse("2026-06-01T12:00:01.000Z").expect("c1");
        let later_created = Timestamp::parse("2026-06-01T12:00:02.000Z").expect("c2");
        let low = ActivityId::from_uuid(
            Uuid::parse_str("01900000-0000-7000-8000-000000000001").expect("low"),
        );
        let high = ActivityId::from_uuid(
            Uuid::parse_str("01900000-0000-7000-8000-000000000002").expect("high"),
        );
        first.set_ordering_keys(low, effective.clone(), later_created.clone());
        second.set_ordering_keys(high, effective.clone(), earlier_created.clone());
        assert_eq!(first.cmp_desc(&second), Ordering::Less);
        first.set_ordering_keys(low, effective.clone(), earlier_created.clone());
        second.set_ordering_keys(high, effective.clone(), earlier_created.clone());
        assert_eq!(second.cmp_desc(&first), Ordering::Less);
        let later_effective = Timestamp::parse("2026-06-02T12:00:00.000Z").expect("later");
        first.set_ordering_keys(low, later_effective, earlier_created);
        second.set_ordering_keys(high, effective, later_created);
        assert_eq!(first.cmp_desc(&second), Ordering::Less);
    }

    impl Activity {
        fn set_ordering_keys(
            &mut self,
            id: ActivityId,
            effective_at: Timestamp,
            created_at: Timestamp,
        ) {
            self.id = id;
            self.effective_at = effective_at;
            self.created_at = created_at;
        }
    }
}
