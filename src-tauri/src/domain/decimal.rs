use std::str::FromStr;

use rust_decimal::{Decimal, RoundingStrategy};

use crate::error::AppError;

pub const MONEY_MAX_INTEGER_DIGITS: usize = 12;
pub const MONEY_MAX_FRACTIONAL_DIGITS: usize = 4;
pub const QUANTITY_MAX_INTEGER_DIGITS: usize = 18;
pub const QUANTITY_MAX_FRACTIONAL_DIGITS: usize = 8;
pub const UNIT_PRICE_MAX_INTEGER_DIGITS: usize = 12;
pub const UNIT_PRICE_MAX_FRACTIONAL_DIGITS: usize = 8;
pub const FX_RATE_MAX_INTEGER_DIGITS: usize = 8;
pub const FX_RATE_MAX_FRACTIONAL_DIGITS: usize = 12;

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
    use super::{canonical_decimal, parse_canonical_decimal, round_to_money_scale, DecimalSyntax};
    use rust_decimal::Decimal;
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
}
