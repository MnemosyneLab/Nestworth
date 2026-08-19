//! Decimal XIRR solver. SQL-free; no binary floating point.

use rust_decimal::Decimal;

use super::decimal::{checked_add, checked_div, checked_mul, checked_powd, checked_sub};
use super::time::CalendarDate;
use crate::error::AppError;

const MAX_NEWTON_ITERATIONS: u32 = 100;
const MAX_BISECTION_ITERATIONS: u32 = 200;
const INITIAL_RATE: Decimal = Decimal::from_parts(1, 0, 0, false, 1);
const NPV_TOLERANCE: Decimal = Decimal::from_parts(1, 0, 0, false, 4);
const RATE_STEP_TOLERANCE: Decimal = Decimal::from_parts(1, 0, 0, false, 9);
const BRACKET_LOW: Decimal = Decimal::from_parts(999_999, 0, 0, true, 6);
const BRACKET_HIGH: Decimal = Decimal::from_parts(100, 0, 0, false, 0);
const DAY_COUNT: Decimal = Decimal::from_parts(365, 0, 0, false, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XirrError {
    NoSignChange,
    NotComputable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XirrCashflow {
    pub date: CalendarDate,
    pub amount: Decimal,
}

pub fn solve_xirr(cashflows: &[XirrCashflow]) -> Result<Decimal, XirrError> {
    solve_xirr_bounded(cashflows, MAX_NEWTON_ITERATIONS, MAX_BISECTION_ITERATIONS)
}

pub fn solve_xirr_bounded(
    cashflows: &[XirrCashflow],
    max_newton: u32,
    max_bisection: u32,
) -> Result<Decimal, XirrError> {
    if cashflows.len() < 2 {
        return Err(XirrError::NotComputable);
    }
    let has_positive = cashflows.iter().any(|flow| flow.amount.is_sign_positive());
    let has_negative = cashflows
        .iter()
        .any(|flow| flow.amount.is_sign_negative() && !flow.amount.is_zero());
    if !has_positive || !has_negative {
        return Err(XirrError::NoSignChange);
    }
    let times = year_fractions(cashflows).ok_or(XirrError::NotComputable)?;
    let amounts: Vec<Decimal> = cashflows.iter().map(|flow| flow.amount).collect();
    if times.iter().all(|time| time.is_zero()) {
        let net = amounts
            .iter()
            .try_fold(Decimal::ZERO, |total, amount| checked_add(total, *amount));
        return match net {
            Ok(value) if value.abs() <= NPV_TOLERANCE => Ok(Decimal::ZERO),
            _ => Err(XirrError::NotComputable),
        };
    }
    match newton(&amounts, &times, max_newton) {
        Ok(rate) => Ok(rate),
        Err(_) => bisection(&amounts, &times, max_bisection),
    }
}

fn year_fractions(cashflows: &[XirrCashflow]) -> Option<Vec<Decimal>> {
    let origin = cashflows[0].date;
    cashflows
        .iter()
        .map(|flow| {
            let days = Decimal::from(
                flow.date
                    .as_naive_date()
                    .signed_duration_since(origin.as_naive_date())
                    .num_days(),
            );
            checked_div(days, DAY_COUNT).ok()
        })
        .collect()
}

fn newton(
    amounts: &[Decimal],
    times: &[Decimal],
    max_iterations: u32,
) -> Result<Decimal, XirrError> {
    let mut rate = INITIAL_RATE;
    for _ in 0..max_iterations {
        let npv = npv_at(amounts, times, rate).map_err(|_| XirrError::NotComputable)?;
        if npv.abs() <= NPV_TOLERANCE {
            return Ok(rate);
        }
        let derivative =
            npv_derivative(amounts, times, rate).map_err(|_| XirrError::NotComputable)?;
        if derivative.is_zero() {
            return Err(XirrError::NotComputable);
        }
        let step = checked_div(npv, derivative).map_err(|_| XirrError::NotComputable)?;
        let next = checked_sub(rate, step).map_err(|_| XirrError::NotComputable)?;
        if !in_bracket(next) {
            return Err(XirrError::NotComputable);
        }
        let delta = checked_sub(next, rate)
            .map_err(|_| XirrError::NotComputable)?
            .abs();
        rate = next;
        if delta <= RATE_STEP_TOLERANCE {
            let settled = npv_at(amounts, times, rate).map_err(|_| XirrError::NotComputable)?;
            if settled.abs() <= NPV_TOLERANCE {
                return Ok(rate);
            }
            return Err(XirrError::NotComputable);
        }
    }
    Err(XirrError::NotComputable)
}

fn bisection(
    amounts: &[Decimal],
    times: &[Decimal],
    max_iterations: u32,
) -> Result<Decimal, XirrError> {
    let mut low = BRACKET_LOW;
    let mut high = BRACKET_HIGH;
    let mut npv_low = npv_at(amounts, times, low).map_err(|_| XirrError::NotComputable)?;
    let npv_high = npv_at(amounts, times, high).map_err(|_| XirrError::NotComputable)?;
    if npv_low.abs() <= NPV_TOLERANCE {
        return Ok(low);
    }
    if npv_high.abs() <= NPV_TOLERANCE {
        return Ok(high);
    }
    if !opposite_sign(npv_low, npv_high) {
        return Err(XirrError::NotComputable);
    }
    for _ in 0..max_iterations {
        let mid = checked_div(
            checked_add(low, high).map_err(|_| XirrError::NotComputable)?,
            Decimal::from(2),
        )
        .map_err(|_| XirrError::NotComputable)?;
        let npv_mid = npv_at(amounts, times, mid).map_err(|_| XirrError::NotComputable)?;
        if npv_mid.abs() <= NPV_TOLERANCE {
            return Ok(mid);
        }
        let width = checked_sub(high, low)
            .map_err(|_| XirrError::NotComputable)?
            .abs();
        if width <= RATE_STEP_TOLERANCE {
            return Err(XirrError::NotComputable);
        }
        if opposite_sign(npv_low, npv_mid) {
            high = mid;
        } else {
            low = mid;
            npv_low = npv_mid;
        }
    }
    Err(XirrError::NotComputable)
}

fn npv_at(amounts: &[Decimal], times: &[Decimal], rate: Decimal) -> Result<Decimal, AppError> {
    let one_plus_r = checked_add(Decimal::ONE, rate)?;
    if one_plus_r.is_sign_negative() || one_plus_r.is_zero() {
        return Err(AppError::DecimalOverflow);
    }
    let mut npv = Decimal::ZERO;
    for (amount, time) in amounts.iter().zip(times) {
        let growth = if time.is_zero() {
            Decimal::ONE
        } else {
            checked_powd(one_plus_r, *time)?
        };
        npv = checked_add(npv, checked_div(*amount, growth)?)?;
    }
    Ok(npv)
}

fn npv_derivative(
    amounts: &[Decimal],
    times: &[Decimal],
    rate: Decimal,
) -> Result<Decimal, AppError> {
    let one_plus_r = checked_add(Decimal::ONE, rate)?;
    if one_plus_r.is_sign_negative() || one_plus_r.is_zero() {
        return Err(AppError::DecimalOverflow);
    }
    let mut derivative = Decimal::ZERO;
    for (amount, time) in amounts.iter().zip(times) {
        if time.is_zero() {
            continue;
        }
        let exponent = checked_add(*time, Decimal::ONE)?;
        let growth = checked_powd(one_plus_r, exponent)?;
        let term = checked_div(checked_mul(*amount, -*time)?, growth)?;
        derivative = checked_add(derivative, term)?;
    }
    Ok(derivative)
}

fn in_bracket(rate: Decimal) -> bool {
    rate > BRACKET_LOW && rate <= BRACKET_HIGH
}

fn opposite_sign(left: Decimal, right: Decimal) -> bool {
    (left.is_sign_negative() && !right.is_sign_negative() && !right.is_zero())
        || (right.is_sign_negative() && !left.is_sign_negative() && !left.is_zero())
}

#[cfg(test)]
mod tests {
    use super::{solve_xirr, solve_xirr_bounded, XirrCashflow, XirrError};
    use crate::domain::return_rate::ReturnRate;
    use crate::domain::CalendarDate;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn date(days_from_epoch: i64) -> CalendarDate {
        CalendarDate::from_naive_date(
            chrono::NaiveDate::from_ymd_opt(2020, 1, 1)
                .expect("epoch")
                .checked_add_signed(chrono::Duration::days(days_from_epoch))
                .expect("date"),
        )
    }

    fn flow(days: i64, amount: &str) -> XirrCashflow {
        XirrCashflow {
            date: date(days),
            amount: Decimal::from_str(amount).expect("amount"),
        }
    }

    #[test]
    fn annual_ten_percent_is_exactly_one_tenth() {
        let rate = solve_xirr(&[flow(0, "-100000"), flow(365, "110000")]).expect("xirr");
        assert_eq!(
            ReturnRate::from_canonical(rate).expect("round").canonical(),
            "0.1"
        );
    }

    #[test]
    fn two_annual_outlays_converge_to_six_fractional_digits() {
        let rate = solve_xirr(&[
            flow(0, "-100000"),
            flow(365, "-100000"),
            flow(730, "230000"),
        ])
        .expect("xirr");
        assert_eq!(
            ReturnRate::from_canonical(rate).expect("round").canonical(),
            "0.096872"
        );
    }

    #[test]
    fn same_date_net_zero_is_zero_not_the_seed_rate() {
        let rate = solve_xirr(&[flow(0, "-63190"), flow(0, "63190")]).expect("zero");
        assert_eq!(rate, Decimal::ZERO);
    }

    #[test]
    fn no_sign_change_is_not_computable() {
        assert_eq!(
            solve_xirr(&[flow(0, "-100"), flow(365, "-50")]),
            Err(XirrError::NoSignChange)
        );
        assert_eq!(
            solve_xirr(&[flow(0, "100"), flow(365, "50")]),
            Err(XirrError::NoSignChange)
        );
    }

    #[test]
    fn non_converging_series_is_not_computable() {
        let failed = solve_xirr(&[flow(0, "100"), flow(365, "-50"), flow(730, "100")]);
        assert_eq!(failed, Err(XirrError::NotComputable));
        let forced = solve_xirr_bounded(&[flow(0, "-100000"), flow(365, "110000")], 0, 0);
        assert_eq!(forced, Err(XirrError::NotComputable));
        assert_ne!(forced, Ok(Decimal::from_str("0.1").expect("literal")));
    }

    #[test]
    fn solver_source_has_no_binary_floats() {
        let source = include_str!("xirr.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production");
        assert!(
            !source.contains("f32") && !source.contains("f64"),
            "binary floats are prohibited in the XIRR solver"
        );
    }
}
