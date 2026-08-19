use std::str::FromStr;

use rust_decimal::{Decimal, MathematicalOps, RoundingStrategy};

use crate::error::AppError;

pub const MONEY_MAX_INTEGER_DIGITS: usize = 12;
pub const MONEY_MAX_FRACTIONAL_DIGITS: usize = 4;
pub const QUANTITY_MAX_INTEGER_DIGITS: usize = 18;
pub const QUANTITY_MAX_FRACTIONAL_DIGITS: usize = 8;
pub const UNIT_PRICE_MAX_INTEGER_DIGITS: usize = 12;
pub const UNIT_PRICE_MAX_FRACTIONAL_DIGITS: usize = 8;
pub const FX_RATE_MAX_INTEGER_DIGITS: usize = 8;
pub const FX_RATE_MAX_FRACTIONAL_DIGITS: usize = 12;
pub const RETURN_RATE_MAX_INTEGER_DIGITS: usize = 8;
pub const RETURN_RATE_MAX_FRACTIONAL_DIGITS: usize = 6;

#[derive(Debug, Clone, Copy)]
pub struct DecimalSyntax {
    pub max_integer_digits: usize,
    pub max_fractional_digits: usize,
    pub allow_zero: bool,
}

pub fn parse_canonical_decimal(input: &str, syntax: DecimalSyntax) -> Result<Decimal, AppError> {
    if input.is_empty()
        || input.contains(|character: char| {
            character.is_whitespace()
                || character == '+'
                || character == '-'
                || character == ','
                || character == 'e'
                || character == 'E'
        })
    {
        return Err(AppError::Internal);
    }

    let integer = match input.split_once('.') {
        Some((integer, fraction)) => {
            if fraction.is_empty()
                || fraction.len() > syntax.max_fractional_digits
                || !fraction.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(AppError::Internal);
            }
            integer
        }
        None => input,
    };

    if !is_valid_integer_part(integer, syntax.max_integer_digits) {
        return Err(AppError::Internal);
    }

    let amount = Decimal::from_str(input)
        .map(|amount| amount.normalize())
        .map_err(|_| AppError::Internal)?;
    if amount.is_zero() && !syntax.allow_zero {
        return Err(AppError::Internal);
    }
    Ok(amount)
}

/// Parse a canonical decimal that may carry a single leading `-`.
///
/// Unsigned types (`Money`, `Quantity`, `UnitPrice`, `FxRate`) must keep calling
/// [`parse_canonical_decimal`], which still rejects `-`. No `+`, exponent, commas,
/// or leading zeros are accepted after the optional sign.
pub fn parse_signed_canonical_decimal(
    input: &str,
    syntax: DecimalSyntax,
) -> Result<Decimal, AppError> {
    let (negative, digits) = match input.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, input),
    };
    if digits.is_empty() {
        return Err(AppError::Internal);
    }
    let amount = parse_canonical_decimal(digits, syntax)?;
    if negative && !amount.is_zero() {
        Ok(-amount)
    } else {
        Ok(amount)
    }
}

pub fn canonical_decimal(amount: Decimal) -> String {
    amount.normalize().to_string()
}

pub fn round_to_money_scale(amount: Decimal) -> Result<Decimal, AppError> {
    let rounded = amount.round_dp_with_strategy(4, RoundingStrategy::MidpointNearestEven);
    let max = Decimal::from_str("999999999999.9999").expect("literal money maximum");
    if rounded.abs() > max {
        return Err(AppError::DecimalOverflow);
    }
    Ok(rounded)
}

pub fn checked_mul(left: Decimal, right: Decimal) -> Result<Decimal, AppError> {
    left.checked_mul(right).ok_or(AppError::DecimalOverflow)
}

pub fn checked_div(left: Decimal, right: Decimal) -> Result<Decimal, AppError> {
    if right.is_zero() {
        return Err(AppError::DecimalOverflow);
    }
    left.checked_div(right).ok_or(AppError::DecimalOverflow)
}

pub fn checked_add(left: Decimal, right: Decimal) -> Result<Decimal, AppError> {
    left.checked_add(right).ok_or(AppError::DecimalOverflow)
}

pub fn checked_sub(left: Decimal, right: Decimal) -> Result<Decimal, AppError> {
    left.checked_sub(right).ok_or(AppError::DecimalOverflow)
}

pub fn round_to_quantity_scale(amount: Decimal) -> Result<Decimal, AppError> {
    let rounded = amount.round_dp_with_strategy(8, RoundingStrategy::MidpointNearestEven);
    let max = Decimal::from_str("999999999999999999.99999999").expect("literal quantity maximum");
    if rounded.abs() > max {
        return Err(AppError::DecimalOverflow);
    }
    Ok(rounded)
}

pub fn round_to_fx_rate_scale(amount: Decimal) -> Result<Decimal, AppError> {
    let rounded = amount.round_dp_with_strategy(12, RoundingStrategy::MidpointNearestEven);
    let max = Decimal::from_str("99999999.999999999999").expect("literal FX rate maximum");
    if rounded.abs() > max {
        return Err(AppError::DecimalOverflow);
    }
    Ok(rounded)
}

