use rust_decimal::Decimal;

use super::{
    category::{PrimaryCategory, SecondaryCategory, TrackingMode},
    currency::CurrencyCode,
    ids::{AccountGroupId, AccountId, AccountValueId, HouseholdId, InstitutionId, MediaAssetId},
    money::Money,
    text::{parse_name, parse_optional_note},
    time::{CalendarDate, Timestamp},
};
use crate::error::AppError;

pub struct PersistedAccount {
    pub id: AccountId,
    pub household_id: HouseholdId,
    pub institution_id: Option<InstitutionId>,
    pub group_id: Option<AccountGroupId>,
    pub name: String,
    pub primary_category: PrimaryCategory,
    pub secondary_category: SecondaryCategory,
    pub tracking_mode: TrackingMode,
    pub default_currency: CurrencyCode,
    pub note: Option<String>,
    pub logo_asset_id: Option<MediaAssetId>,
    pub include_in_net_worth: bool,
    pub include_in_investment: bool,
    pub include_in_liquid_assets: bool,
    pub opened_on: Option<CalendarDate>,
    pub closed_on: Option<CalendarDate>,
    pub sort_order: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub archived_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueKind {
    Balance,
    ManualValue,
}

impl ValueKind {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "balance" => Ok(Self::Balance),
            "manual_value" => Ok(Self::ManualValue),
            _ => Err(AppError::invalid_category(
                "Account value kind is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Balance => "balance",
            Self::ManualValue => "manual_value",
        }
    }

