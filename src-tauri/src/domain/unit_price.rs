use rust_decimal::Decimal;

use super::decimal::{
    canonical_decimal, parse_canonical_decimal, DecimalSyntax, UNIT_PRICE_MAX_FRACTIONAL_DIGITS,
    UNIT_PRICE_MAX_INTEGER_DIGITS,
};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitPrice(Decimal);

impl UnitPrice {
    pub fn parse(input: &str) -> Result<Self, AppError> {
        parse_canonical_decimal(
            input,
            DecimalSyntax {
                max_integer_digits: UNIT_PRICE_MAX_INTEGER_DIGITS,
                max_fractional_digits: UNIT_PRICE_MAX_FRACTIONAL_DIGITS,
                allow_zero: true,
            },
        )
        .map(Self)
        .map_err(|_| invalid_unit_price())
    }

    #[must_use]
    pub fn amount(self) -> Decimal {
        self.0
    }

    #[must_use]
    pub fn canonical(self) -> String {
        canonical_decimal(self.0)
    }

    #[must_use]
    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }
}

fn invalid_unit_price() -> AppError {
    AppError::invalid_unit_price(
        "Unit price must be a non-negative decimal with at most 12 integer digits and 8 fractional digits.",
    )
}

#[cfg(test)]
mod tests {
    use super::UnitPrice;
    use crate::error::AppError;

    #[test]
    fn zero_price_is_distinct_from_missing() {
        let price = UnitPrice::parse("0").expect("zero");
        assert!(price.is_zero());
        assert_eq!(price.canonical(), "0");
        assert_eq!(UnitPrice::parse("700.00").expect("qqq").canonical(), "700");
    }

    #[test]
    fn rejects_illegal_unit_price_syntax() {
        for amount in ["", "-1", "01", "1.000000000", "1000000000000", "1e2"] {
            let error = UnitPrice::parse(amount).expect_err(amount);
            assert!(
                matches!(error, AppError::InvalidUnitPrice { .. }),
                "{amount}: {error:?}"
            );
        }
    }
}
