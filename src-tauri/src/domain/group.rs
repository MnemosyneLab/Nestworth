use super::{
    ids::{AccountGroupId, HouseholdId, MediaAssetId},
    text::{parse_name, parse_optional_text, NAME_MAX_CHARS, NOTE_MAX_CHARS},
    time::Timestamp,
};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccountGroup {
    pub household_id: HouseholdId,
    pub name: String,
    pub icon_key: Option<String>,
    pub color: Option<String>,
    pub logo_asset_id: Option<MediaAssetId>,
    pub description: Option<String>,
    pub sort_order: i64,
}

impl NewAccountGroup {
    pub fn required(household_id: HouseholdId, name: impl Into<String>) -> Self {
        Self {
            household_id,
            name: name.into(),
            icon_key: None,
            color: None,
            logo_asset_id: None,
            description: None,
            sort_order: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountGroup {
    id: AccountGroupId,
    household_id: HouseholdId,
    name: String,
    icon_key: Option<String>,
    color: Option<String>,
    logo_asset_id: Option<MediaAssetId>,
    description: Option<String>,
    sort_order: i64,
    created_at: Timestamp,
    updated_at: Timestamp,
    archived_at: Option<Timestamp>,
}

impl AccountGroup {
    pub fn new(input: NewAccountGroup, now: Timestamp) -> Result<Self, AppError> {
        Ok(Self {
            id: AccountGroupId::new(),
            household_id: input.household_id,
            name: parse_name(&input.name)?,
            icon_key: parse_optional_text(input.icon_key.as_deref(), NAME_MAX_CHARS, "iconKey")?,
            color: parse_optional_text(input.color.as_deref(), NAME_MAX_CHARS, "color")?,
            logo_asset_id: input.logo_asset_id,
            description: parse_optional_text(
                input.description.as_deref(),
                NOTE_MAX_CHARS,
                "description",
            )?,
            sort_order: input.sort_order,
            created_at: now.clone(),
            updated_at: now,
            archived_at: None,
        })
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
    pub fn id(&self) -> AccountGroupId {
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
    pub fn icon_key(&self) -> Option<&str> {
        self.icon_key.as_deref()
    }

    #[must_use]
    pub fn color(&self) -> Option<&str> {
        self.color.as_deref()
    }

    #[must_use]
    pub fn logo_asset_id(&self) -> Option<MediaAssetId> {
        self.logo_asset_id
    }

    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
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
    use super::{AccountGroup, NewAccountGroup};
    use crate::domain::ids::HouseholdId;
    use crate::domain::time::Timestamp;

    #[test]
    fn creates_group_with_trimmed_name() {
        let mut input = NewAccountGroup::required(HouseholdId::new(), " Emergency ");
        input.icon_key = Some("shield".to_owned());
        input.color = Some("#2563EB".to_owned());
        input.description = Some("cash buffer".to_owned());
        input.sort_order = 1;
        let group = AccountGroup::new(input, Timestamp::now()).expect("valid group");
        assert_eq!(group.name(), "Emergency");
        assert_eq!(group.icon_key(), Some("shield"));
        assert!(AccountGroup::new(
            NewAccountGroup::required(HouseholdId::new(), ""),
            Timestamp::now(),
        )
        .is_err());
    }
}
