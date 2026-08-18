use rust_decimal::Decimal;

use super::money::Money;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimaryCategory {
    CashEquivalent,
    Investment,
    Property,
    Receivable,
    Liability,
}

impl PrimaryCategory {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "cash_equivalent" => Ok(Self::CashEquivalent),
            "investment" => Ok(Self::Investment),
            "property" => Ok(Self::Property),
            "receivable" => Ok(Self::Receivable),
            "liability" => Ok(Self::Liability),
            _ => Err(AppError::invalid_category(
                "Primary category is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CashEquivalent => "cash_equivalent",
            Self::Investment => "investment",
            Self::Property => "property",
            Self::Receivable => "receivable",
            Self::Liability => "liability",
        }
    }

    #[must_use]
    pub fn default_tracking_mode(self) -> TrackingMode {
        match self {
            Self::CashEquivalent | Self::Liability => TrackingMode::Balance,
            Self::Investment | Self::Property | Self::Receivable => TrackingMode::ManualValue,
        }
    }

    #[must_use]
    pub fn signed_amount(self, money: Money) -> Decimal {
        if matches!(self, Self::Liability) {
            -money.amount()
        } else {
            money.amount()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecondaryCategory {
    Cash,
    BankAccount,
    DigitalWallet,
    BrokerCash,
    OtherCashEquivalent,
    BrokerageAccount,
    InvestmentFundAccount,
    BankInvestmentProduct,
    Insurance,
    ManualInvestment,
    OtherInvestment,
    RealEstate,
    Vehicle,
    Collectible,
    OtherProperty,
    LoanReceivable,
    OtherReceivable,
    CreditCard,
    Mortgage,
    AutoLoan,
    ConsumerLoan,
    PersonalDebt,
    OtherLiability,
}

impl SecondaryCategory {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "cash" => Ok(Self::Cash),
            "bank_account" => Ok(Self::BankAccount),
            "digital_wallet" => Ok(Self::DigitalWallet),
            "broker_cash" => Ok(Self::BrokerCash),
            "other_cash_equivalent" => Ok(Self::OtherCashEquivalent),
            "brokerage_account" => Ok(Self::BrokerageAccount),
            "investment_fund_account" => Ok(Self::InvestmentFundAccount),
            "bank_investment_product" => Ok(Self::BankInvestmentProduct),
            "insurance" => Ok(Self::Insurance),
            "manual_investment" => Ok(Self::ManualInvestment),
            "other_investment" => Ok(Self::OtherInvestment),
            "real_estate" => Ok(Self::RealEstate),
            "vehicle" => Ok(Self::Vehicle),
            "collectible" => Ok(Self::Collectible),
            "other_property" => Ok(Self::OtherProperty),
            "loan_receivable" => Ok(Self::LoanReceivable),
            "other_receivable" => Ok(Self::OtherReceivable),
            "credit_card" => Ok(Self::CreditCard),
            "mortgage" => Ok(Self::Mortgage),
            "auto_loan" => Ok(Self::AutoLoan),
            "consumer_loan" => Ok(Self::ConsumerLoan),
            "personal_debt" => Ok(Self::PersonalDebt),
            "other_liability" => Ok(Self::OtherLiability),
            _ => Err(AppError::invalid_category(
                "Secondary category is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cash => "cash",
            Self::BankAccount => "bank_account",
            Self::DigitalWallet => "digital_wallet",
            Self::BrokerCash => "broker_cash",
            Self::OtherCashEquivalent => "other_cash_equivalent",
            Self::BrokerageAccount => "brokerage_account",
            Self::InvestmentFundAccount => "investment_fund_account",
            Self::BankInvestmentProduct => "bank_investment_product",
            Self::Insurance => "insurance",
            Self::ManualInvestment => "manual_investment",
            Self::OtherInvestment => "other_investment",
            Self::RealEstate => "real_estate",
            Self::Vehicle => "vehicle",
            Self::Collectible => "collectible",
            Self::OtherProperty => "other_property",
            Self::LoanReceivable => "loan_receivable",
            Self::OtherReceivable => "other_receivable",
            Self::CreditCard => "credit_card",
            Self::Mortgage => "mortgage",
            Self::AutoLoan => "auto_loan",
            Self::ConsumerLoan => "consumer_loan",
            Self::PersonalDebt => "personal_debt",
            Self::OtherLiability => "other_liability",
        }
    }

    #[must_use]
    pub fn primary(self) -> PrimaryCategory {
        match self {
            Self::Cash
            | Self::BankAccount
            | Self::DigitalWallet
            | Self::BrokerCash
            | Self::OtherCashEquivalent => PrimaryCategory::CashEquivalent,
            Self::BrokerageAccount
            | Self::InvestmentFundAccount
            | Self::BankInvestmentProduct
            | Self::Insurance
            | Self::ManualInvestment
            | Self::OtherInvestment => PrimaryCategory::Investment,
            Self::RealEstate | Self::Vehicle | Self::Collectible | Self::OtherProperty => {
                PrimaryCategory::Property
            }
            Self::LoanReceivable | Self::OtherReceivable => PrimaryCategory::Receivable,
            Self::CreditCard
            | Self::Mortgage
            | Self::AutoLoan
            | Self::ConsumerLoan
            | Self::PersonalDebt
            | Self::OtherLiability => PrimaryCategory::Liability,
        }
    }

    pub fn parse_for(primary: PrimaryCategory, value: &str) -> Result<Self, AppError> {
        let secondary = Self::parse(value)?;
        secondary.require_primary(primary)?;
        Ok(secondary)
    }

    pub fn require_primary(self, primary: PrimaryCategory) -> Result<(), AppError> {
        if self.primary() == primary {
            Ok(())
        } else {
            Err(AppError::invalid_category(
                "Secondary category does not belong to the primary category.",
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackingMode {
    Balance,
    ManualValue,
    Holdings,
}

impl TrackingMode {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "balance" => Ok(Self::Balance),
            "manual_value" => Ok(Self::ManualValue),
            "holdings" => Ok(Self::Holdings),
            _ => Err(AppError::invalid_category(
                "Tracking mode is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Balance => "balance",
            Self::ManualValue => "manual_value",
            Self::Holdings => "holdings",
        }
    }

    pub fn require_for_new_account(self, primary: PrimaryCategory) -> Result<(), AppError> {
        let allowed = match self {
            Self::Holdings => primary == PrimaryCategory::Investment,
            _ => self == primary.default_tracking_mode(),
        };
        if allowed {
            Ok(())
        } else {
            Err(AppError::invalid_category(
                "Tracking mode does not match the category policy.",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PrimaryCategory, SecondaryCategory, TrackingMode};
    use crate::domain::currency::CurrencyCode;
    use crate::domain::money::Money;
    use crate::error::AppError;

    #[test]
    fn accepts_valid_primary_and_secondary_pairs() {
        let secondary =
            SecondaryCategory::parse_for(PrimaryCategory::CashEquivalent, "bank_account")
                .expect("valid pair");
        assert_eq!(secondary, SecondaryCategory::BankAccount);
        assert_eq!(secondary.primary(), PrimaryCategory::CashEquivalent);
        assert_eq!(
            PrimaryCategory::Investment.default_tracking_mode(),
            TrackingMode::ManualValue
        );
        assert_eq!(
            PrimaryCategory::Liability.default_tracking_mode(),
            TrackingMode::Balance
        );
    }

    #[test]
    fn rejects_invalid_primary_secondary_pairs() {
        let error = SecondaryCategory::parse_for(PrimaryCategory::Investment, "bank_account")
            .expect_err("mismatched pair");
        assert!(matches!(error, AppError::InvalidCategory { .. }));
        assert!(SecondaryCategory::parse("stock").is_err());
        assert!(PrimaryCategory::parse("asset").is_err());
    }

    #[test]
    fn liability_contribution_is_negative() {
        let value = Money::parse("10000", CurrencyCode::CNY).expect("valid amount");
        assert_eq!(
            PrimaryCategory::Liability.signed_amount(value).to_string(),
            "-10000"
        );
        assert_eq!(
            PrimaryCategory::CashEquivalent
                .signed_amount(value)
                .to_string(),
            "10000"
        );
    }

    #[test]
    fn holdings_is_valid_only_for_investment_accounts() {
        let holdings = TrackingMode::parse("holdings").expect("schema supports holdings");
        assert_eq!(holdings.as_str(), "holdings");
        holdings
            .require_for_new_account(PrimaryCategory::Investment)
            .expect("investment holdings");
        assert!(holdings
            .require_for_new_account(PrimaryCategory::CashEquivalent)
            .is_err());
        TrackingMode::ManualValue
            .require_for_new_account(PrimaryCategory::Investment)
            .expect("investment default");
    }
}
