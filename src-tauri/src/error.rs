use std::collections::HashMap;

use serde::Serialize;
use specta::Type;
use thiserror::Error;

use crate::infrastructure::database_bootstrap::DatabaseBootstrapStatus;

pub const LAST_ACTIVE_MEMBER_MESSAGE: &str = "A household must keep at least one active member.";

#[derive(Debug, Clone, Error)]
pub enum AppError {
    #[error("validation failed for {field}")]
    Validation { field: String, message: String },
    #[error("ownership total is invalid")]
    OwnershipTotalInvalid { actual_bps: i32 },
    #[error("invalid category")]
    InvalidCategory { message: String },
    #[error("invalid money")]
    InvalidMoney { message: String },
    #[error("invalid quantity")]
    InvalidQuantity { message: String },
    #[error("invalid unit price")]
    InvalidUnitPrice { message: String },
    #[error("invalid FX rate")]
    InvalidFxRate { message: String },
    #[error("decimal overflow")]
    DecimalOverflow,
    #[error("the activity is invalid")]
    InvalidActivity { message: String },
    #[error("the activity time is invalid")]
    InvalidActivityTime { message: String },
    #[error("the activity legs are invalid")]
    InvalidActivityLegs { message: String },
    #[error("insufficient balance")]
    InsufficientBalance,
    #[error("insufficient quantity")]
    InsufficientQuantity,
    #[error("transfer amounts do not match")]
    TransferMismatch { message: String },
    #[error("trade totals do not match")]
    TradeTotalMismatch { message: String },
    #[error("the activity has already been reversed")]
    ActivityAlreadyReversed,
    #[error("the activity cannot be reversed or corrected")]
    ActivityNotCorrectable { message: String },
    #[error("the selected image is invalid")]
    MediaInvalid { message: String },
    #[error("household is already onboarded")]
    AlreadyOnboarded,
    #[error("{entity} was not found")]
    NotFound { entity: String, id: String },
    #[error("{message}")]
    Conflict { message: String },
    #[error("the holding already exists")]
    DuplicateHolding,
    #[error("required quote is unavailable")]
    QuoteUnavailable { message: String },
    #[error("provider authentication failed")]
    ProviderAuthentication,
    #[error("provider rate limit reached")]
    ProviderRateLimit,
    #[error("provider is unavailable")]
    ProviderUnavailable { message: String },
    #[error("provider response is malformed")]
    MalformedProviderResponse { message: String },
    #[error("provider symbol is unsupported")]
    UnsupportedProviderSymbol { message: String },
    #[error("database is unavailable")]
    DatabaseUnavailable,
    #[error("database migration failed")]
    MigrationFailed,
    #[error("database version {found} is newer than supported version {supported}")]
    UnsupportedNewerDatabase { found: i64, supported: i64 },
    #[error("database integrity check failed")]
    CorruptDatabase,
    #[error("all application data could not be deleted")]
    DataResetFailed,
    #[error("history origin initialization failed")]
    HistoryInitializationFailed,
    #[error("history timezone confirmation is required")]
    HistoryTimezoneConfirmationRequired,
    #[error("history snapshots need to be rebuilt")]
    SnapshotRebuildRequired,
    #[error("history snapshots could not be rebuilt")]
    SnapshotRebuildFailed,
    #[error("the cost-basis declaration is invalid")]
    InvalidCostBasisDeclaration { message: String },
    #[error("the cost-basis lot was not found")]
    CostBasisLotNotFound,
    #[error("the analytics period is unavailable")]
    AnalyticsPeriodUnavailable {
        reason: String,
        blocking_dates: Vec<String>,
    },
    #[error("the return is not computable")]
    ReturnNotComputable { reason: String },
    #[error("internal application error")]
    Internal,
}

impl AppError {
    pub fn validation(field: &str, message: &str) -> Self {
        Self::Validation {
            field: field.to_owned(),
            message: message.to_owned(),
        }
    }

    pub fn invalid_category(message: &str) -> Self {
        Self::InvalidCategory {
            message: message.to_owned(),
        }
    }

    pub fn not_found(entity: &str, id: &str) -> Self {
        Self::NotFound {
            entity: entity.to_owned(),
            id: id.to_owned(),
        }
    }

    pub fn conflict(message: &str) -> Self {
        Self::Conflict {
            message: message.to_owned(),
        }
    }

