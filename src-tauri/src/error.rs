use std::collections::HashMap;

use serde::Serialize;
use specta::Type;
use thiserror::Error;

use crate::infrastructure::database_bootstrap::DatabaseBootstrapStatus;

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
    #[error("database is unavailable")]
    DatabaseUnavailable,
    #[error("database migration failed")]
    MigrationFailed,
    #[error("database version {found} is newer than supported version {supported}")]
    UnsupportedNewerDatabase { found: i64, supported: i64 },
    #[error("database integrity check failed")]
    CorruptDatabase,
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

    pub fn invalid_money(message: &str) -> Self {
        Self::InvalidMoney {
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

#[derive(Debug, Clone, Serialize, Type)]
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
    MediaInvalid,
    DatabaseError,
    DatabaseUnavailable,
    UnsupportedNewerDatabase,
    MigrationFailed,
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
