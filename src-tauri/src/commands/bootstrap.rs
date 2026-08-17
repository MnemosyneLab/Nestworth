use serde::Serialize;
use specta::Type;
use sqlx::Row;

use crate::{
    error::{AppError, CommandError},
    infrastructure::database_bootstrap::DatabaseBootstrapStatus,
    state::AppState,
};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsDto {
    pub language: String,
    pub appearance: String,
    pub last_household_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HouseholdDto {
    pub id: String,
    pub name: String,
    pub base_currency: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MemberDto {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum BootstrapDto {
    Ready {
        #[serde(rename = "onboardingRequired")]
        onboarding_required: bool,
        settings: AppSettingsDto,
        household: Option<HouseholdDto>,
        members: Vec<MemberDto>,
    },
    Blocked {
        error: CommandError,
        #[serde(rename = "databasePath")]
        database_path: String,
        #[serde(rename = "foundMigration")]
        found_migration: Option<i32>,
        #[serde(rename = "supportedMigration")]
        supported_migration: Option<i32>,
    },
}
pub async fn bootstrap_impl(state: &AppState) -> Result<BootstrapDto, CommandError> {
    if !state.is_writable() {
        return Ok(blocked_dto(state));
    }

    load_ready_dto(state).await.map_err(CommandError::from)
}

fn blocked_dto(state: &AppState) -> BootstrapDto {
    let (found_migration, supported_migration) = match state.bootstrap_status() {
        DatabaseBootstrapStatus::UnsupportedNewerDatabase { found, supported } => (
            Some(migration_number(*found)),
            Some(migration_number(*supported)),
        ),
        _ => (None, None),
    };

    BootstrapDto::Blocked {
        error: AppError::from_bootstrap_status(state.bootstrap_status()).into(),
        database_path: state.database_path().display().to_string(),
        found_migration,
        supported_migration,
    }
}

fn migration_number(value: i64) -> i32 {
    value.try_into().unwrap_or(i32::MAX)
}

async fn load_ready_dto(state: &AppState) -> Result<BootstrapDto, AppError> {
    let database = state.writable_db()?;
    let settings_row = sqlx::query(
        "SELECT language, appearance, last_household_id FROM app_settings WHERE id = 1",
    )
    .fetch_one(database)
    .await
    .map_err(|error| {
        tracing::error!(event = "database.open", error = ?error, "failed to load application settings");
        AppError::from(error)
    })?;

    let settings = AppSettingsDto {
        language: settings_row.try_get("language").map_err(|error| {
            tracing::error!(event = "database.open", error = ?error, "invalid application settings row");
            AppError::DatabaseUnavailable
        })?,
        appearance: settings_row.try_get("appearance").map_err(|error| {
            tracing::error!(event = "database.open", error = ?error, "invalid application settings row");
            AppError::DatabaseUnavailable
        })?,
        last_household_id: settings_row.try_get("last_household_id").map_err(|error| {
            tracing::error!(event = "database.open", error = ?error, "invalid application settings row");
            AppError::DatabaseUnavailable
        })?,
    };

    let household_row = sqlx::query(
        "SELECT id, name, base_currency FROM households ORDER BY created_at, id LIMIT 1",
    )
    .fetch_optional(database)
    .await
    .map_err(|error| {
        tracing::error!(event = "database.open", error = ?error, "failed to load household bootstrap state");
        AppError::from(error)
    })?;

    let household = household_row
        .map(|row| {
            Ok::<_, AppError>(HouseholdDto {
                id: row
                    .try_get("id")
                    .map_err(|_| AppError::DatabaseUnavailable)?,
                name: row
                    .try_get("name")
                    .map_err(|_| AppError::DatabaseUnavailable)?,
                base_currency: row
                    .try_get("base_currency")
                    .map_err(|_| AppError::DatabaseUnavailable)?,
            })
        })
        .transpose()?;

    let members = if let Some(household) = &household {
        sqlx::query("SELECT id, name FROM members WHERE household_id = ? ORDER BY sort_order, created_at, id")
            .bind(&household.id)
            .fetch_all(database)
            .await
            .map_err(|error| {
                tracing::error!(event = "database.open", error = ?error, "failed to load bootstrap members");
                AppError::from(error)
            })?
            .into_iter()
            .map(|row| {
                Ok::<_, AppError>(MemberDto {
                    id: row.try_get("id").map_err(|_| AppError::DatabaseUnavailable)?,
                    name: row.try_get("name").map_err(|_| AppError::DatabaseUnavailable)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };

    Ok(BootstrapDto::Ready {
        onboarding_required: household.is_none(),
        settings,
        household,
        members,
    })
}