    pub fn last_active_member() -> Self {
        Self::Conflict {
            message: LAST_ACTIVE_MEMBER_MESSAGE.to_owned(),
        }
    }

    pub fn invalid_money(message: &str) -> Self {
        Self::InvalidMoney {
            message: message.to_owned(),
        }
    }

    pub fn invalid_quantity(message: &str) -> Self {
        Self::InvalidQuantity {
            message: message.to_owned(),
        }
    }

    pub fn invalid_unit_price(message: &str) -> Self {
        Self::InvalidUnitPrice {
            message: message.to_owned(),
        }
    }

    pub fn invalid_fx_rate(message: &str) -> Self {
        Self::InvalidFxRate {
            message: message.to_owned(),
        }
    }

    pub fn invalid_activity(message: &str) -> Self {
        Self::InvalidActivity {
            message: message.to_owned(),
        }
    }

    pub fn invalid_activity_time(message: &str) -> Self {
        Self::InvalidActivityTime {
            message: message.to_owned(),
        }
    }

    pub fn invalid_activity_legs(message: &str) -> Self {
        Self::InvalidActivityLegs {
            message: message.to_owned(),
        }
    }

    pub fn transfer_mismatch(message: &str) -> Self {
        Self::TransferMismatch {
            message: message.to_owned(),
        }
    }

    pub fn trade_total_mismatch(message: &str) -> Self {
        Self::TradeTotalMismatch {
            message: message.to_owned(),
        }
    }

    pub fn activity_not_correctable(message: &str) -> Self {
        Self::ActivityNotCorrectable {
            message: message.to_owned(),
        }
    }

    pub fn invalid_cost_basis_declaration(message: &str) -> Self {
        Self::InvalidCostBasisDeclaration {
            message: message.to_owned(),
        }
    }

    pub fn media_invalid(message: &str) -> Self {
        Self::MediaInvalid {
            message: message.to_owned(),
        }
    }

    pub fn from_bootstrap_status(status: &DatabaseBootstrapStatus) -> Self {
        match status {
            DatabaseBootstrapStatus::Ready | DatabaseBootstrapStatus::Migrated => Self::Internal,
            DatabaseBootstrapStatus::UnsupportedNewerDatabase { found, supported } => {
                Self::UnsupportedNewerDatabase {
                    found: *found,
                    supported: *supported,
                }
            }
            DatabaseBootstrapStatus::MigrationFailed => Self::MigrationFailed,
            DatabaseBootstrapStatus::HistoryInitializationFailed => {
                Self::HistoryInitializationFailed
            }
            DatabaseBootstrapStatus::Unavailable => Self::DatabaseUnavailable,
            DatabaseBootstrapStatus::Corrupt => Self::CorruptDatabase,
        }
    }

