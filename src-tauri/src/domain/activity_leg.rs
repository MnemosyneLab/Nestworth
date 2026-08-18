use super::{
    decimal::{checked_add, checked_sub},
    fx::FxRate,
    ids::{AccountId, ActivityId, ActivityLegId, HoldingId, InstrumentId},
    money::Money,
    quantity::Quantity,
};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LegRole {
    Source,
    Destination,
    Holding,
    Settlement,
    Fee,
    Income,
    Liability,
    Adjustment,
}

impl LegRole {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "source" => Ok(Self::Source),
            "destination" => Ok(Self::Destination),
            "holding" => Ok(Self::Holding),
            "settlement" => Ok(Self::Settlement),
            "fee" => Ok(Self::Fee),
            "income" => Ok(Self::Income),
            "liability" => Ok(Self::Liability),
            "adjustment" => Ok(Self::Adjustment),
            _ => Err(AppError::validation(
                "legRole",
                "Activity leg role is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Destination => "destination",
            Self::Holding => "holding",
            Self::Settlement => "settlement",
            Self::Fee => "fee",
            Self::Income => "income",
            Self::Liability => "liability",
            Self::Adjustment => "adjustment",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentKind {
    AccountValue,
    HoldingsCash,
    HoldingQuantity,
}

impl ComponentKind {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "account_value" => Ok(Self::AccountValue),
            "holdings_cash" => Ok(Self::HoldingsCash),
            "holding_quantity" => Ok(Self::HoldingQuantity),
            _ => Err(AppError::validation(
                "componentKind",
                "Activity component kind is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AccountValue => "account_value",
            Self::HoldingsCash => "holdings_cash",
            Self::HoldingQuantity => "holding_quantity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Increase,
    Decrease,
}

impl Direction {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "increase" => Ok(Self::Increase),
            "decrease" => Ok(Self::Decrease),
            _ => Err(AppError::validation(
                "direction",
                "Activity direction is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Increase => "increase",
            Self::Decrease => "decrease",
        }
    }

    #[must_use]
    pub fn inverse(self) -> Self {
        match self {
            Self::Increase => Self::Decrease,
            Self::Decrease => Self::Increase,
        }
    }
}

/// Exclusive persisted shape of one Activity leg. Magnitudes are unsigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegComponent {
    AccountValue {
        amount: Money,
    },
    HoldingsCash {
        amount: Money,
    },
    HoldingQuantity {
        instrument_id: InstrumentId,
        holding_id: HoldingId,
        quantity: Quantity,
    },
}

impl LegComponent {
    #[must_use]
    pub fn kind(&self) -> ComponentKind {
        match self {
            Self::AccountValue { .. } => ComponentKind::AccountValue,
            Self::HoldingsCash { .. } => ComponentKind::HoldingsCash,
            Self::HoldingQuantity { .. } => ComponentKind::HoldingQuantity,
        }
    }

    pub fn money(&self) -> Result<Money, AppError> {
        match self {
            Self::AccountValue { amount } | Self::HoldingsCash { amount } => Ok(*amount),
            Self::HoldingQuantity { .. } => Err(AppError::invalid_activity_legs(
                "This leg does not carry a monetary amount.",
            )),
        }
    }

    pub fn quantity(&self) -> Result<Quantity, AppError> {
        match self {
            Self::HoldingQuantity { quantity, .. } => Ok(*quantity),
            Self::AccountValue { .. } | Self::HoldingsCash { .. } => Err(
                AppError::invalid_activity_legs("This leg does not carry a holding quantity."),
            ),
        }
    }

    fn require_positive(&self) -> Result<(), AppError> {
        match self {
            Self::AccountValue { amount } | Self::HoldingsCash { amount } => {
                if amount.is_zero() {
                    return Err(AppError::invalid_activity_legs(
                        "Persisted monetary legs must have a positive amount.",
                    ));
                }
                Ok(())
            }
            Self::HoldingQuantity { quantity, .. } => {
                if quantity.is_zero() {
                    return Err(AppError::invalid_activity_legs(
                        "Persisted quantity legs must have a positive quantity.",
                    ));
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonetaryComponent {
    AccountValue,
    HoldingsCash,
}

impl MonetaryComponent {
    #[must_use]
    pub fn kind(self) -> ComponentKind {
        match self {
            Self::AccountValue => ComponentKind::AccountValue,
            Self::HoldingsCash => ComponentKind::HoldingsCash,
        }
    }

    #[must_use]
    pub fn into_leg_component(self, amount: Money) -> LegComponent {
        match self {
            Self::AccountValue => LegComponent::AccountValue { amount },
            Self::HoldingsCash => LegComponent::HoldingsCash { amount },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonetaryEndpoint {
    pub account_id: AccountId,
    pub component: MonetaryComponent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuantityEndpoint {
    pub account_id: AccountId,
    pub holding_id: HoldingId,
    pub instrument_id: InstrumentId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityLeg {
    id: ActivityLegId,
    activity_id: ActivityId,
    account_id: AccountId,
    role: LegRole,
    direction: Direction,
    component: LegComponent,
    fx_rate: Option<FxRate>,
    sort_order: i64,
}

impl ActivityLeg {
    #[allow(clippy::too_many_arguments)]
    pub fn from_persisted(
        id: ActivityLegId,
        activity_id: ActivityId,
        account_id: AccountId,
        role: LegRole,
        direction: Direction,
        component: LegComponent,
        fx_rate: Option<FxRate>,
        sort_order: i64,
    ) -> Result<Self, AppError> {
        component.require_positive()?;
        if fx_rate.is_some() && matches!(component, LegComponent::HoldingQuantity { .. }) {
            return Err(AppError::invalid_activity_legs(
                "FX rates apply only to monetary transfer legs.",
            ));
        }
        Ok(Self {
            id,
            activity_id,
            account_id,
            role,
            direction,
            component,
            fx_rate,
            sort_order,
        })
    }

    pub(crate) fn new(
        activity_id: ActivityId,
        account_id: AccountId,
        role: LegRole,
        direction: Direction,
        component: LegComponent,
        fx_rate: Option<FxRate>,
        sort_order: i64,
    ) -> Result<Self, AppError> {
        component.require_positive()?;
        if fx_rate.is_some() && matches!(component, LegComponent::HoldingQuantity { .. }) {
            return Err(AppError::invalid_activity_legs(
                "FX rates apply only to monetary transfer legs.",
            ));
        }
        Ok(Self {
            id: ActivityLegId::new(),
            activity_id,
            account_id,
            role,
            direction,
            component,
            fx_rate,
            sort_order,
        })
    }

    #[must_use]
    pub fn id(&self) -> ActivityLegId {
        self.id
    }

    #[must_use]
    pub fn activity_id(&self) -> ActivityId {
        self.activity_id
    }

    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    #[must_use]
    pub fn role(&self) -> LegRole {
        self.role
    }

    #[must_use]
    pub fn direction(&self) -> Direction {
        self.direction
    }

    #[must_use]
    pub fn component_kind(&self) -> ComponentKind {
        self.component.kind()
    }

    #[must_use]
    pub fn component(&self) -> &LegComponent {
        &self.component
    }

    #[must_use]
    pub fn fx_rate(&self) -> Option<FxRate> {
        self.fx_rate
    }

    #[must_use]
    pub fn sort_order(&self) -> i64 {
        self.sort_order
    }

    pub(crate) fn with_activity_id(mut self, activity_id: ActivityId) -> Self {
        self.activity_id = activity_id;
        self
    }

    #[must_use]
    pub fn inverse(&self) -> Self {
        Self {
            id: ActivityLegId::new(),
            activity_id: self.activity_id,
            account_id: self.account_id,
            role: self.role,
            direction: self.direction.inverse(),
            component: self.component.clone(),
            fx_rate: self.fx_rate,
            sort_order: self.sort_order,
        }
    }

    pub fn apply_to_money(&self, current: Money) -> Result<Money, AppError> {
        let magnitude = self.component.money()?;
        if current.currency() != magnitude.currency() {
            return Err(AppError::invalid_activity_legs(
                "The currency does not match this monetary endpoint.",
            ));
        }
        match self.direction {
            Direction::Increase => {
                let sum = checked_add(current.amount(), magnitude.amount())?;
                Money::from_canonical(sum, current.currency())
            }
            Direction::Decrease => {
                let difference = checked_sub(current.amount(), magnitude.amount())?;
                if difference.is_sign_negative() {
                    return Err(AppError::InsufficientBalance);
                }
                Money::from_canonical(difference, current.currency())
            }
        }
    }

    pub fn apply_to_quantity(&self, current: Quantity) -> Result<Quantity, AppError> {
        let magnitude = self.component.quantity()?;
        match self.direction {
            Direction::Increase => {
                let sum = checked_add(current.amount(), magnitude.amount())?;
                Quantity::from_canonical(sum)
            }
            Direction::Decrease => {
                let difference = checked_sub(current.amount(), magnitude.amount())?;
                if difference.is_sign_negative() {
                    return Err(AppError::InsufficientQuantity);
                }
                Quantity::from_canonical(difference)
            }
        }
    }
}

pub fn apply_money_legs(current: Money, legs: &[ActivityLeg]) -> Result<Money, AppError> {
    let mut state = current;
    for leg in legs {
        state = leg.apply_to_money(state)?;
    }
    Ok(state)
}

pub fn apply_quantity_legs(current: Quantity, legs: &[ActivityLeg]) -> Result<Quantity, AppError> {
    let mut state = current;
    for leg in legs {
        state = leg.apply_to_quantity(state)?;
    }
    Ok(state)
}
