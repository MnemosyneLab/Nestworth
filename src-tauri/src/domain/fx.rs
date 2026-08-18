use rust_decimal::Decimal;

use super::{
    currency::CurrencyCode,
    decimal::{
        canonical_decimal, checked_div, checked_mul, parse_canonical_decimal, DecimalSyntax,
        FX_RATE_MAX_FRACTIONAL_DIGITS, FX_RATE_MAX_INTEGER_DIGITS,
    },
};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FxRate(Decimal);

impl FxRate {
    pub fn parse(input: &str) -> Result<Self, AppError> {
        let amount = parse_canonical_decimal(
            input,
            DecimalSyntax {
                max_integer_digits: FX_RATE_MAX_INTEGER_DIGITS,
                max_fractional_digits: FX_RATE_MAX_FRACTIONAL_DIGITS,
                allow_zero: false,
            },
        )
        .map_err(|_| invalid_fx_rate())?;
        if !amount.is_sign_positive() {
            return Err(invalid_fx_rate());
        }
        Ok(Self(amount))
    }

    #[must_use]
    pub fn amount(self) -> Decimal {
        self.0
    }

    #[must_use]
    pub fn canonical(self) -> String {
        canonical_decimal(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FxPair {
    pub left: CurrencyCode,
    pub right: CurrencyCode,
}

impl FxPair {
    pub fn new(left: CurrencyCode, right: CurrencyCode) -> Result<Self, AppError> {
        if left == right {
            return Err(AppError::validation(
                "currency",
                "FX pairs must use two different currencies.",
            ));
        }
        if left.as_str() < right.as_str() {
            Ok(Self { left, right })
        } else {
            Ok(Self {
                left: right,
                right: left,
            })
        }
    }

    #[must_use]
    pub fn currency_a(self) -> CurrencyCode {
        self.left
    }

    #[must_use]
    pub fn currency_b(self) -> CurrencyCode {
        self.right
    }
}

pub fn convert_with_direct_rate(native: Decimal, rate: FxRate) -> Result<Decimal, AppError> {
    checked_mul(native, rate.amount())
}

pub fn convert_with_inverse_rate(native: Decimal, rate: FxRate) -> Result<Decimal, AppError> {
    checked_div(native, rate.amount())
}

fn invalid_fx_rate() -> AppError {
    AppError::invalid_fx_rate(
        "FX rate must be a positive decimal with at most 8 integer digits and 12 fractional digits.",
    )
}

#[cfg(test)]
mod tests {
    use super::{convert_with_direct_rate, convert_with_inverse_rate, FxPair, FxRate};
    use crate::domain::currency::CurrencyCode;
    use crate::domain::decimal::canonical_decimal;
    use crate::error::AppError;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[test]
    fn parses_positive_rates_and_rejects_zero() {
        assert_eq!(FxRate::parse("6.9").expect("usd").canonical(), "6.9");
        assert_eq!(FxRate::parse("5.3000").expect("sgd").canonical(), "5.3");
        assert!(matches!(
            FxRate::parse("0").expect_err("zero"),
            AppError::InvalidFxRate { .. }
        ));
        assert!(FxRate::parse("-1").is_err());
        assert!(FxRate::parse("100000000").is_err());
    }

    #[test]
    fn normalizes_unordered_pairs() {
        let pair = FxPair::new(CurrencyCode::USD, CurrencyCode::CNY).expect("pair");
        assert_eq!(pair.currency_a(), CurrencyCode::CNY);
        assert_eq!(pair.currency_b(), CurrencyCode::USD);
        let same = FxPair::new(CurrencyCode::CNY, CurrencyCode::USD).expect("same");
        assert_eq!(pair, same);
        assert!(FxPair::new(CurrencyCode::CNY, CurrencyCode::CNY).is_err());
    }

    #[test]
    fn direct_and_inverse_conversion_match_for_golden_values() {
        let native = Decimal::from_str("2100").expect("qqq native");
        let direct = convert_with_direct_rate(native, FxRate::parse("6.9").expect("usd cny"))
            .expect("direct");
        let inverse = convert_with_inverse_rate(
            native,
            FxRate::parse("0.144927536232").expect("cny usd inverse of 6.9"),
        )
        .expect("inverse");
        let direct = crate::domain::decimal::round_to_money_scale(direct).expect("direct scale");
        let inverse = crate::domain::decimal::round_to_money_scale(inverse).expect("inverse scale");
        assert_eq!(canonical_decimal(direct), "14490");
        assert_eq!(canonical_decimal(inverse), "14490");
    }
}
