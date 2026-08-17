use std::collections::HashSet;

use super::ids::MemberId;
use crate::error::AppError;

pub const TOTAL_BPS: i32 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnershipShare {
    member_id: MemberId,
    share_bps: i32,
}

impl OwnershipShare {
    pub fn new(member_id: MemberId, share_bps: i32) -> Result<Self, AppError> {
        if share_bps <= 0 || share_bps > TOTAL_BPS {
            return Err(AppError::validation(
                "shareBps",
                "Each ownership share must be between 1 and 10000 basis points.",
            ));
        }
        Ok(Self {
            member_id,
            share_bps,
        })
    }

    #[must_use]
    pub fn member_id(&self) -> MemberId {
        self.member_id
    }

    #[must_use]
    pub fn share_bps(&self) -> i32 {
        self.share_bps
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ownership {
    shares: Vec<OwnershipShare>,
}

impl Ownership {
    pub fn parse(shares: Vec<OwnershipShare>) -> Result<Self, AppError> {
        validate_unique_owners(&shares)?;
        let total: i32 = shares.iter().map(OwnershipShare::share_bps).sum();
        if total != TOTAL_BPS {
            return Err(AppError::OwnershipTotalInvalid { actual_bps: total });
        }
        Ok(Self { shares })
    }

    pub fn equal_split(member_ids: Vec<MemberId>) -> Result<Self, AppError> {
        let owner_count = i32::try_from(member_ids.len())
            .map_err(|_| AppError::validation("owners", "At least one owner is required."))?;
        if owner_count == 0 {
            return Err(AppError::validation(
                "owners",
                "At least one owner is required.",
            ));
        }
        if owner_count > TOTAL_BPS {
            return Err(AppError::validation(
                "owners",
                "Too many owners to split ownership.",
            ));
        }

        let mut seen = HashSet::new();
        for member_id in &member_ids {
            if !seen.insert(*member_id) {
                return Err(AppError::validation("owners", "Owners must be unique."));
            }
        }

        let base = TOTAL_BPS / owner_count;
        let remainder = usize::try_from(TOTAL_BPS % owner_count).expect("remainder fits usize");
        let shares = member_ids
            .into_iter()
            .enumerate()
            .map(|(index, member_id)| OwnershipShare {
                member_id,
                share_bps: base + i32::from(index < remainder),
            })
            .collect();
        Self::parse(shares)
    }

    #[must_use]
    pub fn shares(&self) -> &[OwnershipShare] {
        &self.shares
    }

    #[must_use]
    pub fn is_shared(&self) -> bool {
        self.shares.len() > 1
    }
}

pub fn percent_to_basis_points(percent: &str) -> Result<i32, AppError> {
    if percent.is_empty()
        || percent.contains(|character: char| character != '.' && !character.is_ascii_digit())
    {
        return Err(invalid_percent());
    }

    let (integer, fraction) = match percent.split_once('.') {
        Some((integer, fraction)) => {
            if fraction.is_empty()
                || fraction.len() > 2
                || !fraction.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(invalid_percent());
            }
            (integer, fraction)
        }
        None => (percent, ""),
    };

    if integer.is_empty() || (integer != "0" && integer.starts_with('0')) {
        return Err(invalid_percent());
    }
    if !integer.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_percent());
    }

    let integer_value: i32 = integer.parse().map_err(|_| invalid_percent())?;
    if integer_value > 100 {
        return Err(invalid_percent());
    }

    let fraction_value = match fraction.len() {
        0 => 0,
        1 => {
            fraction
                .parse::<i32>()
                .expect("fraction digits were validated")
                * 10
        }
        2 => fraction
            .parse::<i32>()
            .expect("fraction digits were validated"),
        _ => return Err(invalid_percent()),
    };

    if integer_value == 100 && fraction_value != 0 {
        return Err(invalid_percent());
    }

    let basis_points = integer_value
        .checked_mul(100)
        .and_then(|value| value.checked_add(fraction_value))
        .ok_or_else(invalid_percent)?;
    if !(1..=TOTAL_BPS).contains(&basis_points) {
        return Err(invalid_percent());
    }
    Ok(basis_points)
}

