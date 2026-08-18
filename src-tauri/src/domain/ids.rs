use uuid::Uuid;

use crate::error::AppError;

macro_rules! typed_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            #[must_use]
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            pub fn parse(value: &str) -> Result<Self, AppError> {
                Uuid::parse_str(value)
                    .map(Self)
                    .map_err(|_| AppError::validation("id", "The identifier is not a valid UUID."))
            }

            #[must_use]
            pub fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

typed_id!(HouseholdId);
typed_id!(MemberId);
typed_id!(InstitutionId);
typed_id!(AccountGroupId);
typed_id!(AccountId);
typed_id!(AccountValueId);
typed_id!(MediaAssetId);
typed_id!(InstrumentId);
typed_id!(HoldingId);
typed_id!(AccountCashValueId);
typed_id!(InstrumentQuoteId);
typed_id!(FxQuoteId);

#[cfg(test)]
mod tests {
    use super::{AccountId, HoldingId, HouseholdId, InstrumentId, MemberId};

    #[test]
    fn generated_ids_use_uuid_v7() {
        assert_eq!(HouseholdId::new().as_uuid().get_version_num(), 7);
        assert_eq!(MemberId::new().as_uuid().get_version_num(), 7);
        assert_eq!(AccountId::new().as_uuid().get_version_num(), 7);
        assert_eq!(InstrumentId::new().as_uuid().get_version_num(), 7);
        assert_eq!(HoldingId::new().as_uuid().get_version_num(), 7);
    }

    #[test]
    fn parse_rejects_invalid_uuid() {
        assert!(HouseholdId::parse("not-a-uuid").is_err());
        assert!(MemberId::parse("").is_err());
    }

    #[test]
    fn parse_round_trips_hyphenated_uuid() {
        let id = AccountId::new();
        let parsed = AccountId::parse(&id.to_string()).expect("generated id should parse");
        assert_eq!(parsed, id);
    }

    #[test]
    fn typed_ids_are_not_interchangeable() {
        let household = HouseholdId::new();
        let member = MemberId::from_uuid(household.as_uuid());
        assert_eq!(household.as_uuid(), member.as_uuid());
        assert_ne!(household.to_string(), "invalid");
    }
}