pub fn round_to_return_rate_scale(amount: Decimal) -> Result<Decimal, AppError> {
    let rounded = amount.round_dp_with_strategy(6, RoundingStrategy::MidpointNearestEven);
    let max = Decimal::from_str("99999999.999999").expect("literal return rate maximum");
    if rounded.abs() > max {
        return Err(AppError::DecimalOverflow);
    }
    Ok(rounded)
}

pub fn checked_ln(value: Decimal) -> Result<Decimal, AppError> {
    if value.is_sign_negative() || value.is_zero() {
        return Err(AppError::validation(
            "ln",
            "Natural logarithm is undefined for zero or negative values.",
        ));
    }
    value.checked_ln().ok_or(AppError::DecimalOverflow)
}

pub fn checked_exp(value: Decimal) -> Result<Decimal, AppError> {
    value.checked_exp().ok_or(AppError::DecimalOverflow)
}

pub fn checked_powd(base: Decimal, exponent: Decimal) -> Result<Decimal, AppError> {
    base.checked_powd(exponent).ok_or(AppError::DecimalOverflow)
}

fn is_valid_integer_part(integer: &str, max_integer_digits: usize) -> bool {
    if integer == "0" {
        return true;
    }
    if integer.is_empty() || integer.len() > max_integer_digits {
        return false;
    }
    let mut characters = integer.chars();
    match characters.next() {
        Some(first) if ('1'..='9').contains(&first) => {
            characters.all(|character| character.is_ascii_digit())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_decimal, checked_exp, checked_ln, checked_powd, parse_canonical_decimal,
        parse_signed_canonical_decimal, round_to_money_scale, round_to_return_rate_scale,
        DecimalSyntax,
    };
    use crate::error::AppError;
    use rust_decimal::Decimal;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::str::FromStr;

    fn money_syntax() -> DecimalSyntax {
        DecimalSyntax {
            max_integer_digits: 12,
            max_fractional_digits: 4,
            allow_zero: true,
        }
    }

    #[test]
    fn normalizes_and_rounds_half_to_even() {
        let parsed = parse_canonical_decimal("1.2300", money_syntax()).expect("valid");
        assert_eq!(canonical_decimal(parsed), "1.23");
        let rounded =
            round_to_money_scale(Decimal::from_str("1.23455").expect("literal")).expect("round");
        assert_eq!(canonical_decimal(rounded), "1.2346");
        let even =
            round_to_money_scale(Decimal::from_str("1.23445").expect("literal")).expect("round");
        assert_eq!(canonical_decimal(even), "1.2344");
    }

    #[test]
    fn signed_parser_accepts_minus_unsigned_parser_rejects_it() {
        let signed = parse_signed_canonical_decimal("-1.25", money_syntax()).expect("signed");
        assert_eq!(canonical_decimal(signed), "-1.25");
        assert!(parse_canonical_decimal("-1.25", money_syntax()).is_err());
        assert!(parse_signed_canonical_decimal("+1.25", money_syntax()).is_err());
        let zero = parse_signed_canonical_decimal("-0.0000", money_syntax()).expect("neg zero");
        assert_eq!(canonical_decimal(zero), "0");
    }

    #[test]
    fn return_rate_scale_rounds_half_to_even() {
        let rounded = round_to_return_rate_scale(Decimal::from_str("0.0404005").expect("literal"))
            .expect("round");
        assert_eq!(canonical_decimal(rounded), "0.0404");
        let even = round_to_return_rate_scale(Decimal::from_str("0.0404015").expect("literal"))
            .expect("even");
        assert_eq!(canonical_decimal(even), "0.040402");
    }

    #[test]
    fn checked_ln_errors_instead_of_panicking() {
        let negative = catch_unwind(AssertUnwindSafe(|| {
            checked_ln(Decimal::from_str("-1").expect("literal"))
        }));
        let zero = catch_unwind(AssertUnwindSafe(|| checked_ln(Decimal::ZERO)));
        assert!(negative.is_ok(), "checked_ln must not panic on negatives");
        assert!(zero.is_ok(), "checked_ln must not panic on zero");
        assert!(matches!(
            negative.expect("caught"),
            Err(AppError::Validation { .. })
        ));
        assert!(matches!(
            zero.expect("caught"),
            Err(AppError::Validation { .. })
        ));
        assert_eq!(checked_ln(Decimal::ONE).expect("ln(1)"), Decimal::ZERO);
    }

    #[test]
    fn checked_exp_and_powd_error_instead_of_panicking() {
        let overflow_exp = catch_unwind(AssertUnwindSafe(|| checked_exp(Decimal::from(1000))));
        let overflow_pow = catch_unwind(AssertUnwindSafe(|| {
            checked_powd(Decimal::from(10), Decimal::from(80))
        }));
        assert!(overflow_exp.is_ok(), "checked_exp must not panic");
        assert!(overflow_pow.is_ok(), "checked_powd must not panic");
        assert!(matches!(
            overflow_exp.expect("caught"),
            Err(AppError::DecimalOverflow)
        ));
        assert!(matches!(
            overflow_pow.expect("caught"),
            Err(AppError::DecimalOverflow)
        ));
        assert_eq!(checked_exp(Decimal::ZERO).expect("e^0"), Decimal::ONE);
        assert_eq!(
            checked_powd(Decimal::from(2), Decimal::from(3)).expect("2^3"),
            Decimal::from(8)
        );
    }
}