fn validate_unique_owners(shares: &[OwnershipShare]) -> Result<(), AppError> {
    if shares.is_empty() {
        return Err(AppError::validation(
            "owners",
            "At least one owner is required.",
        ));
    }
    let mut seen = HashSet::new();
    for share in shares {
        if !seen.insert(share.member_id) {
            return Err(AppError::validation("owners", "Owners must be unique."));
        }
    }
    Ok(())
}

fn invalid_percent() -> AppError {
    AppError::validation(
        "percent",
        "Ownership percent must be greater than 0 and at most 100, with up to two decimal places.",
    )
}

#[cfg(test)]
mod tests {
    use super::{percent_to_basis_points, Ownership, OwnershipShare, TOTAL_BPS};
    use crate::domain::ids::MemberId;
    use crate::error::AppError;

    fn share(bps: i32) -> OwnershipShare {
        OwnershipShare::new(MemberId::new(), bps).expect("valid share")
    }

    #[test]
    fn accepts_full_and_even_manual_totals() {
        let sole = Ownership::parse(vec![share(TOTAL_BPS)]).expect("100%");
        assert!(!sole.is_shared());
        let first = share(5_000);
        let second = OwnershipShare::new(MemberId::new(), 5_000).expect("50%");
        let shared = Ownership::parse(vec![first, second]).expect("50/50");
        assert!(shared.is_shared());
        assert_eq!(
            shared
                .shares()
                .iter()
                .map(OwnershipShare::share_bps)
                .sum::<i32>(),
            TOTAL_BPS
        );
    }

    #[test]
    fn rejects_zero_shares_and_invalid_totals_without_rewriting() {
        assert!(OwnershipShare::new(MemberId::new(), 0).is_err());
        let error =
            Ownership::parse(vec![share(6_000), share(5_000)]).expect_err("110% must stay invalid");
        assert!(matches!(
            error,
            AppError::OwnershipTotalInvalid { actual_bps: 11_000 }
        ));
        let three_manual = vec![share(3_333), share(3_333), share(3_333)];
        let error = Ownership::parse(three_manual).expect_err("9999 must not be auto-corrected");
        assert!(matches!(
            error,
            AppError::OwnershipTotalInvalid { actual_bps: 9_999 }
        ));
    }

    #[test]
    fn equal_split_assigns_remainder_from_the_first_owner() {
        let members = vec![MemberId::new(), MemberId::new(), MemberId::new()];
        let ownership = Ownership::equal_split(members).expect("three-way split");
        let shares: Vec<i32> = ownership
            .shares()
            .iter()
            .map(OwnershipShare::share_bps)
            .collect();
        assert_eq!(shares, vec![3_334, 3_333, 3_333]);
    }

    #[test]
    fn percent_to_basis_points_converts_two_decimal_strings() {
        assert_eq!(percent_to_basis_points("100").expect("100%"), 10_000);
        assert_eq!(percent_to_basis_points("50").expect("50%"), 5_000);
        assert_eq!(percent_to_basis_points("33.33").expect("33.33%"), 3_333);
        assert_eq!(percent_to_basis_points("0.01").expect("0.01%"), 1);
        assert!(percent_to_basis_points("0").is_err());
        assert!(percent_to_basis_points("100.01").is_err());
        assert!(percent_to_basis_points("33.333").is_err());
        assert!(percent_to_basis_points("01").is_err());
    }

    #[test]
    fn rejects_duplicate_or_empty_owners() {
        let member = MemberId::new();
        let first = OwnershipShare::new(member, 5_000).expect("share");
        let duplicate = OwnershipShare::new(member, 5_000).expect("share");
        assert!(Ownership::parse(vec![first, duplicate]).is_err());
        assert!(Ownership::parse(Vec::new()).is_err());
        assert!(Ownership::equal_split(Vec::new()).is_err());
        assert!(Ownership::equal_split(vec![member, member]).is_err());
    }
}
