mod account;
mod account_cash;
mod activity;
mod activity_leg;
mod analytics_scope;
mod category;
mod currency;
mod decimal;
mod fx;
mod group;
mod holding;
mod household;
mod ids;
mod institution;
mod instrument;
mod lot_ledger;
mod member;
mod money;
mod ownership;
mod quantity;
mod quote;
mod reference_catalog;
mod return_rate;
mod signed_money;
mod text;
mod time;
mod unit_price;
mod valuation;

pub use account::{
    Account, AccountValue, NewAccount, PersistedAccount, PersistedAccountValue, ValueKind,
};
pub use account_cash::AccountCashValue;
pub use activity::{
    classify, inverse_legs, Activity, ActivityKind, ActivityRecordParams, Classification,
    ComponentOpening, ConstructActivity, DebtCashLink, DebtDrawSpec, DebtPaymentSpec, FeeKind,
    IncomeKind, TradeSpec,
};
pub use activity_leg::{
    apply_money_legs, apply_quantity_legs, ActivityLeg, ComponentKind, Direction, LegComponent,
    LegRole, MonetaryComponent, MonetaryEndpoint, QuantityEndpoint,
};
pub use analytics_scope::{
    classify_scope_flow, endpoint_in_scope, AnalyticsScope, LegFlowClassification,
    ScopeEndpointFacts, ScopeFlowActivity, ScopeFlowLeg, ScopeFlowResult,
};
pub use category::{PrimaryCategory, SecondaryCategory, TrackingMode};
pub use currency::CurrencyCode;
pub use decimal::{
    canonical_decimal, checked_add, checked_exp, checked_ln, checked_powd, checked_sub,
    round_to_money_scale, round_to_return_rate_scale,
};
pub use fx::{convert_with_direct_rate, convert_with_inverse_rate, FxPair, FxRate};
pub use group::{AccountGroup, NewAccountGroup, PersistedAccountGroup};
pub use holding::{Holding, PersistedHolding};
pub use household::Household;
pub use ids::{
    AccountCashValueId, AccountGroupId, AccountId, AccountStateObservationId, AccountValueId,
    ActivityId, ActivityLegId, CostBasisDeclarationId, FxQuoteId, HistoryOriginId,
    HistoryOriginItemId, HoldingId, HoldingQuantityValueId, HoldingStateObservationId, HouseholdId,
    InstitutionId, InstrumentId, InstrumentQuoteId, MediaAssetId, MemberId,
    QuotePreferenceObservationId, ValuationSnapshotId, ValuationSnapshotItemId,
};
pub use institution::{Institution, NewInstitution, PersistedInstitution};
pub use instrument::{Instrument, InstrumentType, NewInstrument, PersistedInstrument};
pub use lot_ledger::{
    replay, ActivityLedgerEvent, BasisStatus, ConsumptionKind, LedgerDiagnostic, LedgerEvent,
    LotConsumption, LotEffect, LotLedger, LotOpening, LotRef, OpenLot, RealizedGainTotals,
};
pub use member::{Member, PersistedMember};
pub use money::Money;
pub use ownership::{percent_to_basis_points, Ownership, OwnershipShare, TOTAL_BPS};
pub use quantity::Quantity;
pub use quote::{Freshness, FxQuote, InstrumentQuote, QuoteSourceKind};
pub use reference_catalog::{
    is_supported_appearance, is_supported_country, is_supported_currency, is_supported_group_color,
    is_supported_group_icon, is_supported_institution_type, is_supported_language, APPEARANCES,
    COUNTRIES, CURRENCIES, GROUP_COLORS, GROUP_ICONS, INSTITUTION_TYPES, LANGUAGES,
};
pub use return_rate::ReturnRate;
pub use signed_money::SignedMoney;
pub use text::{parse_optional_note, NOTE_MAX_CHARS};
pub use time::{
    closed_day_cutoff, inclusive_closed_day_instant, origin_timezone_from_iana_name,
    resolve_activity_time, resolve_host_origin_timezone, resolve_local_datetime,
    validate_activity_time, AmbiguousOffset, CalendarDate, HistoryTimezone, Timestamp,
};
pub use unit_price::UnitPrice;
pub use valuation::{
    convert_native_to_base, holding_native_value, unavailable_holding, ConvertedValue, HoldingValue,
};

#[cfg(test)]
mod tests {
    use super::currency::CurrencyCode;
    use super::{FxRate, Money, Quantity, ReturnRate, SignedMoney, UnitPrice};

    #[test]
    fn signed_and_lot_modules_do_not_use_binary_floats() {
        for source in [
            include_str!("signed_money.rs"),
            include_str!("return_rate.rs"),
            include_str!("lot_ledger.rs"),
            include_str!("analytics_scope.rs"),
        ] {
            assert!(
                !source.contains("f32"),
                "binary f32 is prohibited in analytics domain modules"
            );
            assert!(
                !source.contains("f64"),
                "binary f64 is prohibited in analytics domain modules"
            );
        }
    }

    #[test]
    fn signed_values_never_leak_into_unsigned_types() {
        assert!(Money::parse("-1", CurrencyCode::USD).is_err());
        assert!(Quantity::parse("-1").is_err());
        assert!(UnitPrice::parse("-1").is_err());
        assert!(FxRate::parse("-1").is_err());
        let signed = SignedMoney::parse("-1", CurrencyCode::USD).expect("signed");
        assert!(Money::parse(&signed.canonical_amount(), CurrencyCode::USD).is_err());
        let rate = ReturnRate::parse("-0.0404").expect("rate");
        assert!(Money::parse(&rate.canonical(), CurrencyCode::USD).is_err());
        assert!(Quantity::parse(&rate.canonical()).is_err());
        assert!(UnitPrice::parse(&rate.canonical()).is_err());
        assert!(FxRate::parse(&rate.canonical()).is_err());
    }
}