    pub fn from_tracking_mode(mode: TrackingMode) -> Result<Self, AppError> {
        match mode {
            TrackingMode::Balance => Ok(Self::Balance),
            TrackingMode::ManualValue => Ok(Self::ManualValue),
            TrackingMode::Holdings => Err(AppError::invalid_category(
                "Holdings accounts cannot record a simple account value.",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccount {
    pub household_id: HouseholdId,
    pub name: String,
    pub primary_category: PrimaryCategory,
    pub secondary_category: SecondaryCategory,
    pub default_currency: CurrencyCode,
    pub institution_id: Option<InstitutionId>,
    pub group_id: Option<AccountGroupId>,
    pub tracking_mode: Option<TrackingMode>,
    pub note: Option<String>,
    pub logo_asset_id: Option<MediaAssetId>,
    pub include_in_net_worth: bool,
    pub include_in_investment: bool,
    pub include_in_liquid_assets: bool,
    pub opened_on: Option<CalendarDate>,
    pub closed_on: Option<CalendarDate>,
    pub sort_order: i64,
}

impl NewAccount {
    pub fn required(
        household_id: HouseholdId,
        name: impl Into<String>,
        primary_category: PrimaryCategory,
        secondary_category: SecondaryCategory,
        default_currency: CurrencyCode,
    ) -> Self {
        Self {
            household_id,
            name: name.into(),
            primary_category,
            secondary_category,
            default_currency,
            institution_id: None,
            group_id: None,
            tracking_mode: None,
            note: None,
            logo_asset_id: None,
            include_in_net_worth: true,
            include_in_investment: false,
            include_in_liquid_assets: false,
            opened_on: None,
            closed_on: None,
            sort_order: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    id: AccountId,
    household_id: HouseholdId,
    institution_id: Option<InstitutionId>,
    group_id: Option<AccountGroupId>,
    name: String,
    primary_category: PrimaryCategory,
    secondary_category: SecondaryCategory,
    tracking_mode: TrackingMode,
    default_currency: CurrencyCode,
    note: Option<String>,
    logo_asset_id: Option<MediaAssetId>,
    include_in_net_worth: bool,
    include_in_investment: bool,
    include_in_liquid_assets: bool,
    opened_on: Option<CalendarDate>,
    closed_on: Option<CalendarDate>,
    sort_order: i64,
    created_at: Timestamp,
    updated_at: Timestamp,
    archived_at: Option<Timestamp>,
}

impl Account {
    pub fn new(input: NewAccount, now: Timestamp) -> Result<Self, AppError> {
        input
            .secondary_category
            .require_primary(input.primary_category)?;
        let tracking_mode = input
            .tracking_mode
            .unwrap_or_else(|| input.primary_category.default_tracking_mode());
        tracking_mode.require_for_new_account(input.primary_category)?;
        if let (Some(opened_on), Some(closed_on)) = (input.opened_on, input.closed_on) {
            if closed_on < opened_on {
                return Err(AppError::validation(
                    "closedOn",
                    "Closed date cannot be earlier than opened date.",
                ));
            }
        }

        Ok(Self {
            id: AccountId::new(),
            household_id: input.household_id,
            institution_id: input.institution_id,
            group_id: input.group_id,
            name: parse_name(&input.name)?,
            primary_category: input.primary_category,
            secondary_category: input.secondary_category,
            tracking_mode,
            default_currency: input.default_currency,
            note: parse_optional_note(input.note.as_deref())?,
            logo_asset_id: input.logo_asset_id,
            include_in_net_worth: input.include_in_net_worth,
            include_in_investment: input.include_in_investment,
            include_in_liquid_assets: input.include_in_liquid_assets,
            opened_on: input.opened_on,
            closed_on: input.closed_on,
            sort_order: input.sort_order,
            created_at: now.clone(),
            updated_at: now,
            archived_at: None,
        })
    }

    #[must_use]
    pub fn from_persisted(row: PersistedAccount) -> Self {
        Self {
            id: row.id,
            household_id: row.household_id,
            institution_id: row.institution_id,
            group_id: row.group_id,
            name: row.name,
            primary_category: row.primary_category,
            secondary_category: row.secondary_category,
            tracking_mode: row.tracking_mode,
            default_currency: row.default_currency,
            note: row.note,
            logo_asset_id: row.logo_asset_id,
            include_in_net_worth: row.include_in_net_worth,
            include_in_investment: row.include_in_investment,
            include_in_liquid_assets: row.include_in_liquid_assets,
            opened_on: row.opened_on,
            closed_on: row.closed_on,
            sort_order: row.sort_order,
            created_at: row.created_at,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
        }
    }

    pub fn update(&mut self, input: NewAccount, now: Timestamp) -> Result<(), AppError> {
        input
            .secondary_category
            .require_primary(input.primary_category)?;
        let tracking_mode = match input.tracking_mode {
            Some(mode) if mode != self.tracking_mode => {
                return Err(AppError::validation(
                    "trackingMode",
                    "Tracking mode cannot be changed after an account is created.",
                ));
            }
            Some(mode) => mode,
            None => self.tracking_mode,
        };
        tracking_mode.require_for_new_account(input.primary_category)?;
        if let (Some(opened_on), Some(closed_on)) = (input.opened_on, input.closed_on) {
            if closed_on < opened_on {
                return Err(AppError::validation(
                    "closedOn",
                    "Closed date cannot be earlier than opened date.",
                ));
            }
        }
        self.institution_id = input.institution_id;
        self.group_id = input.group_id;
        self.name = parse_name(&input.name)?;
        self.primary_category = input.primary_category;
        self.secondary_category = input.secondary_category;
        self.note = parse_optional_note(input.note.as_deref())?;
        self.include_in_net_worth = input.include_in_net_worth;
        self.include_in_investment = input.include_in_investment;
        self.include_in_liquid_assets = input.include_in_liquid_assets;
        self.opened_on = input.opened_on;
        self.closed_on = input.closed_on;
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

    pub fn net_worth_contribution(&self, value: Money) -> Result<Decimal, AppError> {
        if value.currency() != self.default_currency {
            return Err(AppError::invalid_money(
                "Account value currency must match the account currency.",
            ));
        }
        if !self.include_in_net_worth {
            return Ok(Decimal::ZERO);
        }
        Ok(self.primary_category.signed_amount(value))
    }

    #[must_use]
    pub fn id(&self) -> AccountId {
        self.id
    }

    #[must_use]
    pub fn household_id(&self) -> HouseholdId {
        self.household_id
    }

    #[must_use]
    pub fn institution_id(&self) -> Option<InstitutionId> {
        self.institution_id
    }

    #[must_use]
    pub fn group_id(&self) -> Option<AccountGroupId> {
        self.group_id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn primary_category(&self) -> PrimaryCategory {
        self.primary_category
    }

    #[must_use]
    pub fn secondary_category(&self) -> SecondaryCategory {
        self.secondary_category
    }

    #[must_use]
    pub fn tracking_mode(&self) -> TrackingMode {
        self.tracking_mode
    }

    #[must_use]
    pub fn default_currency(&self) -> CurrencyCode {
        self.default_currency
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
    pub fn include_in_net_worth(&self) -> bool {
        self.include_in_net_worth
    }

    #[must_use]
    pub fn include_in_investment(&self) -> bool {
        self.include_in_investment
    }

    #[must_use]
    pub fn include_in_liquid_assets(&self) -> bool {
        self.include_in_liquid_assets
    }

    #[must_use]
    pub fn opened_on(&self) -> Option<CalendarDate> {
        self.opened_on
    }

    #[must_use]
    pub fn closed_on(&self) -> Option<CalendarDate> {
        self.closed_on
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedAccountValue {
    pub id: AccountValueId,
    pub account_id: AccountId,
    pub value_kind: ValueKind,
    pub money: Money,
    pub effective_at: Timestamp,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountValue {
    id: AccountValueId,
    account_id: AccountId,
    value_kind: ValueKind,
    money: Money,
    effective_at: Timestamp,
    created_at: Timestamp,
}

impl AccountValue {
    pub fn initial(
        account_id: AccountId,
        tracking_mode: TrackingMode,
        money: Money,
        now: Timestamp,
    ) -> Result<Self, AppError> {
        Ok(Self {
            id: AccountValueId::new(),
            account_id,
            value_kind: ValueKind::from_tracking_mode(tracking_mode)?,
            money,
            effective_at: now.clone(),
            created_at: now,
        })
    }

    #[must_use]
    pub fn from_persisted(row: PersistedAccountValue) -> Self {
        Self {
            id: row.id,
            account_id: row.account_id,
            value_kind: row.value_kind,
            money: row.money,
            effective_at: row.effective_at,
            created_at: row.created_at,
        }
    }

    #[must_use]
    pub fn id(&self) -> AccountValueId {
        self.id
    }

    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    #[must_use]
    pub fn value_kind(&self) -> ValueKind {
        self.value_kind
    }

    #[must_use]
    pub fn money(&self) -> Money {
        self.money
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
    use super::{Account, AccountValue, NewAccount, ValueKind};
    use crate::domain::category::{PrimaryCategory, SecondaryCategory, TrackingMode};
    use crate::domain::currency::CurrencyCode;
    use crate::domain::ids::{AccountId, HouseholdId};
    use crate::domain::money::Money;
    use crate::domain::time::{CalendarDate, Timestamp};
    use crate::error::AppError;

    fn bank_account() -> NewAccount {
        NewAccount::required(
            HouseholdId::new(),
            "DBS Savings",
            PrimaryCategory::CashEquivalent,
            SecondaryCategory::BankAccount,
            CurrencyCode::CNY,
        )
    }

    #[test]
    fn creates_account_with_category_default_tracking_mode() {
        let account = Account::new(bank_account(), Timestamp::now()).expect("valid account");
        assert_eq!(account.tracking_mode(), TrackingMode::Balance);
        assert_eq!(account.primary_category(), PrimaryCategory::CashEquivalent);
        assert!(account.include_in_net_worth());
    }

    #[test]
    fn rejects_mismatched_category_and_tracking_policy() {
        let mut input = bank_account();
        input.secondary_category = SecondaryCategory::Mortgage;
        assert!(matches!(
            Account::new(input, Timestamp::now()),
            Err(AppError::InvalidCategory { .. })
        ));

        let mut input = bank_account();
        input.tracking_mode = Some(TrackingMode::Holdings);
        assert!(matches!(
            Account::new(input, Timestamp::now()),
            Err(AppError::InvalidCategory { .. })
        ));
    }

    #[test]
    fn liability_value_stays_positive_and_contribution_is_negative() {
        let mut input = NewAccount::required(
            HouseholdId::new(),
            "Mortgage",
            PrimaryCategory::Liability,
            SecondaryCategory::Mortgage,
            CurrencyCode::CNY,
        );
        input.opened_on = Some(CalendarDate::parse("2020-01-01").expect("opened"));
        input.closed_on = Some(CalendarDate::parse("2026-08-17").expect("closed"));
        let account = Account::new(input, Timestamp::now()).expect("liability");
        let value = Money::parse("10000", CurrencyCode::CNY).expect("amount");
        assert_eq!(
            account
                .net_worth_contribution(value)
                .expect("same currency")
                .to_string(),
            "-10000"
        );
    }

    #[test]
    fn excluded_accounts_contribute_zero() {
        let mut input = bank_account();
        input.include_in_net_worth = false;
        let account = Account::new(input, Timestamp::now()).expect("excluded");
        let value = Money::parse("10000", CurrencyCode::CNY).expect("amount");
        assert_eq!(
            account
                .net_worth_contribution(value)
                .expect("same currency")
                .to_string(),
            "0"
        );
    }

    #[test]
    fn rejects_closed_date_before_opened_date() {
        let mut input = bank_account();
        input.opened_on = Some(CalendarDate::parse("2026-08-17").expect("opened"));
        input.closed_on = Some(CalendarDate::parse("2026-01-01").expect("closed"));
        assert!(Account::new(input, Timestamp::now()).is_err());
    }

    #[test]
    fn keeps_tracking_mode_on_update_when_unchanged() {
        let mut account = Account::new(bank_account(), Timestamp::now()).expect("account");
        let mut input = bank_account();
        input.name = "DBS Joint".to_owned();
        input.tracking_mode = Some(TrackingMode::Balance);
        account.update(input, Timestamp::now()).expect("same mode");
        assert_eq!(account.name(), "DBS Joint");
        assert_eq!(account.tracking_mode(), TrackingMode::Balance);
    }

    #[test]
    fn rejects_tracking_mode_change_after_create() {
        let mut account = Account::new(bank_account(), Timestamp::now()).expect("account");
        let mut input = bank_account();
        input.tracking_mode = Some(TrackingMode::ManualValue);
        let error = account.update(input, Timestamp::now()).expect_err("locked");
        assert!(matches!(error, AppError::Validation { field, .. } if field == "trackingMode"));
        assert_eq!(account.tracking_mode(), TrackingMode::Balance);
    }

    #[test]
    fn rejects_category_that_does_not_match_existing_tracking_mode() {
        let mut account = Account::new(bank_account(), Timestamp::now()).expect("account");
        let mut input = bank_account();
        input.primary_category = PrimaryCategory::Investment;
        input.secondary_category = SecondaryCategory::BrokerageAccount;
        input.tracking_mode = None;
        let error = account.update(input, Timestamp::now()).expect_err("policy");
        assert!(matches!(error, AppError::InvalidCategory { .. }));
        assert_eq!(account.primary_category(), PrimaryCategory::CashEquivalent);
    }

    #[test]
    fn initial_value_uses_tracking_mode_kind() {
        let value = AccountValue::initial(
            AccountId::new(),
            TrackingMode::ManualValue,
            Money::parse("4000000", CurrencyCode::CNY).expect("amount"),
            Timestamp::now(),
        )
        .expect("initial value");
        assert_eq!(value.value_kind(), ValueKind::ManualValue);
        assert!(AccountValue::initial(
            AccountId::new(),
            TrackingMode::Holdings,
            Money::parse("1", CurrencyCode::CNY).expect("amount"),
            Timestamp::now(),
        )
        .is_err());
    }
}