    pub fn into_command_error(self) -> CommandError {
        match self {
            Self::Validation { field, message } => {
                let mut fields = HashMap::new();
                fields.insert(field, message.clone());
                CommandError {
                    code: ErrorCode::ValidationError,
                    message,
                    fields: Some(fields),
                }
            }
            Self::OwnershipTotalInvalid { actual_bps } => {
                let mut fields = HashMap::new();
                fields.insert("actualBps".to_owned(), actual_bps.to_string());
                fields.insert("expectedBps".to_owned(), "10000".to_owned());
                CommandError {
                    code: ErrorCode::OwnershipTotalInvalid,
                    message: "Ownership shares must total 10000 basis points.".to_owned(),
                    fields: Some(fields),
                }
            }
            Self::InvalidCategory { message } => {
                let mut fields = HashMap::new();
                fields.insert("category".to_owned(), message);
                CommandError {
                    code: ErrorCode::InvalidCategory,
                    message: "The account category is invalid.".to_owned(),
                    fields: Some(fields),
                }
            }
            Self::InvalidMoney { message } => {
                let mut fields = HashMap::new();
                fields.insert("amount".to_owned(), message);
                CommandError {
                    code: ErrorCode::InvalidMoney,
                    message: "The amount is invalid.".to_owned(),
                    fields: Some(fields),
                }
            }
            Self::InvalidQuantity { message } => {
                let mut fields = HashMap::new();
                fields.insert("quantity".to_owned(), message);
                CommandError {
                    code: ErrorCode::InvalidQuantity,
                    message: "The quantity is invalid.".to_owned(),
                    fields: Some(fields),
                }
            }
            Self::InvalidUnitPrice { message } => {
                let mut fields = HashMap::new();
                fields.insert("unitPrice".to_owned(), message);
                CommandError {
                    code: ErrorCode::InvalidUnitPrice,
                    message: "The unit price is invalid.".to_owned(),
                    fields: Some(fields),
                }
            }
            Self::InvalidFxRate { message } => {
                let mut fields = HashMap::new();
                fields.insert("rate".to_owned(), message);
                CommandError {
                    code: ErrorCode::InvalidFxRate,
                    message: "The FX rate is invalid.".to_owned(),
                    fields: Some(fields),
                }
            }
            Self::DecimalOverflow => CommandError::new(
                ErrorCode::DecimalOverflow,
                "The calculated amount is too large to store.",
            ),
            Self::InvalidActivity { message } => CommandError {
                code: ErrorCode::InvalidActivity,
                message,
                fields: None,
            },
            Self::InvalidActivityTime { message } => CommandError {
                code: ErrorCode::InvalidActivityTime,
                message,
                fields: None,
            },
            Self::InvalidActivityLegs { message } => CommandError {
                code: ErrorCode::InvalidActivityLegs,
                message,
                fields: None,
            },
            Self::InsufficientBalance => CommandError::new(
                ErrorCode::InsufficientBalance,
                "This activity would make the balance negative.",
            ),
            Self::InsufficientQuantity => CommandError::new(
                ErrorCode::InsufficientQuantity,
                "This activity would make the holding quantity negative.",
            ),
            Self::TransferMismatch { message } => CommandError {
                code: ErrorCode::TransferMismatch,
                message,
                fields: None,
            },
            Self::TradeTotalMismatch { message } => CommandError {
                code: ErrorCode::TradeTotalMismatch,
                message,
                fields: None,
            },
            Self::ActivityAlreadyReversed => CommandError::new(
                ErrorCode::ActivityAlreadyReversed,
                "This activity has already been reversed.",
            ),
            Self::ActivityNotCorrectable { message } => CommandError {
                code: ErrorCode::ActivityNotCorrectable,
                message,
                fields: None,
            },
            Self::MediaInvalid { message } => {
                let mut fields = HashMap::new();
                fields.insert("image".to_owned(), message.clone());
                CommandError {
                    code: ErrorCode::MediaInvalid,
                    message: "The selected image is invalid.".to_owned(),
                    fields: Some(fields),
                }
            }
            Self::AlreadyOnboarded => CommandError::new(
                ErrorCode::AlreadyOnboarded,
                "This household has already been set up.",
            ),
            Self::NotFound { entity, id } => {
                let mut fields = HashMap::new();
                fields.insert("entity".to_owned(), entity.clone());
                fields.insert("id".to_owned(), id);
                CommandError {
                    code: ErrorCode::NotFound,
                    message: format!("The {entity} could not be found."),
                    fields: Some(fields),
                }
            }
            Self::Conflict { message } => {
                let fields = (message == LAST_ACTIVE_MEMBER_MESSAGE).then(|| {
                    let mut fields = HashMap::new();
                    fields.insert("reason".to_owned(), "lastActiveMember".to_owned());
                    fields
                });
                CommandError {
                    code: ErrorCode::Conflict,
                    message,
                    fields,
                }
            }
            Self::DuplicateHolding => CommandError::new(
                ErrorCode::DuplicateHolding,
                "This instrument is already held in the account.",
            ),
            Self::QuoteUnavailable { message } => CommandError {
                code: ErrorCode::QuoteUnavailable,
                message,
                fields: None,
            },
            Self::ProviderAuthentication => CommandError::new(
                ErrorCode::ProviderAuthentication,
                "The quote provider rejected the stored credentials.",
            ),
            Self::ProviderRateLimit => CommandError::new(
                ErrorCode::ProviderRateLimit,
                "The quote provider rate limit was reached.",
            ),
            Self::ProviderUnavailable { message } => CommandError {
                code: ErrorCode::ProviderUnavailable,
                message,
                fields: None,
            },
            Self::MalformedProviderResponse { message } => CommandError {
                code: ErrorCode::MalformedProviderResponse,
                message,
                fields: None,
            },
            Self::UnsupportedProviderSymbol { message } => CommandError {
                code: ErrorCode::UnsupportedProviderSymbol,
                message,
                fields: None,
            },
            Self::DatabaseUnavailable => CommandError::new(
                ErrorCode::DatabaseUnavailable,
                "The database is unavailable.",
            ),
            Self::MigrationFailed => {
                CommandError::new(ErrorCode::MigrationFailed, "Database migration failed.")
            }
            Self::UnsupportedNewerDatabase { found, supported } => {
                let mut fields = HashMap::new();
                fields.insert("foundMigration".to_owned(), found.to_string());
                fields.insert("supportedMigration".to_owned(), supported.to_string());
                CommandError {
                    code: ErrorCode::UnsupportedNewerDatabase,
                    message: "This database was created by a newer version of Nestworth."
                        .to_owned(),
                    fields: Some(fields),
                }
            }
            Self::CorruptDatabase => {
                CommandError::new(ErrorCode::DatabaseError, "The database is corrupt.")
            }
            Self::DataResetFailed => CommandError::new(
                ErrorCode::DataResetFailed,
                "All application data could not be deleted.",
            ),
            Self::HistoryInitializationFailed => CommandError::new(
                ErrorCode::HistoryInitializationFailed,
                "History origin could not be initialized.",
            ),
            Self::HistoryTimezoneConfirmationRequired => CommandError::new(
                ErrorCode::HistoryTimezoneConfirmationRequired,
                "Confirm the history timezone before recording activity or snapshots.",
            ),
            Self::SnapshotRebuildRequired => CommandError::new(
                ErrorCode::SnapshotRebuildRequired,
                "History snapshots need to be rebuilt.",
            ),
            Self::SnapshotRebuildFailed => CommandError::new(
                ErrorCode::SnapshotRebuildFailed,
                "History snapshots could not be rebuilt.",
            ),
            // Phase 7 will add dedicated ErrorCode variants. Map to existing
            // codes so generated bindings stay unchanged until then.
            Self::InvalidCostBasisDeclaration { message } => CommandError {
                code: ErrorCode::ValidationError,
                message,
                fields: None,
            },
            Self::CostBasisLotNotFound => {
                let mut fields = HashMap::new();
                fields.insert("entity".to_owned(), "costBasisLot".to_owned());
                CommandError {
                    code: ErrorCode::NotFound,
                    message: "The cost-basis lot could not be found.".to_owned(),
                    fields: Some(fields),
                }
            }
            Self::AnalyticsPeriodUnavailable { reason, .. } => CommandError {
                code: ErrorCode::Conflict,
                message: reason,
                fields: None,
            },
            Self::ReturnNotComputable { reason } => CommandError {
                code: ErrorCode::ValidationError,
                message: reason,
                fields: None,
            },
            Self::Internal => CommandError::new(
                ErrorCode::InternalError,
                "An internal application error occurred.",
            ),
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(_: sqlx::Error) -> Self {
        Self::DatabaseUnavailable
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ValidationError,
    NotFound,
    Conflict,
    AlreadyOnboarded,
    OwnershipTotalInvalid,
    BaseCurrencyChangeNotAllowed,
    InvalidCategory,
    InvalidMoney,
    InvalidQuantity,
    InvalidUnitPrice,
    InvalidFxRate,
    DecimalOverflow,
    InvalidActivity,
    InvalidActivityTime,
    InvalidActivityLegs,
    InsufficientBalance,
    InsufficientQuantity,
    TransferMismatch,
    TradeTotalMismatch,
    ActivityAlreadyReversed,
    ActivityNotCorrectable,
    QuoteUnavailable,
    IncompleteValuation,
    DuplicateHolding,
    UnsupportedProviderSymbol,
    ProviderAuthentication,
    ProviderRateLimit,
    ProviderUnavailable,
    MalformedProviderResponse,
    MediaInvalid,
    DatabaseError,
    DatabaseUnavailable,
    UnsupportedNewerDatabase,
    MigrationFailed,
    DataResetFailed,
    HistoryInitializationFailed,
    HistoryTimezoneConfirmationRequired,
    SnapshotRebuildRequired,
    SnapshotRebuildFailed,
    InternalError,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct CommandError {
    pub code: ErrorCode,
    pub message: String,
    pub fields: Option<HashMap<String, String>>,
}

impl CommandError {
    pub fn new(code: ErrorCode, message: &str) -> Self {
        Self {
            code,
            message: message.to_owned(),
            fields: None,
        }
    }
}

impl From<AppError> for CommandError {
    fn from(error: AppError) -> Self {
        error.into_command_error()
    }
}
