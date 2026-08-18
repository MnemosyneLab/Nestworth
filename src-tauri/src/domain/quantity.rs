use rust_decimal::Decimal;

use super::decimal::{
    canonical_decimal, parse_canonical_decimal, DecimalSyntax, QUANTITY_MAX_FRACTIONAL_DIGITS,
    QUANTITY_MAX_INTEGER_DIGITS,
};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quantity(Decimal);

impl Quantity {
    pub fn parse(input: &str) -> Result<Self, AppError> {
        parse_canonical_decimal(
            input,
            DecimalSyntax {
                max_integer_digits: QUANTITY_MAX_INTEGER_DIGITS,
                max_fractional_digits: QUANTITY_MAX_FRACTIONAL_DIGITS,
                allow_zero: true,
            },
        )
        .map(Self)
        .map_err(|_| invalid_quantity())
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

fn invalid_quantity() -> AppError {
    AppError::invalid_quantity(
        "Quantity must be a non-negative decimal with at most 18 integer digits and 8 fractional digits.",
    )
}

#[cfg(test)]
mod tests {
    use super::Quantity;
    use crate::error::AppError;

    #[test]
    fn parses_zero_and_normalizes() {
        assert_eq!(Quantity::parse("0").expect("zero").canonical(), "0");
        assert_eq!(Quantity::parse("3").expect("whole").canonical(), "3");
        assert_eq!(
            Quantity::parse("1000.00000000").expect("scale").canonical(),
            "1000"
        );
        assert_eq!(
            Quantity::parse("0.00000001").expect("tiny").canonical(),
            "0.00000001"
        );
        assert!(Quantity::parse("0").expect("zero").is_zero());
    }

    #[test]
    fn rejects_illegal_quantity_syntax() {
        for amount in [
            "",
            "-1",
            "+1",
            "1e3",
            "01",
            "1.",
            ".5",
            "1.000000000",
            "1000000000000000000",
            "NaN",
        ] {
            let error = Quantity::parse(amount).expect_err(amount);
            assert!(
                matches!(error, AppError::InvalidQuantity { .. }),
                "{amount}: {error:?}"
            );
        }
    }
}
