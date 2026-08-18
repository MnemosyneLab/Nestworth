use std::{fs, path::Path};

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::Row;

use super::reference::{begin_write_tx, finish_write_tx, map_read_error, map_write_error};
use crate::{domain::Timestamp, error::AppError, state::AppState};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsDto {
    pub language: String,
    pub appearance: String,
    pub last_household_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsInput {
    pub language: String,
    pub appearance: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAllDataInput {
    pub confirmed: bool,
}

pub async fn get_settings(state: &AppState) -> Result<AppSettingsDto, AppError> {
    let database = state.writable_db()?;
    load_settings(database).await
}

pub async fn update_settings(
    state: &AppState,
    input: UpdateSettingsInput,
) -> Result<AppSettingsDto, AppError> {
    let language = parse_language(&input.language)?;
    let appearance = parse_appearance(&input.appearance)?;
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = update_settings_in_tx(&mut tx, language, appearance).await;
    finish_write_tx(tx, result).await
}

pub async fn delete_all_data(state: &AppState, input: DeleteAllDataInput) -> Result<(), AppError> {
    if !input.confirmed {
        return Err(AppError::validation(
            "confirmed",
            "Deleting all data requires explicit confirmation.",
        ));
    }

    let database = state.writable_db()?.clone();
    let database_path = state.database_path().to_path_buf();
    database.close().await;

    remove_pre_migration_snapshots(&database_path)?;
    for suffix in ["-wal", "-shm"] {
        remove_if_present(&sidecar_path(&database_path, suffix))?;
    }
    remove_if_present(&database_path)?;

    tracing::info!(event = "data.reset", "all application data deleted");
    Ok(())
}

fn remove_pre_migration_snapshots(database_path: &Path) -> Result<(), AppError> {
    let Some(parent) = database_path.parent() else {
        return Err(AppError::DataResetFailed);
    };
    let Some(database_name) = database_path.file_name().and_then(|name| name.to_str()) else {
        return Err(AppError::DataResetFailed);
    };
    let prefix = format!("{database_name}.pre-migrate-");
    let entries = fs::read_dir(parent).map_err(|_error| AppError::DataResetFailed)?;
    for entry in entries {
        let entry = entry.map_err(|_error| AppError::DataResetFailed)?;
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            remove_if_present(&entry.path())?;
        }
    }
    Ok(())
}

fn sidecar_path(database_path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut path = database_path.as_os_str().to_os_string();
    path.push(suffix);
    path.into()
}

fn remove_if_present(path: &Path) -> Result<(), AppError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_error) => {
            tracing::error!(event = "data.reset", "failed to delete application data");
            Err(AppError::DataResetFailed)
        }
    }
}

async fn update_settings_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    language: &str,
    appearance: &str,
) -> Result<AppSettingsDto, AppError> {
    let updated = sqlx::query(
        "UPDATE app_settings SET language = ?, appearance = ?, updated_at = ? WHERE id = 1",
    )
    .bind(language)
    .bind(appearance)
    .bind(Timestamp::now().to_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("settings.update_failed", error))?;
    if updated.rows_affected() != 1 {
        return Err(AppError::Internal);
    }
    tracing::info!(event = "settings.update", "application settings updated");
    load_settings_in_tx(tx).await
}

pub async fn load_settings(database: &sqlx::SqlitePool) -> Result<AppSettingsDto, AppError> {
    let row = sqlx::query(
        "SELECT language, appearance, last_household_id FROM app_settings WHERE id = 1",
    )
    .fetch_one(database)
    .await
    .map_err(|error| map_read_error("settings.load_failed", error))?;
    settings_from_row(row)
}

async fn load_settings_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<AppSettingsDto, AppError> {
    let row = sqlx::query(
        "SELECT language, appearance, last_household_id FROM app_settings WHERE id = 1",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| map_read_error("settings.load_failed", error))?;
    settings_from_row(row)
}

