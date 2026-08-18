use rust_decimal::Decimal;

use super::{
    currency::CurrencyCode,
    decimal::{
        parse_canonical_decimal, round_to_money_scale, DecimalSyntax, MONEY_MAX_FRACTIONAL_DIGITS,
        MONEY_MAX_INTEGER_DIGITS,
    },
};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Money {
    amount: Decimal,
    currency: CurrencyCode,
}

impl Money {
    pub fn parse(amount: &str, currency: CurrencyCode) -> Result<Self, AppError> {
        let amount = parse_canonical_decimal(
            amount,
            DecimalSyntax {
                max_integer_digits: MONEY_MAX_INTEGER_DIGITS,
                max_fractional_digits: MONEY_MAX_FRACTIONAL_DIGITS,
                allow_zero: true,
            },
        )
        .map_err(|_| invalid_amount())?;
        Ok(Self { amount, currency })
    }

    pub fn from_canonical(amount: Decimal, currency: CurrencyCode) -> Result<Self, AppError> {
        Ok(Self {
            amount: round_to_money_scale(amount)?.normalize(),
            currency,
        })
    }

    pub(crate) fn from_unrounded(amount: Decimal, currency: CurrencyCode) -> Self {
        Self {
            amount: amount.normalize(),
            currency,
        }
    }

    #[must_use]
    pub fn amount(self) -> Decimal {
        self.amount
    }

    #[must_use]
    pub fn currency(self) -> CurrencyCode {
        self.currency
    }

    #[must_use]
    pub fn canonical_amount(self) -> String {
        self.amount.normalize().to_string()
    }

    #[must_use]
    pub fn is_zero(self) -> bool {
        self.amount.is_zero()
    }
}

fn invalid_amount() -> AppError {
    AppError::invalid_money(
        "Amount must be a non-negative decimal with at most 12 integer digits and 4 fractional digits.",
    )
}

#[cfg(test)]
mod tests {
    use super::Money;
    use crate::domain::currency::CurrencyCode;
    use crate::error::AppError;

    fn cny(amount: &str) -> Money {
        Money::parse(amount, CurrencyCode::CNY).expect("valid CNY amount")
    }

    #[test]
    fn parses_and_normalizes_amounts() {
        assert_eq!(cny("0").canonical_amount(), "0");
        assert_eq!(cny("1.23").canonical_amount(), "1.23");
        assert_eq!(cny("1.2300").canonical_amount(), "1.23");
        assert_eq!(cny("0.0000").canonical_amount(), "0");
        assert_eq!(cny("0.10").canonical_amount(), "0.1");
        assert_eq!(
            cny("999999999999.9999").canonical_amount(),
            "999999999999.9999"
        );
        assert!(cny("0").is_zero());
    }

    #[test]
    fn rejects_illegal_amount_syntax() {
        for amount in [
            "",
            " 1",
            "1 ",
            "+1",
            "-1",
            "1e3",
            "1E3",
            "1,000",
            "0001",
            "01",
            "1.",
            ".5",
            "1.23456",
            "1000000000000",
            "NaN",
            "Infinity",
            "0.00000",
        ] {
            let error = Money::parse(amount, CurrencyCode::CNY).expect_err(amount);
            assert!(
                matches!(error, AppError::InvalidMoney { .. }),
                "{amount}: {error:?}"
            );
        }
    }

    #[test]
    fn keeps_currency_with_the_amount() {
        let money = Money::parse("14490.00", CurrencyCode::SGD).expect("valid money");
        assert_eq!(money.currency(), CurrencyCode::SGD);
        assert_eq!(money.canonical_amount(), "14490");
    }
}
