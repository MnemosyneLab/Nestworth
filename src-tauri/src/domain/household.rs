use super::{currency::CurrencyCode, ids::HouseholdId, text::parse_name, time::Timestamp};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Household {
    id: HouseholdId,
    name: String,
    base_currency: CurrencyCode,
    created_at: Timestamp,
    updated_at: Timestamp,
}

impl Household {
    pub fn new(name: &str, base_currency: CurrencyCode, now: Timestamp) -> Result<Self, AppError> {
        Ok(Self {
            id: HouseholdId::new(),
            name: parse_name(name)?,
            base_currency,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn rename(&mut self, name: &str, now: Timestamp) -> Result<(), AppError> {
        self.name = parse_name(name)?;
        self.updated_at = now;
        Ok(())
    }

    pub fn change_base_currency(&mut self, base_currency: CurrencyCode, now: Timestamp) {
        self.base_currency = base_currency;
        self.updated_at = now;
    }

    #[must_use]
    pub fn id(&self) -> HouseholdId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn base_currency(&self) -> CurrencyCode {
        self.base_currency
    }

    #[must_use]
    pub fn created_at(&self) -> &Timestamp {
        &self.created_at
    }

    #[must_use]
    pub fn updated_at(&self) -> &Timestamp {
        &self.updated_at
    }
}

#[cfg(test)]
mod tests {
    use super::Household;
    use crate::domain::currency::CurrencyCode;
    use crate::domain::time::Timestamp;

    #[test]
    fn creates_household_with_validated_name_and_currency() {
        let now = Timestamp::now();
        let household = Household::new("  Wang Family  ", CurrencyCode::CNY, now.clone())
            .expect("valid household");
        assert_eq!(household.name(), "Wang Family");
        assert_eq!(household.base_currency(), CurrencyCode::CNY);
        assert_eq!(household.created_at(), household.updated_at());
        assert!(Household::new("", CurrencyCode::CNY, now).is_err());
    }
}
