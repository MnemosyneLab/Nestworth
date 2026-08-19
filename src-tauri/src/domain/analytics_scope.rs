use super::{
    activity::{classify, ActivityKind, Classification},
    activity_leg::{ComponentKind, Direction, LegRole},
    ids::{AccountId, InstrumentId},
};

/// Analytics aggregation scope. Membership is evaluated per local day by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnalyticsScope {
    Household,
    Portfolio,
    Account(AccountId),
    Instrument(InstrumentId),
}

/// Effective-dated facts needed to decide whether an endpoint is inside a scope.
///
/// The caller supplies these from observations; this module never reads SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeEndpointFacts {
    pub account_id: AccountId,
    pub instrument_id: Option<InstrumentId>,
    pub component_kind: ComponentKind,
    pub included_in_net_worth: bool,
    pub included_in_investment: bool,
    pub is_liability: bool,
    pub is_active: bool,
}

/// One Activity plus precomputed inside/outside flags for each of its legs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeFlowActivity {
    pub kind: ActivityKind,
    pub related_instrument_id: Option<InstrumentId>,
    pub legs: Vec<ScopeFlowLeg>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeFlowLeg {
    pub role: LegRole,
    pub component_kind: ComponentKind,
    pub endpoint_in_scope: bool,
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegFlowClassification {
    NotInScope,
    ZeroFlow,
    SignedFlow { direction: Direction },
    Return,
    UnexplainedReturn,
    UnknownBasisFlow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeFlowResult {
    legs: Vec<LegFlowClassification>,
    basis_complete: bool,
}

impl ScopeFlowResult {
    #[must_use]
    pub fn legs(&self) -> &[LegFlowClassification] {
        &self.legs
    }

    #[must_use]
    pub fn basis_complete(&self) -> bool {
        self.basis_complete
    }

    #[must_use]
    pub fn has_signed_flow(&self) -> bool {
        self.legs
            .iter()
            .any(|leg| matches!(leg, LegFlowClassification::SignedFlow { .. }))
    }

    #[must_use]
    pub fn in_scope_is_zero_flow(&self) -> bool {
        let in_scope: Vec<_> = self
            .legs
            .iter()
            .filter(|leg| !matches!(leg, LegFlowClassification::NotInScope))
            .collect();
        !in_scope.is_empty()
            && in_scope
                .iter()
                .all(|leg| matches!(leg, LegFlowClassification::ZeroFlow))
    }

    #[must_use]
    pub fn has_return(&self) -> bool {
        self.legs
            .iter()
            .any(|leg| matches!(leg, LegFlowClassification::Return))
    }

    #[must_use]
    pub fn has_unknown_basis_flow(&self) -> bool {
        self.legs
            .iter()
            .any(|leg| matches!(leg, LegFlowClassification::UnknownBasisFlow))
    }

    #[must_use]
    pub fn has_unexplained_return(&self) -> bool {
        self.legs
            .iter()
            .any(|leg| matches!(leg, LegFlowClassification::UnexplainedReturn))
    }
}

/// Whether an endpoint belongs to `scope` on the day these facts apply.
#[must_use]
pub fn endpoint_in_scope(scope: AnalyticsScope, endpoint: &ScopeEndpointFacts) -> bool {
    match scope {
        AnalyticsScope::Household => endpoint.is_active && endpoint.included_in_net_worth,
        AnalyticsScope::Portfolio => {
            endpoint.is_active && !endpoint.is_liability && endpoint.included_in_investment
        }
        AnalyticsScope::Account(account_id) => endpoint.account_id == account_id,
        AnalyticsScope::Instrument(instrument_id) => {
            endpoint.component_kind == ComponentKind::HoldingQuantity
                && endpoint.instrument_id == Some(instrument_id)
        }
    }
}

/// Classify each leg of an Activity against a scope for one local day.
///
/// Income and fee legs inside the scope are return, not flow. Instrument-scope
/// income and fees attributed with `related_instrument_id` are return even when
/// the cash endpoint is outside the instrument. Quantity remeasurement inside
/// the scope is unknown-basis flow and sets `basisComplete = false`. Monetary
/// remeasurement is unexplained return, not flow.
#[must_use]
pub fn classify_scope_flow(scope: AnalyticsScope, activity: &ScopeFlowActivity) -> ScopeFlowResult {
    let any_other_in_scope = |index: usize| {
        activity
            .legs
            .iter()
            .enumerate()
            .any(|(other, leg)| other != index && leg.endpoint_in_scope)
    };

    let mut basis_complete = true;
    let mut legs = Vec::with_capacity(activity.legs.len());
    for (index, leg) in activity.legs.iter().enumerate() {
        let classification = classify(activity.kind, leg.role);
        let attributed_instrument_return = matches!(scope, AnalyticsScope::Instrument(instrument_id) if activity.related_instrument_id == Some(instrument_id))
            && matches!(classification, Classification::Income | Classification::Fee);

        let classified = if attributed_instrument_return {
            LegFlowClassification::Return
        } else if matches!(classification, Classification::Income | Classification::Fee) {
            if leg.endpoint_in_scope {
                LegFlowClassification::Return
            } else {
                LegFlowClassification::NotInScope
            }
        } else if classification == Classification::Remeasurement && leg.endpoint_in_scope {
            if leg.component_kind == ComponentKind::HoldingQuantity {
                basis_complete = false;
                LegFlowClassification::UnknownBasisFlow
            } else {
                LegFlowClassification::UnexplainedReturn
            }
        } else if !leg.endpoint_in_scope {
            LegFlowClassification::NotInScope
        } else if any_other_in_scope(index) {
            LegFlowClassification::ZeroFlow
        } else {
            LegFlowClassification::SignedFlow {
                direction: leg.direction,
            }
        };
        legs.push(classified);
    }

    ScopeFlowResult {
        legs,
        basis_complete,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_scope_flow, endpoint_in_scope, AnalyticsScope, LegFlowClassification,
        ScopeEndpointFacts, ScopeFlowActivity, ScopeFlowLeg,
    };
    use crate::domain::activity::ActivityKind;
    use crate::domain::activity_leg::{ComponentKind, Direction, LegRole};
    use crate::domain::ids::{AccountId, InstrumentId};

    fn account(n: u128) -> AccountId {
        AccountId::from_uuid(uuid::Uuid::from_u128(n))
    }

    fn instrument(n: u128) -> InstrumentId {
        InstrumentId::from_uuid(uuid::Uuid::from_u128(n))
    }

    fn holding_endpoint(account_id: AccountId, instrument_id: InstrumentId) -> ScopeEndpointFacts {
        ScopeEndpointFacts {
            account_id,
            instrument_id: Some(instrument_id),
            included_in_net_worth: true,
            included_in_investment: true,
            is_liability: false,
            is_active: true,
            component_kind: ComponentKind::HoldingQuantity,
        }
    }

    #[test]
    fn internal_transfer_contributes_zero_flow() {
        let activity = ScopeFlowActivity {
            kind: ActivityKind::Transfer,
            related_instrument_id: None,
            legs: vec![
                ScopeFlowLeg {
                    role: LegRole::Source,
                    component_kind: ComponentKind::HoldingsCash,
                    endpoint_in_scope: true,
                    direction: Direction::Decrease,
                },
                ScopeFlowLeg {
                    role: LegRole::Destination,
                    component_kind: ComponentKind::HoldingsCash,
                    endpoint_in_scope: true,
                    direction: Direction::Increase,
                },
            ],
        };
        let result = classify_scope_flow(AnalyticsScope::Household, &activity);
        assert!(result.in_scope_is_zero_flow());
        assert!(!result.has_signed_flow());
        assert!(result.basis_complete());
    }

    #[test]
    fn boundary_crossing_is_signed_flow() {
        let activity = ScopeFlowActivity {
            kind: ActivityKind::Deposit,
            related_instrument_id: None,
            legs: vec![
                ScopeFlowLeg {
                    role: LegRole::Destination,
                    component_kind: ComponentKind::HoldingsCash,
                    endpoint_in_scope: true,
                    direction: Direction::Increase,
                },
                ScopeFlowLeg {
                    role: LegRole::Source,
                    component_kind: ComponentKind::AccountValue,
                    endpoint_in_scope: false,
                    direction: Direction::Decrease,
                },
            ],
        };
        let result = classify_scope_flow(AnalyticsScope::Account(account(1)), &activity);
        assert!(result.has_signed_flow());
        assert!(!result.in_scope_is_zero_flow());
        assert_eq!(
            result.legs()[0],
            LegFlowClassification::SignedFlow {
                direction: Direction::Increase
            }
        );
        assert_eq!(result.legs()[1], LegFlowClassification::NotInScope);
    }

    #[test]
    fn instrument_scope_attributed_income_and_fees_are_return_not_flow() {
        let instrument_id = instrument(9);
        let income = ScopeFlowActivity {
            kind: ActivityKind::Income,
            related_instrument_id: Some(instrument_id),
            legs: vec![ScopeFlowLeg {
                role: LegRole::Income,
                component_kind: ComponentKind::HoldingsCash,
                endpoint_in_scope: false,
                direction: Direction::Increase,
            }],
        };
        let income_result = classify_scope_flow(AnalyticsScope::Instrument(instrument_id), &income);
        assert!(income_result.has_return());
        assert!(!income_result.has_signed_flow());
        assert_eq!(income_result.legs()[0], LegFlowClassification::Return);

        let fee = ScopeFlowActivity {
            kind: ActivityKind::Fee,
            related_instrument_id: Some(instrument_id),
            legs: vec![ScopeFlowLeg {
                role: LegRole::Fee,
                component_kind: ComponentKind::HoldingsCash,
                endpoint_in_scope: false,
                direction: Direction::Decrease,
            }],
        };
        let fee_result = classify_scope_flow(AnalyticsScope::Instrument(instrument_id), &fee);
        assert!(fee_result.has_return());
        assert!(!fee_result.has_signed_flow());
        assert_eq!(fee_result.legs()[0], LegFlowClassification::Return);
    }

    #[test]
    fn quantity_remeasurement_inside_scope_sets_basis_complete_false() {
        let activity = ScopeFlowActivity {
            kind: ActivityKind::PositionAdjustment,
            related_instrument_id: None,
            legs: vec![ScopeFlowLeg {
                role: LegRole::Adjustment,
                component_kind: ComponentKind::HoldingQuantity,
                endpoint_in_scope: true,
                direction: Direction::Increase,
            }],
        };
        let result = classify_scope_flow(AnalyticsScope::Household, &activity);
        assert!(!result.basis_complete());
        assert!(result.has_unknown_basis_flow());
        assert!(!result.has_signed_flow());
        assert_eq!(result.legs()[0], LegFlowClassification::UnknownBasisFlow);
    }

    #[test]
    fn monetary_remeasurement_is_unexplained_return_not_flow() {
        let activity = ScopeFlowActivity {
            kind: ActivityKind::BalanceAdjustment,
            related_instrument_id: None,
            legs: vec![ScopeFlowLeg {
                role: LegRole::Adjustment,
                component_kind: ComponentKind::AccountValue,
                endpoint_in_scope: true,
                direction: Direction::Increase,
            }],
        };
        let result = classify_scope_flow(AnalyticsScope::Household, &activity);
        assert!(result.basis_complete());
        assert!(result.has_unexplained_return());
        assert!(!result.has_signed_flow());
        assert!(!result.has_unknown_basis_flow());
    }

    #[test]
    fn household_and_instrument_membership_use_endpoint_facts() {
        let brokerage = account(1);
        let qqq = instrument(2);
        let holding = holding_endpoint(brokerage, qqq);
        assert!(endpoint_in_scope(AnalyticsScope::Household, &holding));
        assert!(endpoint_in_scope(AnalyticsScope::Portfolio, &holding));
        assert!(endpoint_in_scope(
            AnalyticsScope::Account(brokerage),
            &holding
        ));
        assert!(endpoint_in_scope(AnalyticsScope::Instrument(qqq), &holding));
        assert!(!endpoint_in_scope(
            AnalyticsScope::Instrument(instrument(3)),
            &holding
        ));
        let cash = ScopeEndpointFacts {
            instrument_id: None,
            component_kind: ComponentKind::HoldingsCash,
            ..holding
        };
        assert!(!endpoint_in_scope(AnalyticsScope::Instrument(qqq), &cash));
    }
}
