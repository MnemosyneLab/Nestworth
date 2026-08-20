use rust_decimal::Decimal;

use super::{
    currency::CurrencyCode,
    decimal::{
        canonical_decimal, checked_add, checked_sub, parse_signed_canonical_decimal,
        round_to_money_scale, DecimalSyntax, MONEY_MAX_FRACTIONAL_DIGITS, MONEY_MAX_INTEGER_DIGITS,
    },
};
use crate::error::AppError;

/// Signed currency amount used only for analytics outputs such as gain.
///
/// Never converted into [`super::money::Money`]. Zero is allowed. The canonical
/// string may carry a leading `-`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedMoney {
    amount: Decimal,
    currency: CurrencyCode,
}

impl SignedMoney {
    pub fn parse(amount: &str, currency: CurrencyCode) -> Result<Self, AppError> {
        let amount = parse_signed_canonical_decimal(
            amount,
            DecimalSyntax {
                max_integer_digits: MONEY_MAX_INTEGER_DIGITS,
                max_fractional_digits: MONEY_MAX_FRACTIONAL_DIGITS,
                allow_zero: true,
            },
        )
        .map_err(|_| invalid_signed_amount())?;
        Ok(Self { amount, currency })
    }

    pub fn from_canonical(amount: Decimal, currency: CurrencyCode) -> Result<Self, AppError> {
        Ok(Self {
            amount: round_to_money_scale(amount)?.normalize(),
            currency,
        })
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

    pub fn checked_add(self, other: Self) -> Result<Self, AppError> {
        if self.currency != other.currency {
            return Err(AppError::validation(
                "currency",
                "Signed amounts must use the same currency.",
            ));
        }
        let sum = checked_add(self.amount, other.amount)?;
        Ok(Self {
            amount: sum.normalize(),
            currency: self.currency,
        })
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, AppError> {
        if self.currency != other.currency {
            return Err(AppError::validation(
                "currency",
                "Signed amounts must use the same currency.",
            ));
        }
        let difference = checked_sub(self.amount, other.amount)?;
        Ok(Self {
            amount: difference.normalize(),
            currency: self.currency,
        })
    }
}

fn invalid_signed_amount() -> AppError {
    AppError::validation(
        "signedAmount",
        "Signed amount must be a decimal with an optional leading minus, at most 12 integer digits, and 4 fractional digits.",
    )
}

#[cfg(test)]
mod tests {
    use super::SignedMoney;
    use crate::domain::currency::CurrencyCode;
    use crate::domain::money::Money;
    use crate::error::AppError;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn usd(amount: &str) -> SignedMoney {
        SignedMoney::parse(amount, CurrencyCode::USD).expect("valid USD signed amount")
    }

    #[test]
    fn parses_and_normalizes_signed_amounts() {
        assert_eq!(usd("0").canonical_amount(), "0");
        assert_eq!(usd("-0.0000").canonical_amount(), "0");
        assert_eq!(usd("1.2300").canonical_amount(), "1.23");
        assert_eq!(usd("-1.2300").canonical_amount(), "-1.23");
        assert_eq!(usd("-160").canonical_amount(), "-160");
        assert!(usd("0").is_zero());
        assert!(usd("-1").is_negative());
        assert!(!usd("1").is_negative());
        assert_eq!(
            usd("999999999999.9999").canonical_amount(),
            "999999999999.9999"
        );
        assert_eq!(
            usd("-999999999999.9999").canonical_amount(),
            "-999999999999.9999"
        );
    }

    #[test]
    fn rounds_half_to_even_at_dto_boundary() {
        let up = SignedMoney::from_canonical(
            Decimal::from_str("1.23455").expect("literal"),
            CurrencyCode::USD,
        )
        .expect("round");
        assert_eq!(up.canonical_amount(), "1.2346");
        let even = SignedMoney::from_canonical(
            Decimal::from_str("-1.23445").expect("literal"),
            CurrencyCode::USD,
        )
        .expect("even");
        assert_eq!(even.canonical_amount(), "-1.2344");
    }

    #[test]
    fn rejects_malformed_signed_amounts() {
        for amount in [
            "",
            "-",
            " 1",
            "1 ",
            "+1",
            "+1.0",
            "1e3",
            "1E3",
            "1,000",
            "0001",
            "01",
            "-01",
            "1.",
            ".5",
            "-.5",
            "1.23456",
            "-1.23456",
            "1000000000000",
            "-1000000000000",
            "NaN",
            "Infinity",
            "0.00000",
        ] {
            let error = SignedMoney::parse(amount, CurrencyCode::USD).expect_err(amount);
            assert!(
                matches!(error, AppError::Validation { .. }),
                "{amount}: {error:?}"
            );
        }
    }

    #[test]
    fn cannot_become_a_money_balance() {
        let signed = usd("-1");
        assert!(Money::parse(&signed.canonical_amount(), CurrencyCode::USD).is_err());
        assert!(Money::parse("-1", CurrencyCode::USD).is_err());
        let _ = signed.amount();
        let _ = signed.currency();
    }

    #[test]
    fn checked_add_and_sub_require_the_same_currency() {
        let left = usd("10");
        let right = usd("-4");
        assert_eq!(
            left.checked_add(right).expect("add").canonical_amount(),
            "6"
        );
        assert_eq!(
            left.checked_sub(right).expect("sub").canonical_amount(),
            "14"
        );
        let cny = SignedMoney::parse("1", CurrencyCode::CNY).expect("cny");
        assert!(matches!(
            left.checked_add(cny),
            Err(AppError::Validation { .. })
        ));
        assert!(matches!(
            left.checked_sub(cny),
            Err(AppError::Validation { .. })
        ));
    }
}
