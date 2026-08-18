use rust_decimal::Decimal;

use super::{
    currency::CurrencyCode,
    decimal::{canonical_decimal, checked_mul, round_to_money_scale},
    fx::{convert_with_direct_rate, convert_with_inverse_rate, FxRate},
    ids::{FxQuoteId, InstrumentQuoteId},
    money::Money,
    quantity::Quantity,
    quote::{Freshness, InstrumentQuote},
    unit_price::UnitPrice,
};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedValue {
    pub native: Money,
    pub base: Option<Money>,
    pub fx_quote_id: Option<FxQuoteId>,
    pub freshness: Option<Freshness>,
    pub complete: bool,
    pub missing_reason: Option<String>,
}
pub fn holding_native_value(
    quantity: Quantity,
    quote: &InstrumentQuote,
) -> Result<Money, AppError> {
    let native = checked_mul(quantity.amount(), quote.unit_price().amount())?;
    Ok(Money::from_unrounded(native, quote.quote_currency()))
}
pub fn convert_native_to_base(
    native: Money,
    household_base: CurrencyCode,
    direct: Option<(FxQuoteId, FxRate, Freshness)>,
    inverse: Option<(FxQuoteId, FxRate, Freshness)>,
) -> Result<ConvertedValue, AppError> {
    if native.currency() == household_base {
        return Ok(ConvertedValue {
            native,
            base: Some(native),
            fx_quote_id: None,
            freshness: None,
            complete: true,
            missing_reason: None,
        });
    }
    if let Some((id, rate, freshness)) = direct {
        let converted = convert_with_direct_rate(native.amount(), rate)?;
        return Ok(ConvertedValue {
            native,
            base: Some(Money::from_unrounded(converted, household_base)),
            fx_quote_id: Some(id),
            freshness: Some(freshness),
            complete: true,
            missing_reason: None,
        });
    }
    if let Some((id, rate, freshness)) = inverse {
        let converted = convert_with_inverse_rate(native.amount(), rate)?;
        return Ok(ConvertedValue {
            native,
            base: Some(Money::from_unrounded(converted, household_base)),
            fx_quote_id: Some(id),
            freshness: Some(freshness),
            complete: true,
            missing_reason: None,
        });
    }
    Ok(ConvertedValue {
        native,
        base: None,
        fx_quote_id: None,
        freshness: Some(Freshness::Unavailable),
        complete: false,
        missing_reason: Some("fx_quote".to_owned()),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldingValue {
    pub native: Option<Money>,
    pub base: Option<Money>,
    pub instrument_quote_id: Option<InstrumentQuoteId>,
    pub fx_quote_id: Option<FxQuoteId>,
    pub freshness: Freshness,
    pub complete: bool,
    pub missing_reason: Option<String>,
}

pub fn unavailable_holding(reason: &str) -> HoldingValue {
    HoldingValue {
        native: None,
        base: None,
        instrument_quote_id: None,
        fx_quote_id: None,
        freshness: Freshness::Unavailable,
        complete: false,
        missing_reason: Some(reason.to_owned()),
    }
}

#[allow(dead_code)]
pub fn money_from_unit_price(price: UnitPrice, currency: CurrencyCode) -> Result<Money, AppError> {
    let rounded = round_to_money_scale(price.amount())?;
    Money::from_canonical(rounded, currency)
}

#[allow(dead_code)]
pub fn canonical_money_amount(amount: Decimal) -> Result<String, AppError> {
    Ok(canonical_decimal(round_to_money_scale(amount)?))
}

#[cfg(test)]
mod tests {
    use super::{convert_native_to_base, holding_native_value};
    use crate::domain::currency::CurrencyCode;
    use crate::domain::fx::FxRate;
    use crate::domain::ids::{FxQuoteId, InstrumentId};
    use crate::domain::money::Money;
    use crate::domain::quantity::Quantity;
    use crate::domain::quote::{InstrumentQuote, QuoteSourceKind};
    use crate::domain::time::Timestamp;
    use crate::domain::unit_price::UnitPrice;

    #[test]
    fn values_golden_qqq_position() {
        let quote = InstrumentQuote::new(
            InstrumentId::new(),
            UnitPrice::parse("700").expect("price"),
            CurrencyCode::USD,
            QuoteSourceKind::Manual,
            "manual",
            false,
            Timestamp::now(),
            Timestamp::now(),
        )
        .expect("quote");
        let native =
            holding_native_value(Quantity::parse("3").expect("qty"), &quote).expect("native");
        assert_eq!(native.canonical_amount(), "2100");
        let converted = convert_native_to_base(
            native,
            CurrencyCode::CNY,
            Some((
                FxQuoteId::new(),
                FxRate::parse("6.9").expect("rate"),
                crate::domain::quote::Freshness::Manual,
            )),
            None,
        )
        .expect("fx");
        assert_eq!(converted.base.expect("base").canonical_amount(), "14490");
        assert!(converted.complete);
    }

    #[test]
    fn aggregate_rounding_happens_after_component_sum() {
        let quote = InstrumentQuote::new(
            InstrumentId::new(),
            UnitPrice::parse("0.00005").expect("price"),
            CurrencyCode::CNY,
            QuoteSourceKind::Manual,
            "manual",
            false,
            Timestamp::now(),
            Timestamp::now(),
        )
        .expect("quote");
        let first =
            holding_native_value(Quantity::parse("1").expect("quantity"), &quote).expect("first");
        let second =
            holding_native_value(Quantity::parse("1").expect("quantity"), &quote).expect("second");
        assert_eq!(first.canonical_amount(), "0.00005");
        assert_eq!(
            crate::domain::decimal::round_to_money_scale(first.amount()).expect("first round"),
            rust_decimal::Decimal::ZERO
        );
        let aggregate = crate::domain::decimal::checked_add(first.amount(), second.amount())
            .expect("aggregate");
        assert_eq!(
            crate::domain::decimal::canonical_decimal(
                crate::domain::decimal::round_to_money_scale(aggregate).expect("aggregate round")
            ),
            "0.0001"
        );
    }

    #[test]
    fn identity_conversion_needs_no_fx_quote() {
        let native = Money::parse("100000", CurrencyCode::CNY).expect("cash");
        let converted =
            convert_native_to_base(native, CurrencyCode::CNY, None, None).expect("identity");
        assert_eq!(converted.base.expect("base").canonical_amount(), "100000");
        assert!(converted.fx_quote_id.is_none());
        assert!(converted.freshness.is_none());
    }

    #[test]
    fn missing_fx_marks_conversion_incomplete() {
        let native = Money::parse("5000", CurrencyCode::SGD).expect("cash");
        let converted =
            convert_native_to_base(native, CurrencyCode::CNY, None, None).expect("missing");
        assert!(!converted.complete);
        assert_eq!(converted.missing_reason.as_deref(), Some("fx_quote"));
        assert!(converted.base.is_none());
    }
}
