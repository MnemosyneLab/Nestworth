use super::{
    ids::{HouseholdId, InstitutionId, MediaAssetId},
    text::{
        parse_country_code, parse_name, parse_optional_note, parse_optional_text, NAME_MAX_CHARS,
        NOTE_MAX_CHARS,
    },
    time::Timestamp,
};
use crate::error::AppError;

pub struct PersistedInstitution {
    pub id: InstitutionId,
    pub household_id: HouseholdId,
    pub name: String,
    pub institution_type: Option<String>,
    pub country_code: Option<String>,
    pub website: Option<String>,
    pub note: Option<String>,
    pub logo_asset_id: Option<MediaAssetId>,
    pub sort_order: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub archived_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInstitution {
    pub household_id: HouseholdId,
    pub name: String,
    pub institution_type: Option<String>,
    pub country_code: Option<String>,
    pub website: Option<String>,
    pub note: Option<String>,
    pub logo_asset_id: Option<MediaAssetId>,
    pub sort_order: i64,
}

impl NewInstitution {
    pub fn required(household_id: HouseholdId, name: impl Into<String>) -> Self {
        Self {
            household_id,
            name: name.into(),
            institution_type: None,
            country_code: None,
            website: None,
            note: None,
            logo_asset_id: None,
            sort_order: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Institution {
    id: InstitutionId,
    household_id: HouseholdId,
    name: String,
    institution_type: Option<String>,
    country_code: Option<String>,
    website: Option<String>,
    note: Option<String>,
    logo_asset_id: Option<MediaAssetId>,
    sort_order: i64,
    created_at: Timestamp,
    updated_at: Timestamp,
    archived_at: Option<Timestamp>,
}

impl Institution {
    pub fn new(input: NewInstitution, now: Timestamp) -> Result<Self, AppError> {
        Ok(Self {
            id: InstitutionId::new(),
            household_id: input.household_id,
            name: parse_name(&input.name)?,
            institution_type: parse_optional_text(
                input.institution_type.as_deref(),
                NAME_MAX_CHARS,
                "institutionType",
            )?,
            country_code: parse_country_code(input.country_code.as_deref())?,
            website: parse_optional_text(input.website.as_deref(), NOTE_MAX_CHARS, "website")?,
            note: parse_optional_note(input.note.as_deref())?,
            logo_asset_id: input.logo_asset_id,
            sort_order: input.sort_order,
            created_at: now.clone(),
            updated_at: now,
            archived_at: None,
        })
    }

    #[must_use]
    pub fn from_persisted(row: PersistedInstitution) -> Self {
        Self {
            id: row.id,
            household_id: row.household_id,
            name: row.name,
            institution_type: row.institution_type,
            country_code: row.country_code,
            website: row.website,
            note: row.note,
            logo_asset_id: row.logo_asset_id,
            sort_order: row.sort_order,
            created_at: row.created_at,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
        }
    }

    pub fn update(&mut self, input: NewInstitution, now: Timestamp) -> Result<(), AppError> {
        self.name = parse_name(&input.name)?;
        self.institution_type = parse_optional_text(
            input.institution_type.as_deref(),
            NAME_MAX_CHARS,
            "institutionType",
        )?;
        self.country_code = parse_country_code(input.country_code.as_deref())?;
        self.website = parse_optional_text(input.website.as_deref(), NOTE_MAX_CHARS, "website")?;
        self.note = parse_optional_note(input.note.as_deref())?;
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

    pub fn set_logo(&mut self, logo_asset_id: MediaAssetId, now: Timestamp) {
        self.logo_asset_id = Some(logo_asset_id);
        self.updated_at = now;
    }

    #[must_use]
    pub fn id(&self) -> InstitutionId {
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
    pub fn institution_type(&self) -> Option<&str> {
        self.institution_type.as_deref()
    }

    #[must_use]
    pub fn country_code(&self) -> Option<&str> {
        self.country_code.as_deref()
    }

    #[must_use]
    pub fn website(&self) -> Option<&str> {
        self.website.as_deref()
    }

    #[must_use]
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    #[must_use]
    pub fn logo_asset_id(&self) -> Option<MediaAssetId> {
        self.logo_asset_id
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
    use super::{Institution, NewInstitution};
    use crate::domain::ids::HouseholdId;
    use crate::domain::time::Timestamp;

    #[test]
    fn validates_optional_country_code() {
        let mut input = NewInstitution::required(HouseholdId::new(), "DBS");
        input.institution_type = Some("bank".to_owned());
        input.country_code = Some("SG".to_owned());
        input.website = Some("https://www.dbs.com".to_owned());
        let institution = Institution::new(input, Timestamp::now()).expect("valid institution");
        assert_eq!(institution.country_code(), Some("SG"));

        let mut invalid = NewInstitution::required(HouseholdId::new(), "DBS");
        invalid.country_code = Some("sg".to_owned());
        assert!(Institution::new(invalid, Timestamp::now()).is_err());
    }
}
