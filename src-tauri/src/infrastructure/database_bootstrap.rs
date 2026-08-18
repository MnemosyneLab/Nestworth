use std::{
    fs,
    path::{Path, PathBuf},
};

use sqlx::migrate::Migrator;

use super::database::{
    connect_writable, ensure_app_settings, read_migration_version, verify_sqlite_runtime,
    SqlitePool,
};

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseBootstrapStatus {
    Ready,
    Migrated,
    UnsupportedNewerDatabase { found: i64, supported: i64 },
    MigrationFailed,
    Unavailable,
    Corrupt,
}

pub struct DatabaseBootstrapResult {
    pub status: DatabaseBootstrapStatus,
    pub pool: Option<SqlitePool>,
}

pub fn max_supported_migration() -> i64 {
    MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .max()
        .unwrap_or(0)
}

pub async fn initialize_database(path: PathBuf) -> DatabaseBootstrapResult {
    let supported_migration = max_supported_migration();
    if path.as_os_str().is_empty() {
        return blocked(DatabaseBootstrapStatus::Unavailable);
    }

    let database_exists = path.exists();
    let found_migration = if database_exists {
        match read_migration_version(&path).await {
            Ok(version) => version,
            Err(_error) => {
                tracing::error!(
                    event = "database.open",
                    "failed to inspect database metadata"
                );
                return blocked(DatabaseBootstrapStatus::Corrupt);
            }
        }
    } else {
        0
    };

    if found_migration > supported_migration {
        tracing::error!(
            event = "database.open",
            found_migration,
            supported_migration,
            "database version is newer than this application"
        );
        return blocked(DatabaseBootstrapStatus::UnsupportedNewerDatabase {
            found: found_migration,
            supported: supported_migration,
        });
    }

    let migration_required = found_migration < supported_migration;
    if database_exists && migration_required {
        if let Err(_error) = copy_pre_migration_snapshot(&path, found_migration) {
            tracing::error!(
                event = "migration.snapshot_failed",
                "failed to copy pre-migration snapshot"
            );
            return blocked(DatabaseBootstrapStatus::MigrationFailed);
        }
    }

    if !database_exists {
        if let Some(parent) = path.parent() {
            if let Err(_error) = fs::create_dir_all(parent) {
                tracing::error!(
                    event = "database.open",
                    "failed to create database directory"
                );
                return blocked(DatabaseBootstrapStatus::Unavailable);
            }
        }
    }

    let pool = match connect_writable(&path, !database_exists).await {
        Ok(pool) => pool,
        Err(_error) => {
            tracing::error!(event = "database.open", "failed to open database");
            return blocked(DatabaseBootstrapStatus::Unavailable);
        }
    };

    if let Err(_error) = MIGRATOR.run(&pool).await {
        tracing::error!(event = "migration.failed", "database migration failed");
        pool.close().await;
        return blocked(DatabaseBootstrapStatus::MigrationFailed);
    }

    if let Err(_error) = verify_sqlite_runtime(&pool).await {
        tracing::error!(
            event = "database.open",
            "database integrity verification failed"
        );
        pool.close().await;
        return blocked(DatabaseBootstrapStatus::Corrupt);
    }

    if let Err(_error) = ensure_app_settings(&pool).await {
        tracing::error!(
            event = "database.open",
            "failed to initialize application settings"
        );
        pool.close().await;
        return blocked(DatabaseBootstrapStatus::Unavailable);
    }

    tracing::info!(
        event = "migration.complete",
        migration = supported_migration,
        migrated = migration_required,
        "database bootstrap completed"
    );

    DatabaseBootstrapResult {
        status: if migration_required {
            DatabaseBootstrapStatus::Migrated
        } else {
            DatabaseBootstrapStatus::Ready
        },
        pool: Some(pool),
    }
}

fn blocked(status: DatabaseBootstrapStatus) -> DatabaseBootstrapResult {
    DatabaseBootstrapResult { status, pool: None }
}

pub fn pre_migration_snapshot_path(database_path: &Path, found_migration: i64) -> PathBuf {
    let name = database_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("nestworth.sqlite3");
    database_path.with_file_name(format!("{name}.pre-migrate-{found_migration}"))
}

fn sqlite_sidecar(database_path: &Path, suffix: &str) -> PathBuf {
    let mut raw = database_path.as_os_str().to_os_string();
    raw.push(suffix);
    PathBuf::from(raw)
}