fn settings_from_row(row: sqlx::sqlite::SqliteRow) -> Result<AppSettingsDto, AppError> {
    Ok(AppSettingsDto {
        language: row
            .try_get("language")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        appearance: row
            .try_get("appearance")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        last_household_id: row
            .try_get("last_household_id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    })
}

fn parse_language(value: &str) -> Result<&str, AppError> {
    match value {
        "system" | "en" | "zh-CN" => Ok(value),
        _ => Err(AppError::validation(
            "language",
            "Language must be system, en, or zh-CN.",
        )),
    }
}

fn parse_appearance(value: &str) -> Result<&str, AppError> {
    match value {
        "system" | "light" | "dark" => Ok(value),
        _ => Err(AppError::validation(
            "appearance",
            "Appearance must be system, light, or dark.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        delete_all_data, get_settings, update_settings, DeleteAllDataInput, UpdateSettingsInput,
    };
    use crate::{
        error::AppError,
        infrastructure::database_bootstrap::pre_migration_snapshot_path,
        test_support::{blocked_future_state, cleanup, file_hash, initialize_state, test_path},
    };

    #[test]
    fn updates_language_and_appearance_without_touching_household() {
        tauri::async_runtime::block_on(async {
            let path = test_path("phase9", "settings-update");
            let state = initialize_state(path.clone()).await;
            let current = get_settings(&state).await.expect("settings");
            assert_eq!(current.language, "system");
            assert_eq!(current.appearance, "system");
            let updated = update_settings(
                &state,
                UpdateSettingsInput {
                    language: "zh-CN".to_owned(),
                    appearance: "dark".to_owned(),
                },
            )
            .await
            .expect("update");
            assert_eq!(updated.language, "zh-CN");
            assert_eq!(updated.appearance, "dark");
            assert_eq!(updated.last_household_id, current.last_household_id);
            let reloaded = get_settings(&state).await.expect("reload");
            assert_eq!(reloaded, updated);
            cleanup(&path);
        });
    }

    #[test]
    fn invalid_settings_write_nothing() {
        tauri::async_runtime::block_on(async {
            let path = test_path("phase9", "settings-invalid");
            let state = initialize_state(path.clone()).await;
            let before = get_settings(&state).await.expect("settings");
            let error = update_settings(
                &state,
                UpdateSettingsInput {
                    language: "fr".to_owned(),
                    appearance: "dark".to_owned(),
                },
            )
            .await
            .expect_err("invalid language");
            assert!(matches!(error, AppError::Validation { field, .. } if field == "language"));
            let error = update_settings(
                &state,
                UpdateSettingsInput {
                    language: "en".to_owned(),
                    appearance: "neon".to_owned(),
                },
            )
            .await
            .expect_err("invalid appearance");
            assert!(matches!(error, AppError::Validation { field, .. } if field == "appearance"));
            assert_eq!(get_settings(&state).await.expect("unchanged"), before);
            cleanup(&path);
        });
    }

    #[test]
    fn blocked_future_database_rejects_settings_writes() {
        tauri::async_runtime::block_on(async {
            let (state, path, before_hash) = blocked_future_state("settings").await;
            let error = get_settings(&state).await.expect_err("blocked get");
            assert!(matches!(error, AppError::UnsupportedNewerDatabase { .. }));
            let error = update_settings(
                &state,
                UpdateSettingsInput {
                    language: "en".to_owned(),
                    appearance: "light".to_owned(),
                },
            )
            .await
            .expect_err("blocked update");
            assert!(matches!(error, AppError::UnsupportedNewerDatabase { .. }));
            let error = delete_all_data(&state, DeleteAllDataInput { confirmed: true })
                .await
                .expect_err("blocked delete");
            assert!(matches!(error, AppError::UnsupportedNewerDatabase { .. }));
            assert_eq!(file_hash(&path), before_hash);
            cleanup(&path);
        });
    }

    #[test]
    fn delete_all_data_requires_confirmation() {
        tauri::async_runtime::block_on(async {
            let path = test_path("settings", "delete-unconfirmed");
            let state = initialize_state(path.clone()).await;
            let error = delete_all_data(&state, DeleteAllDataInput { confirmed: false })
                .await
                .expect_err("confirmation should be required");
            assert!(matches!(error, AppError::Validation { field, .. } if field == "confirmed"));
            assert!(path.exists());
            cleanup(&path);
        });
    }

    #[test]
    fn delete_all_data_removes_database_and_sidecars() {
        tauri::async_runtime::block_on(async {
            let path = test_path("settings", "delete-confirmed");
            let state = initialize_state(path.clone()).await;
            let snapshot = pre_migration_snapshot_path(&path, 1);
            std::fs::write(&snapshot, b"test snapshot").expect("snapshot fixture");
            assert!(path.exists());

            delete_all_data(&state, DeleteAllDataInput { confirmed: true })
                .await
                .expect("data should be deleted");

            assert!(!path.exists());
            assert!(!snapshot.exists());
            assert!(!super::sidecar_path(&path, "-wal").exists());
            assert!(!super::sidecar_path(&path, "-shm").exists());
        });
    }
}
