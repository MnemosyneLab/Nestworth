use super::{
    ids::{HouseholdId, MediaAssetId, MemberId},
    text::{parse_name, parse_optional_note},
    time::Timestamp,
};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    id: MemberId,
    household_id: HouseholdId,
    name: String,
    avatar_asset_id: Option<MediaAssetId>,
    note: Option<String>,
    sort_order: i64,
    created_at: Timestamp,
    updated_at: Timestamp,
    archived_at: Option<Timestamp>,
}

impl Member {
    pub fn new(
        household_id: HouseholdId,
        name: &str,
        avatar_asset_id: Option<MediaAssetId>,
        note: Option<&str>,
        sort_order: i64,
        now: Timestamp,
    ) -> Result<Self, AppError> {
        Ok(Self {
            id: MemberId::new(),
            household_id,
            name: parse_name(name)?,
            avatar_asset_id,
            note: parse_optional_note(note)?,
            sort_order,
            created_at: now.clone(),
            updated_at: now,
            archived_at: None,
        })
    }

    pub fn rename(&mut self, name: &str, now: Timestamp) -> Result<(), AppError> {
        self.name = parse_name(name)?;
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
    pub fn id(&self) -> MemberId {
        self.id
    }

    #[must_use]
    pub fn household_id(&self) -> HouseholdId {
        self.household_id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn avatar_asset_id(&self) -> Option<MediaAssetId> {
        self.avatar_asset_id
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
    use super::Member;
    use crate::domain::ids::HouseholdId;
    use crate::domain::time::Timestamp;

    #[test]
    fn archives_and_restores_without_changing_identity() {
        let now = Timestamp::now();
        let mut member = Member::new(HouseholdId::new(), "Walt", None, Some(" primary "), 0, now)
            .expect("valid member");
        assert_eq!(member.note(), Some("primary"));
        member.archive(Timestamp::now());
        assert!(member.is_archived());
        member.restore(Timestamp::now());
        assert!(!member.is_archived());
        assert!(Member::new(HouseholdId::new(), "", None, None, 0, Timestamp::now()).is_err());
    }
}
