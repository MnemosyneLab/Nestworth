use rust_decimal::Decimal;

use super::decimal::{
    canonical_decimal, parse_signed_canonical_decimal, round_to_return_rate_scale, DecimalSyntax,
    RETURN_RATE_MAX_FRACTIONAL_DIGITS, RETURN_RATE_MAX_INTEGER_DIGITS,
};
use crate::error::AppError;

/// Signed return expressed as a **fraction**, not a percentage.
///
/// `0.0404` means 4.04%. This is not a [`super::money::Money`],
/// [`super::quantity::Quantity`], [`super::unit_price::UnitPrice`], or
/// [`super::fx::FxRate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReturnRate {
    amount: Decimal,
}

impl ReturnRate {
    pub fn parse(input: &str) -> Result<Self, AppError> {
        let amount = parse_signed_canonical_decimal(
            input,
            DecimalSyntax {
                max_integer_digits: RETURN_RATE_MAX_INTEGER_DIGITS,
                max_fractional_digits: RETURN_RATE_MAX_FRACTIONAL_DIGITS,
                allow_zero: true,
            },
        )
        .map_err(|_| invalid_return_rate())?;
        Ok(Self { amount })
    }

    pub fn from_canonical(amount: Decimal) -> Result<Self, AppError> {
        Ok(Self {
            amount: round_to_return_rate_scale(amount)?.normalize(),
        })
    }

    #[must_use]
    pub fn amount(self) -> Decimal {
        self.amount
    }

    #[must_use]
    pub fn canonical(self) -> String {
        canonical_decimal(self.amount)
    }

    #[must_use]
    pub fn is_zero(self) -> bool {
        self.amount.is_zero()
    }

    #[must_use]
    pub fn is_negative(self) -> bool {
        self.amount.is_sign_negative() && !self.amount.is_zero()
    }
}

fn invalid_return_rate() -> AppError {
    AppError::validation(
        "returnRate",
        "Return rate must be a signed fraction with at most 8 integer digits and 6 fractional digits.",
    )
}

#[cfg(test)]
mod tests {
    use super::ReturnRate;
    use crate::domain::fx::FxRate;
    use crate::domain::money::Money;
    use crate::domain::quantity::Quantity;
    use crate::domain::unit_price::UnitPrice;
    use crate::domain::CurrencyCode;
    use crate::error::AppError;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[test]
    fn parses_fraction_not_percentage() {
        let twr = ReturnRate::parse("0.0404").expect("twr");
        assert_eq!(twr.canonical(), "0.0404");
        assert_eq!(
            ReturnRate::parse("0.040400").expect("scale").canonical(),
            "0.0404"
        );
        assert_eq!(ReturnRate::parse("-0.1").expect("loss").canonical(), "-0.1");
        assert_eq!(ReturnRate::parse("0").expect("zero").canonical(), "0");
        assert!(ReturnRate::parse("0").expect("zero").is_zero());
        assert!(ReturnRate::parse("-0.000001").expect("tiny").is_negative());
        let xirr = ReturnRate::parse("0.096872").expect("xirr");
        assert_eq!(xirr.canonical(), "0.096872");
    }

    #[test]
    fn rounds_six_fractional_digits_half_to_even() {
        let rounded = ReturnRate::from_canonical(Decimal::from_str("0.0404005").expect("literal"))
            .expect("round");
        assert_eq!(rounded.canonical(), "0.0404");
        let even = ReturnRate::from_canonical(Decimal::from_str("0.0404015").expect("literal"))
            .expect("even");
        assert_eq!(even.canonical(), "0.040402");
    }

    #[test]
    fn rejects_malformed_return_rates() {
        for amount in [
            "",
            "-",
            "+0.1",
            "1e-2",
            "01",
            "-01",
            "1.",
            ".5",
            "0.0404000",
            "100000000",
            "NaN",
        ] {
            let error = ReturnRate::parse(amount).expect_err(amount);
            assert!(
                matches!(error, AppError::Validation { .. }),
                "{amount}: {error:?}"
            );
        }
    }

    #[test]
    fn is_not_a_money_quantity_unit_price_or_fx_rate() {
        let rate = ReturnRate::parse("0.0404").expect("rate");
        assert!(Money::parse(&rate.canonical(), CurrencyCode::USD).is_ok());
        assert!(Quantity::parse(&rate.canonical()).is_ok());
        assert!(UnitPrice::parse(&rate.canonical()).is_ok());
        assert!(FxRate::parse(&rate.canonical()).is_ok());
        assert!(Money::parse("-0.1", CurrencyCode::USD).is_err());
        assert!(Quantity::parse("-0.1").is_err());
        assert!(UnitPrice::parse("-0.1").is_err());
        assert!(FxRate::parse("-0.1").is_err());
        let _ = rate.amount();
    }
}
