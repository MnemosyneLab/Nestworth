use super::{
    currency::CurrencyCode,
    ids::{AccountCashValueId, AccountId},
    money::Money,
    time::Timestamp,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountCashValue {
    id: AccountCashValueId,
    account_id: AccountId,
    money: Money,
    effective_at: Timestamp,
    created_at: Timestamp,
}

impl AccountCashValue {
    pub fn new(account_id: AccountId, money: Money, now: Timestamp) -> Self {
        Self {
            id: AccountCashValueId::new(),
            account_id,
            money,
            effective_at: now.clone(),
            created_at: now,
        }
    }

    #[must_use]
    pub fn from_persisted(
        id: AccountCashValueId,
        account_id: AccountId,
        money: Money,
        effective_at: Timestamp,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            account_id,
            money,
            effective_at,
            created_at,
        }
    }

    #[must_use]
    pub fn id(&self) -> AccountCashValueId {
        self.id
    }

    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    #[must_use]
    pub fn money(&self) -> Money {
        self.money
    }

    #[must_use]
    pub fn currency(&self) -> CurrencyCode {
        self.money.currency()
    }

    #[must_use]
    pub fn effective_at(&self) -> &Timestamp {
        &self.effective_at
    }

    #[must_use]
    pub fn created_at(&self) -> &Timestamp {
        &self.created_at
    }
}

#[cfg(test)]
mod tests {
    use super::AccountCashValue;
    use crate::domain::currency::CurrencyCode;
    use crate::domain::ids::AccountId;
    use crate::domain::money::Money;
    use crate::domain::time::Timestamp;

    #[test]
    fn zero_cash_is_a_valid_observation() {
        let cash = AccountCashValue::new(
            AccountId::new(),
            Money::parse("0", CurrencyCode::SGD).expect("zero"),
            Timestamp::now(),
        );
        assert!(cash.money().is_zero());
        assert_eq!(cash.currency(), CurrencyCode::SGD);
    }
}
