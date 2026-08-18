use super::{
    ids::{AccountId, HoldingId, InstrumentId},
    quantity::Quantity,
    text::parse_optional_note,
    time::Timestamp,
};
use crate::error::AppError;

pub struct PersistedHolding {
    pub id: HoldingId,
    pub account_id: AccountId,
    pub instrument_id: InstrumentId,
    pub quantity: Quantity,
    pub note: Option<String>,
    pub sort_order: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub archived_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holding {
    id: HoldingId,
    account_id: AccountId,
    instrument_id: InstrumentId,
    quantity: Quantity,
    note: Option<String>,
    sort_order: i64,
    created_at: Timestamp,
    updated_at: Timestamp,
    archived_at: Option<Timestamp>,
}

impl Holding {
    pub fn new(
        account_id: AccountId,
        instrument_id: InstrumentId,
        quantity: Quantity,
        note: Option<&str>,
        sort_order: i64,
        now: Timestamp,
    ) -> Result<Self, AppError> {
        Ok(Self {
            id: HoldingId::new(),
            account_id,
            instrument_id,
            quantity,
            note: parse_optional_note(note)?,
            sort_order,
            created_at: now.clone(),
            updated_at: now,
            archived_at: None,
        })
    }

    #[must_use]
    pub fn from_persisted(row: PersistedHolding) -> Self {
        Self {
            id: row.id,
            account_id: row.account_id,
            instrument_id: row.instrument_id,
            quantity: row.quantity,
            note: row.note,
            sort_order: row.sort_order,
            created_at: row.created_at,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
        }
    }

    pub fn update_current_state(
        &mut self,
        quantity: Quantity,
        note: Option<&str>,
        now: Timestamp,
    ) -> Result<(), AppError> {
        self.quantity = quantity;
        self.note = parse_optional_note(note)?;
        self.updated_at = now;
        Ok(())
    }

    pub fn archive(&mut self, now: Timestamp) {
        if self.archived_at.is_none() {
            self.archived_at = Some(now.clone());
        }
        self.updated_at = now;
    }

    pub fn restore(&mut self, now: Timestamp) {
        self.archived_at = None;
        self.updated_at = now;
    }

    #[must_use]
    pub fn id(&self) -> HoldingId {
        self.id
    }

    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[must_use]
    pub fn quantity(&self) -> Quantity {
        self.quantity
    }

    #[must_use]
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    #[must_use]
    pub fn sort_order(&self) -> i64 {
        self.sort_order
    }

    #[must_use]
    pub fn created_at(&self) -> &Timestamp {
        &self.created_at
    }

    #[must_use]
    pub fn updated_at(&self) -> &Timestamp {
        &self.updated_at
    }

    #[must_use]
    pub fn archived_at(&self) -> Option<&Timestamp> {
        self.archived_at.as_ref()
    }

    #[must_use]
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::Holding;
    use crate::domain::ids::{AccountId, InstrumentId};
    use crate::domain::quantity::Quantity;
    use crate::domain::time::Timestamp;

    #[test]
    fn zero_quantity_is_valid_current_state() {
        let mut holding = Holding::new(
            AccountId::new(),
            InstrumentId::new(),
            Quantity::parse("3").expect("qty"),
            None,
            0,
            Timestamp::now(),
        )
        .expect("holding");
        holding
            .update_current_state(
                Quantity::parse("0").expect("zero"),
                Some("kept"),
                Timestamp::now(),
            )
            .expect("update");
        assert!(holding.quantity().is_zero());
        assert_eq!(holding.note(), Some("kept"));
        assert!(!holding.is_archived());
    }
}