fn copy_pre_migration_snapshot(path: &Path, found_migration: i64) -> std::io::Result<()> {
    let snapshot = pre_migration_snapshot_path(path, found_migration);
    if snapshot.exists() {
        if !snapshot.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "pre-migration snapshot path exists and is not a file",
            ));
        }
        tracing::info!(
            event = "migration.snapshot_reused",
            found_migration,
            "existing pre-migration snapshot was kept"
        );
        return Ok(());
    }

    fs::copy(path, &snapshot)?;
    for suffix in ["-wal", "-shm"] {
        let source = sqlite_sidecar(path, suffix);
        if source.exists() {
            fs::copy(&source, sqlite_sidecar(&snapshot, suffix))?;
        }
    }
    tracing::info!(
        event = "migration.snapshot_created",
        found_migration,
        "created recoverable pre-migration snapshot"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        hash::{Hash, Hasher},
        path::{Path, PathBuf},
        time::SystemTime,
    };

    use sqlx::Row;

    use super::{initialize_database, pre_migration_snapshot_path, DatabaseBootstrapStatus};
    use crate::infrastructure::database::{connect_writable, read_migration_version};

    fn test_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "nestworth-phase2-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn file_hash(path: &Path) -> u64 {
        let bytes = fs::read(path).expect("database fixture should exist");
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        hasher.finish()
    }

    fn file_mtime(path: &Path) -> SystemTime {
        fs::metadata(path)
            .expect("database fixture metadata should exist")
            .modified()
            .expect("database fixture should have mtime")
    }

    #[test]
    fn creates_database_migrates_once_and_enables_required_pragmas() {
        tauri::async_runtime::block_on(async {
            let path = test_path("initial");
            let _ = fs::remove_file(&path);

            let first = initialize_database(path.clone()).await;
            assert_eq!(first.status, DatabaseBootstrapStatus::Migrated);
            let first_pool = first.pool.expect("first startup should be writable");
            let first_tables: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('app_settings', 'households', 'members', 'accounts')",
            )
            .fetch_one(&first_pool)
            .await
            .expect("schema query should succeed");
            assert_eq!(first_tables, 4);
            let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
                .fetch_one(&first_pool)
                .await
                .expect("foreign_keys pragma should be readable");
            assert_eq!(foreign_keys, 1);
            let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
                .fetch_one(&first_pool)
                .await
                .expect("journal mode pragma should be readable");
            assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
            first_pool.close().await;

            let second = initialize_database(path.clone()).await;
            assert_eq!(second.status, DatabaseBootstrapStatus::Ready);
            let second_pool = second.pool.expect("second startup should be writable");
            let settings_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_settings")
                .fetch_one(&second_pool)
                .await
                .expect("settings query should succeed");
            assert_eq!(settings_count, 1);
            second_pool.close().await;

            assert!(
                !pre_migration_snapshot_path(&path, 0).exists(),
                "a fresh database must not create a pre-migration snapshot"
            );
            remove_database(&path);
        });
    }

    #[test]
    fn existing_unmigrated_database_is_snapshotted_before_migrate() {
        tauri::async_runtime::block_on(async {
            let path = test_path("snapshot");
            remove_database(&path);

            let pool = connect_writable(&path, true)
                .await
                .expect("unmigrated fixture database should open");
            pool.close().await;

            let result = initialize_database(path.clone()).await;
            assert_eq!(result.status, DatabaseBootstrapStatus::Migrated);
            let pool = result.pool.expect("migrated startup should be writable");
            pool.close().await;

            let snapshot = pre_migration_snapshot_path(&path, 0);
            assert!(snapshot.is_file(), "pre-migration snapshot should exist");
            assert_eq!(
                read_migration_version(&snapshot)
                    .await
                    .expect("snapshot should remain readable"),
                0
            );
            assert_eq!(
                read_migration_version(&path)
                    .await
                    .expect("migrated database should be readable"),
                1
            );

            remove_database(&path);
            remove_database(&snapshot);
        });
    }

    #[test]
    fn existing_pre_migration_snapshot_is_not_overwritten() {
        tauri::async_runtime::block_on(async {
            let path = test_path("snapshot-keep");
            remove_database(&path);

            let pool = connect_writable(&path, true)
                .await
                .expect("unmigrated fixture database should open");
            pool.close().await;

            let snapshot = pre_migration_snapshot_path(&path, 0);
            fs::write(&snapshot, b"keep-me").expect("marker snapshot should be written");

            let result = initialize_database(path.clone()).await;
            assert_eq!(result.status, DatabaseBootstrapStatus::Migrated);
            result
                .pool
                .expect("migrated startup should be writable")
                .close()
                .await;

            assert_eq!(
                fs::read(&snapshot).expect("kept snapshot should be readable"),
                b"keep-me"
            );

            remove_database(&path);
            let _ = fs::remove_file(snapshot);
        });
    }

    #[test]
    fn snapshot_copy_failure_blocks_migration_without_writes() {
        tauri::async_runtime::block_on(async {
            let path = test_path("snapshot-fail");
            remove_database(&path);

            let pool = connect_writable(&path, true)
                .await
                .expect("unmigrated fixture database should open");
            pool.close().await;

            let before_hash = file_hash(&path);
            let snapshot = pre_migration_snapshot_path(&path, 0);
            fs::create_dir_all(&snapshot).expect("blocking snapshot directory should be created");

            let result = initialize_database(path.clone()).await;
            assert_eq!(result.status, DatabaseBootstrapStatus::MigrationFailed);
            assert!(result.pool.is_none());
            assert_eq!(file_hash(&path), before_hash);
            assert_eq!(
                read_migration_version(&path)
                    .await
                    .expect("blocked database should remain readable"),
                0
            );

            remove_database(&path);
            let _ = fs::remove_dir_all(snapshot);
        });
    }

    #[test]
    fn future_database_is_blocked_without_writes() {
        tauri::async_runtime::block_on(async {
            let path = test_path("future");
            let _ = fs::remove_file(&path);

            let pool = connect_writable(&path, true)
                .await
                .expect("fixture database should open");
            sqlx::query(
                "CREATE TABLE _sqlx_migrations (version BIGINT PRIMARY KEY NOT NULL, description TEXT NOT NULL, installed_on TIMESTAMP NOT NULL, success BOOLEAN NOT NULL, checksum BLOB NOT NULL, execution_time BIGINT NOT NULL)",
            )
            .execute(&pool)
            .await
            .expect("migration metadata table should be created");
            sqlx::query(
                "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (999, 'future', CURRENT_TIMESTAMP, 1, zeroblob(32), 1)",
            )
            .execute(&pool)
            .await
            .expect("future migration row should be inserted");
            pool.close().await;

            let before_rows = migration_rows(&path).await;
            let before_hash = file_hash(&path);
            let before_mtime = file_mtime(&path);

            let result = initialize_database(path.clone()).await;

            assert_eq!(
                result.status,
                DatabaseBootstrapStatus::UnsupportedNewerDatabase {
                    found: 999,
                    supported: 1,
                }
            );
            assert!(result.pool.is_none());
            assert_eq!(file_hash(&path), before_hash);
            assert_eq!(file_mtime(&path), before_mtime);
            assert_eq!(migration_rows(&path).await, before_rows);
            let app_state = crate::state::AppState::initialize(path.clone()).await;
            assert!(!app_state.is_writable());
            assert!(matches!(
                app_state.runtime(),
                crate::state::DatabaseRuntime::Blocked {
                    status: DatabaseBootstrapStatus::UnsupportedNewerDatabase {
                        found: 999,
                        supported: 1,
                    },
                    ..
                }
            ));
            assert!(matches!(
                app_state.writable_db(),
                Err(crate::error::AppError::UnsupportedNewerDatabase {
                    found: 999,
                    supported: 1,
                })
            ));

            remove_database(&path);
        });
    }

    fn remove_database(path: &Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(format!("{}-wal", path.display()));
        let _ = fs::remove_file(format!("{}-shm", path.display()));
    }

    async fn migration_rows(path: &Path) -> Vec<(i64, String, i64)> {
        let pool = crate::infrastructure::database::connect_read_only(path)
            .await
            .expect("fixture database should be readable");
        let rows = sqlx::query(
            "SELECT version, description, execution_time FROM _sqlx_migrations ORDER BY version",
        )
        .fetch_all(&pool)
        .await
        .expect("migration rows should be readable")
        .into_iter()
        .map(|row| {
            (
                row.get::<i64, _>("version"),
                row.get::<String, _>("description"),
                row.get::<i64, _>("execution_time"),
            )
        })
        .collect();
        pool.close().await;
        rows
    }
}
