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
    HistoryInitializationFailed,
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

    if let Err(_error) = crate::application::history_origin::initialize_history_origin_if_needed(
        &pool,
        crate::application::history_origin::HISTORY_ORIGIN_SCHEMA_VERSION,
    )
    .await
    {
        tracing::error!(
            event = "history.origin_init_failed",
            "history origin initialization failed"
        );
        pool.close().await;
        return blocked(DatabaseBootstrapStatus::HistoryInitializationFailed);
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
        str::FromStr,
        time::SystemTime,
    };

    use rust_decimal::Decimal;
    use sqlx::Row;

    use super::{initialize_database, pre_migration_snapshot_path, DatabaseBootstrapStatus};
    use crate::{
        application::{
            account_service::get_account, overview_service::get_overview,
            portfolio_service::get_portfolio,
        },
        infrastructure::database::{connect_writable, read_migration_version},
        state::AppState,
    };

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

    async fn stable_sqlite_hash(path: &Path) -> u64 {
        if let Ok(pool) = connect_writable(path, false).await {
            let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                .execute(&pool)
                .await;
            pool.close().await;
        }
        file_hash(path)
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
            let portfolio_tables: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('instruments', 'holdings', 'account_cash_values', 'instrument_quotes', 'fx_quotes', 'fx_quote_preferences')",
            )
            .fetch_one(&first_pool)
            .await
            .expect("portfolio schema query should succeed");
            assert_eq!(portfolio_tables, 6);
            let origin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history_origins")
                .fetch_one(&first_pool)
                .await
                .expect("origin count");
            assert_eq!(origin_count, 0);
            let history_tables: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('history_origins', 'activities', 'activity_legs', 'daily_valuation_snapshots')",
            )
            .fetch_one(&first_pool)
            .await
            .expect("history schema query should succeed");
            assert_eq!(history_tables, 4);
            let analytics_tables: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'cost_basis_declarations'",
            )
            .fetch_one(&first_pool)
            .await
            .expect("analytics schema query should succeed");
            assert_eq!(analytics_tables, 1);
            let declarations: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM cost_basis_declarations")
                    .fetch_one(&first_pool)
                    .await
                    .expect("declaration count");
            assert_eq!(declarations, 0);
            let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&first_pool)
                .await
                .expect("version");
            assert_eq!(version, 4);
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
                4
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

            let before_hash = stable_sqlite_hash(&path).await;
            let snapshot = pre_migration_snapshot_path(&path, 0);
            fs::create_dir_all(&snapshot).expect("blocking snapshot directory should be created");

            let result = initialize_database(path.clone()).await;
            assert_eq!(result.status, DatabaseBootstrapStatus::MigrationFailed);
            assert!(result.pool.is_none());
            assert_eq!(stable_sqlite_hash(&path).await, before_hash);
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
            let before_hash = stable_sqlite_hash(&path).await;
            let before_mtime = file_mtime(&path);

            let result = initialize_database(path.clone()).await;

            assert_eq!(
                result.status,
                DatabaseBootstrapStatus::UnsupportedNewerDatabase {
                    found: 999,
                    supported: 4,
                }
            );
            assert!(result.pool.is_none());
            assert_eq!(stable_sqlite_hash(&path).await, before_hash);
            assert_eq!(file_mtime(&path), before_mtime);
            assert_eq!(migration_rows(&path).await, before_rows);
            let app_state = crate::state::AppState::initialize(path.clone()).await;
            assert!(!app_state.is_writable());
            assert!(matches!(
                app_state.runtime(),
                crate::state::DatabaseRuntime::Blocked {
                    status: DatabaseBootstrapStatus::UnsupportedNewerDatabase {
                        found: 999,
                        supported: 4,
                    },
                    ..
                }
            ));
            assert!(matches!(
                app_state.writable_db(),
                Err(crate::error::AppError::UnsupportedNewerDatabase {
                    found: 999,
                    supported: 4,
                })
            ));

            remove_database(&path);
        });
    }

    #[test]
    fn released_v011_fixture_migrates_without_changing_overview_or_relationships() {
        tauri::async_runtime::block_on(async {
            let path = test_path("v011-released-fixture");
            remove_database(&path);
            let pool = connect_writable(&path, true)
                .await
                .expect("v0.1.1 fixture should open");
            let migration = super::MIGRATOR
                .iter()
                .find(|item| item.version == 1)
                .expect("migration 001 should exist")
                .clone();
            {
                let mut conn = pool.acquire().await.expect("connection");
                sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
                    .await
                    .expect("migration metadata table should be created");
                sqlx::migrate::Migrate::apply(&mut *conn, &migration)
                    .await
                    .expect("v0.1.1 schema should apply");
            }
            sqlx::raw_sql(include_str!("../../test-fixtures/v0.1.1.sql"))
                .execute(&pool)
                .await
                .expect("released fixture should load");

            let rows = sqlx::query(
                "SELECT a.primary_category, v.amount
                 FROM accounts a
                 JOIN (
                   SELECT account_id, amount,
                          ROW_NUMBER() OVER (PARTITION BY account_id ORDER BY effective_at DESC, created_at DESC, id DESC) AS rn
                   FROM account_values
                 ) v ON v.account_id = a.id AND v.rn = 1
                 WHERE a.archived_at IS NULL AND a.include_in_net_worth = 1",
            )
            .fetch_all(&pool)
            .await
            .expect("legacy overview rows");
            let mut before_assets = Decimal::ZERO;
            let mut before_liabilities = Decimal::ZERO;
            for row in rows {
                let category: String = row.get("primary_category");
                let amount: Decimal =
                    Decimal::from_str(&row.get::<String, _>("amount")).expect("fixture amount");
                if category == "liability" {
                    before_liabilities += amount;
                } else {
                    before_assets += amount;
                }
            }
            let before_net_worth = before_assets - before_liabilities;
            assert_eq!(before_assets, Decimal::from_str("125000").expect("assets"));
            assert_eq!(
                before_liabilities,
                Decimal::from_str("5000").expect("liabilities")
            );
            assert_eq!(
                before_net_worth,
                Decimal::from_str("120000").expect("net worth")
            );
            pool.close().await;

            let migrated = initialize_database(path.clone()).await;
            assert_eq!(migrated.status, DatabaseBootstrapStatus::Migrated);
            let pool = migrated.pool.expect("migrated fixture should be writable");
            let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .expect("version");
            assert_eq!(version, 4);
            let declarations: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM cost_basis_declarations")
                    .fetch_one(&pool)
                    .await
                    .expect("declaration count");
            assert_eq!(declarations, 0);
            for (table, expected) in [
                ("households", 1),
                ("app_settings", 1),
                ("media_assets", 1),
                ("members", 2),
                ("institutions", 2),
                ("account_groups", 2),
                ("accounts", 4),
                ("account_ownership", 4),
                ("account_values", 6),
            ] {
                let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                    .fetch_one(&pool)
                    .await
                    .expect("representative row count");
                assert_eq!(count, expected, "row count for {table}");
            }
            let retained: (String, String) = sqlx::query_as(
                "SELECT institution_id, group_id FROM accounts WHERE id = '99999999-9999-4999-8999-999999999999'",
            )
            .fetch_one(&pool)
            .await
            .expect("retained archived references");
            assert_eq!(retained.0, "55555555-5555-4555-8555-555555555555");
            assert_eq!(retained.1, "77777777-7777-4777-8777-777777777777");
            let history: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM account_values WHERE account_id = '99999999-9999-4999-8999-999999999999'",
            )
            .fetch_one(&pool)
            .await
            .expect("account value history");
            assert_eq!(history, 2);
            let origin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history_origins")
                .fetch_one(&pool)
                .await
                .expect("origin count");
            assert_eq!(origin_count, 1);
            let activity_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
                .fetch_one(&pool)
                .await
                .expect("activity count");
            assert_eq!(activity_count, 0);
            let foreign_keys = sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&pool)
                .await
                .expect("foreign key check");
            assert!(foreign_keys.is_empty());
            let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
                .fetch_one(&pool)
                .await
                .expect("integrity check");
            assert_eq!(integrity, "ok");
            pool.close().await;

            let overview = {
                let state = AppState::initialize(path.clone()).await;
                get_overview(&state).await.expect("migrated overview")
            };
            assert_eq!(overview.account_count, 3);
            assert_eq!(overview.assets.amount, "125000");
            assert_eq!(overview.liabilities.amount, "5000");
            assert_eq!(overview.net_worth.amount, "120000");
            assert_eq!(overview.assets.amount, before_assets.to_string());
            assert_eq!(overview.liabilities.amount, before_liabilities.to_string());
            assert_eq!(overview.net_worth.amount, before_net_worth.to_string());

            let reopened = initialize_database(path.clone()).await;
            assert_eq!(reopened.status, DatabaseBootstrapStatus::Ready);
            reopened.pool.expect("reopened pool").close().await;
            let snapshot = pre_migration_snapshot_path(&path, 1);
            remove_database(&path);
            remove_database(&snapshot);
        });
    }

    #[test]
    fn released_v012_fixture_loads_on_schema_002_without_changing_golden_totals() {
        tauri::async_runtime::block_on(async {
            let fixture = include_str!("../../test-fixtures/v0.1.2.sql");
            let goldens = include_str!("../../test-fixtures/v0.1.3-activity-goldens.md");
            for (label, source) in [
                ("v0.1.2.sql", fixture),
                ("v0.1.3-activity-goldens.md", goldens),
            ] {
                for forbidden in ["/Users/", "/home/", "password", "api_key", "secret"] {
                    assert!(
                        !source.contains(forbidden),
                        "{label} must not contain {forbidden}"
                    );
                }
            }

            let path = test_path("v012-released-fixture");
            remove_database(&path);
            let pool = connect_writable(&path, true)
                .await
                .expect("v0.1.2 fixture should open");
            for version in [1_i64, 2] {
                let migration = super::MIGRATOR
                    .iter()
                    .find(|item| item.version == version)
                    .expect("migration 001 and 002 should exist")
                    .clone();
                let mut conn = pool.acquire().await.expect("connection");
                sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
                    .await
                    .expect("migration metadata table should be created");
                sqlx::migrate::Migrate::apply(&mut *conn, &migration)
                    .await
                    .expect("released schema should apply");
            }
            sqlx::raw_sql(fixture)
                .execute(&pool)
                .await
                .expect("released fixture should load");

            let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .expect("version");
            assert_eq!(version, 2);
            let activity_id_columns: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM pragma_table_info('account_values') WHERE name = 'activity_id'",
            )
            .fetch_one(&pool)
            .await
            .expect("activity_id column probe");
            assert_eq!(activity_id_columns, 0);

            let expected_counts = [
                ("households", 1),
                ("app_settings", 1),
                ("media_assets", 1),
                ("members", 3),
                ("institutions", 2),
                ("account_groups", 2),
                ("accounts", 5),
                ("account_ownership", 6),
                ("account_values", 7),
                ("instruments", 4),
                ("holdings", 3),
                ("account_cash_values", 2),
                ("instrument_quotes", 5),
                ("fx_quote_preferences", 3),
                ("fx_quotes", 5),
            ];
            assert_table_counts(&pool, &expected_counts).await;
            let retained: (String, String) = sqlx::query_as(
                "SELECT institution_id, group_id FROM accounts WHERE id = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'",
            )
            .fetch_one(&pool)
            .await
            .expect("retained archived references");
            assert_eq!(retained.0, "55555555-5555-4555-8555-555555555555");
            assert_eq!(retained.1, "77777777-7777-4777-8777-777777777777");
            let archived_holding: (String, Option<String>) = sqlx::query_as(
                "SELECT instrument_id, archived_at FROM holdings WHERE id = '32323232-3232-4323-8323-323232323232'",
            )
            .fetch_one(&pool)
            .await
            .expect("archived holding should remain");
            assert_eq!(archived_holding.0, "23232323-2323-4323-8323-232323232323");
            assert!(archived_holding.1.is_some());
            let value_history: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM account_values WHERE account_id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'",
            )
            .fetch_one(&pool)
            .await
            .expect("account value history");
            assert_eq!(value_history, 2);
            let cash_history: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM account_cash_values WHERE account_id = '99999999-9999-4999-8999-999999999999'",
            )
            .fetch_one(&pool)
            .await
            .expect("account cash history");
            assert_eq!(cash_history, 2);
            let ownership_mismatch: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM (
                   SELECT account_id FROM account_ownership GROUP BY account_id HAVING SUM(share_bps) != 10000
                 )",
            )
            .fetch_one(&pool)
            .await
            .expect("ownership totals");
            assert_eq!(ownership_mismatch, 0);
            let foreign_keys = sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&pool)
                .await
                .expect("foreign key check");
            assert!(foreign_keys.is_empty());
            let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
                .fetch_one(&pool)
                .await
                .expect("integrity check");
            assert_eq!(integrity, "ok");
            pool.close().await;

            let initialized = initialize_database(path.clone()).await;
            assert_eq!(initialized.status, DatabaseBootstrapStatus::Migrated);
            let pool = initialized.pool.expect("loaded fixture should be writable");
            let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .expect("migrated version");
            assert_eq!(version, 4);
            let declarations: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM cost_basis_declarations")
                    .fetch_one(&pool)
                    .await
                    .expect("declaration count");
            assert_eq!(declarations, 0);
            let activity_id_columns: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM pragma_table_info('account_values') WHERE name = 'activity_id'",
            )
            .fetch_one(&pool)
            .await
            .expect("activity_id column probe after migrate");
            assert_eq!(activity_id_columns, 1);
            assert_table_counts(&pool, &expected_counts).await;
            assert_v012_origin_baseline(&pool).await;
            pool.close().await;

            let (overview, holdings_detail, portfolio) = {
                let state = AppState::initialize(path.clone()).await;
                let overview = get_overview(&state).await.expect("fixture overview");
                let holdings_detail = get_account(&state, "99999999-9999-4999-8999-999999999999")
                    .await
                    .expect("holdings account");
                let portfolio = get_portfolio(&state).await.expect("fixture portfolio");
                (overview, holdings_detail, portfolio)
            };
            assert!(overview.is_complete);
            assert_eq!(overview.account_count, 3);
            assert_eq!(overview.assets.amount, "63190");
            assert_eq!(overview.assets.currency, "CNY");
            assert_eq!(overview.liabilities.amount, "0");
            assert_eq!(overview.net_worth.amount, "63190");
            assert_eq!(
                holdings_detail
                    .valuation
                    .base
                    .as_ref()
                    .map(|value| value.amount.as_str()),
                Some("62190")
            );
            assert!(holdings_detail.valuation.complete);
            assert_eq!(portfolio.total.amount, "63190");
            assert_eq!(portfolio.total.currency, "CNY");
            assert!(portfolio.is_complete);
            assert_eq!(portfolio.coverage_bps, 10_000);
            assert_eq!(portfolio.accounts.len(), 2);
            assert!(portfolio.accounts.iter().any(|item| {
                item.account_id == "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
                    && item.base_value.as_ref().map(|value| value.amount.as_str()) == Some("1000")
            }));
            assert_eq!(portfolio.positions.len(), 2);

            let reopened = initialize_database(path.clone()).await;
            assert_eq!(reopened.status, DatabaseBootstrapStatus::Ready);
            let pool = reopened.pool.expect("reopened pool");
            assert_table_counts(&pool, &expected_counts).await;
            assert_v012_origin_baseline(&pool).await;
            pool.close().await;
            let snapshot = pre_migration_snapshot_path(&path, 2);
            remove_database(&path);
            remove_database(&snapshot);
        });
    }

    #[test]
    fn released_v013_fixture_loads_on_schema_003_without_v012_overview_goldens() {
        tauri::async_runtime::block_on(async {
            let fixture = include_str!("../../test-fixtures/v0.1.3.sql");
            let goldens = include_str!("../../test-fixtures/v0.1.4-analytics-goldens.md");
            for (label, source) in [
                ("v0.1.3.sql", fixture),
                ("v0.1.4-analytics-goldens.md", goldens),
            ] {
                for forbidden in ["/Users/", "/home/", "password", "api_key", "secret"] {
                    assert!(
                        !source.contains(forbidden),
                        "{label} must not contain {forbidden}"
                    );
                }
            }

            let path = test_path("v013-released-fixture");
            remove_database(&path);
            let pool = connect_writable(&path, true)
                .await
                .expect("v0.1.3 fixture should open");
            for version in [1_i64, 2, 3] {
                let migration = super::MIGRATOR
                    .iter()
                    .find(|item| item.version == version)
                    .expect("migrations 001, 002, and 003 should exist")
                    .clone();
                let mut conn = pool.acquire().await.expect("connection");
                sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
                    .await
                    .expect("migration metadata table should be created");
                sqlx::migrate::Migrate::apply(&mut *conn, &migration)
                    .await
                    .expect("released schema should apply");
            }
            sqlx::raw_sql(fixture)
                .execute(&pool)
                .await
                .expect("released v0.1.3 fixture should load");

            let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .expect("version");
            assert_eq!(version, 3);

            let origin: (i64, String, i64) = sqlx::query_as(
                "SELECT COUNT(*), timezone, timezone_confirmed FROM history_origins",
            )
            .fetch_one(&pool)
            .await
            .expect("history origin");
            assert_eq!(origin.0, 1);
            assert_eq!(origin.1, "Asia/Singapore");
            assert_eq!(origin.2, 1);

            let kinds: Vec<String> =
                sqlx::query_scalar("SELECT DISTINCT kind FROM activities ORDER BY kind")
                    .fetch_all(&pool)
                    .await
                    .expect("activity kinds");
            for required in [
                "buy",
                "deposit",
                "fee",
                "income",
                "opening_adjustment",
                "position_adjustment",
                "reversal",
                "sell",
                "transfer",
                "withdrawal",
            ] {
                assert!(
                    kinds.iter().any(|kind| kind == required),
                    "v0.1.3 fixture must include activity kind {required}"
                );
            }

            let origin_qqq: String = sqlx::query_scalar(
                "SELECT quantity FROM history_origin_holdings WHERE holding_id = '30303030-3030-4303-8303-303030303030'",
            )
            .fetch_one(&pool)
            .await
            .expect("origin QQQ holding");
            assert_eq!(origin_qqq, "3");
            let current_qqq: String = sqlx::query_scalar(
                "SELECT quantity FROM holdings WHERE id = '30303030-3030-4303-8303-303030303030'",
            )
            .fetch_one(&pool)
            .await
            .expect("current QQQ holding");
            assert_eq!(current_qqq, "3");

            let reversal_pairs: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM activities original
                 JOIN activities reversal
                   ON reversal.reverses = original.id
                  AND reversal.kind = 'reversal'",
            )
            .fetch_one(&pool)
            .await
            .expect("reversal pairs");
            assert!(reversal_pairs >= 1, "fixture must include a reversal pair");

            let correction_chains: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM activities original
                 JOIN activities reversal
                   ON reversal.reverses = original.id
                  AND reversal.kind = 'reversal'
                  AND reversal.correction_group IS NOT NULL
                 JOIN activities replacement
                   ON replacement.corrects = original.id
                  AND replacement.correction_group = reversal.correction_group",
            )
            .fetch_one(&pool)
            .await
            .expect("correction chains");
            assert!(
                correction_chains >= 1,
                "fixture must include a correction chain"
            );

            let revised_days: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM (
                   SELECT snapshot_on FROM daily_valuation_snapshots
                   GROUP BY snapshot_on HAVING COUNT(DISTINCT revision) >= 2
                 )",
            )
            .fetch_one(&pool)
            .await
            .expect("snapshot revisions");
            assert!(
                revised_days >= 1,
                "fixture must include two snapshot revisions for one local date"
            );

            let incomplete_days: i64 = sqlx::query_scalar(
                "SELECT COUNT(DISTINCT snapshot_on) FROM daily_valuation_snapshots WHERE is_complete = 0",
            )
            .fetch_one(&pool)
            .await
            .expect("incomplete snapshot days");
            assert!(
                incomplete_days >= 1,
                "fixture must include an incomplete snapshot day"
            );

            let zero_gross_settlement_legs: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM activity_legs
                 WHERE activity_id = '01a0188f-8621-7a61-a206-bf5800173c36'
                   AND role = 'settlement'",
            )
            .fetch_one(&pool)
            .await
            .expect("zero-gross settlement legs");
            assert_eq!(zero_gross_settlement_legs, 0);

            let fifo_trade_fee_kinds: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM activities
                 WHERE id IN (
                   '01a0188f-861c-7b20-8609-535e345b7c42',
                   '01a0188f-861e-7e70-930b-5f4e2d6cda2d',
                   '01a0188f-861f-7c20-83d1-4abb57f8ddc0'
                 )
                   AND fee_kind IS NOT NULL",
            )
            .fetch_one(&pool)
            .await
            .expect("FIFO trade fee_kind");
            assert_eq!(fifo_trade_fee_kinds, 0);

            let foreign_keys = sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&pool)
                .await
                .expect("foreign key check");
            assert!(foreign_keys.is_empty());
            let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
                .fetch_one(&pool)
                .await
                .expect("integrity check");
            assert_eq!(integrity, "ok");
            pool.close().await;

            // Posted Activities after origin change current Overview/Portfolio.
            // Do not assert v0.1.2 goldens 62190/63190 against v0.1.3.sql.

            remove_database(&path);
        });
    }

    #[test]
    fn released_v013_fixture_migrates_to_4_with_zero_declarations_and_unchanged_ids() {
        tauri::async_runtime::block_on(async {
            let path = test_path("v013-migrate-004");
            remove_database(&path);
            let pool = connect_writable(&path, true)
                .await
                .expect("v0.1.3 fixture should open");
            for version in [1_i64, 2, 3] {
                let migration = super::MIGRATOR
                    .iter()
                    .find(|item| item.version == version)
                    .expect("migrations 001, 002, and 003 should exist")
                    .clone();
                let mut conn = pool.acquire().await.expect("connection");
                sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
                    .await
                    .expect("migration metadata table should be created");
                sqlx::migrate::Migrate::apply(&mut *conn, &migration)
                    .await
                    .expect("released schema should apply");
            }
            sqlx::raw_sql(include_str!("../../test-fixtures/v0.1.3.sql"))
                .execute(&pool)
                .await
                .expect("released v0.1.3 fixture should load");

            let before_activities: String = sqlx::query_scalar(
                "SELECT COALESCE(GROUP_CONCAT(id, ','), '') FROM (SELECT id FROM activities ORDER BY id)",
            )
            .fetch_one(&pool)
            .await
            .expect("activity ids");
            let before_legs: String = sqlx::query_scalar(
                "SELECT COALESCE(GROUP_CONCAT(id, ','), '') FROM (SELECT id FROM activity_legs ORDER BY id)",
            )
            .fetch_one(&pool)
            .await
            .expect("leg ids");
            let before_snapshots: String = sqlx::query_scalar(
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || CAST(revision AS TEXT), ','), '') FROM (SELECT id, revision FROM daily_valuation_snapshots ORDER BY id)",
            )
            .fetch_one(&pool)
            .await
            .expect("snapshot ids");
            let before_corrections: String = sqlx::query_scalar(
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || IFNULL(reverses, '') || ':' || IFNULL(corrects, ''), ','), '') FROM (SELECT id, reverses, corrects FROM activities ORDER BY id)",
            )
            .fetch_one(&pool)
            .await
            .expect("correction links");
            let before_archives: String = sqlx::query_scalar(
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || IFNULL(archived_at, ''), ','), '') FROM (SELECT id, archived_at FROM accounts ORDER BY id)",
            )
            .fetch_one(&pool)
            .await
            .expect("archive state");
            let before_holdings: String = sqlx::query_scalar(
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || quantity, ','), '') FROM (SELECT id, quantity FROM holdings ORDER BY id)",
            )
            .fetch_one(&pool)
            .await
            .expect("holding quantities");
            let before_activity_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
                .fetch_one(&pool)
                .await
                .expect("activity count");
            let origin_schema: i64 =
                sqlx::query_scalar("SELECT schema_version FROM history_origins")
                    .fetch_one(&pool)
                    .await
                    .expect("origin schema");
            assert_eq!(origin_schema, 3);
            pool.close().await;

            let migrated = initialize_database(path.clone()).await;
            assert_eq!(migrated.status, DatabaseBootstrapStatus::Migrated);
            let pool = migrated.pool.expect("migrated fixture should be writable");
            let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .expect("version");
            assert_eq!(version, 4);
            let declarations: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM cost_basis_declarations")
                    .fetch_one(&pool)
                    .await
                    .expect("declaration count");
            assert_eq!(declarations, 0);
            let after_activity_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
                .fetch_one(&pool)
                .await
                .expect("activity count after migrate");
            assert_eq!(after_activity_count, before_activity_count);
            let after_activities: String = sqlx::query_scalar(
                "SELECT COALESCE(GROUP_CONCAT(id, ','), '') FROM (SELECT id FROM activities ORDER BY id)",
            )
            .fetch_one(&pool)
            .await
            .expect("activity ids after");
            let after_legs: String = sqlx::query_scalar(
                "SELECT COALESCE(GROUP_CONCAT(id, ','), '') FROM (SELECT id FROM activity_legs ORDER BY id)",
            )
            .fetch_one(&pool)
            .await
            .expect("leg ids after");
            let after_snapshots: String = sqlx::query_scalar(
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || CAST(revision AS TEXT), ','), '') FROM (SELECT id, revision FROM daily_valuation_snapshots ORDER BY id)",
            )
            .fetch_one(&pool)
            .await
            .expect("snapshot ids after");
            let after_corrections: String = sqlx::query_scalar(
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || IFNULL(reverses, '') || ':' || IFNULL(corrects, ''), ','), '') FROM (SELECT id, reverses, corrects FROM activities ORDER BY id)",
            )
            .fetch_one(&pool)
            .await
            .expect("correction links after");
            let after_archives: String = sqlx::query_scalar(
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || IFNULL(archived_at, ''), ','), '') FROM (SELECT id, archived_at FROM accounts ORDER BY id)",
            )
            .fetch_one(&pool)
            .await
            .expect("archive state after");
            let after_holdings: String = sqlx::query_scalar(
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || quantity, ','), '') FROM (SELECT id, quantity FROM holdings ORDER BY id)",
            )
            .fetch_one(&pool)
            .await
            .expect("holding quantities after");
            assert_eq!(after_activities, before_activities);
            assert_eq!(after_legs, before_legs);
            assert_eq!(after_snapshots, before_snapshots);
            assert_eq!(after_corrections, before_corrections);
            assert_eq!(after_archives, before_archives);
            assert_eq!(after_holdings, before_holdings);
            let origin_schema: i64 =
                sqlx::query_scalar("SELECT schema_version FROM history_origins")
                    .fetch_one(&pool)
                    .await
                    .expect("origin schema after");
            assert_eq!(origin_schema, 3);
            let zero_gross_settlement_legs: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM activity_legs
                 WHERE activity_id = '01a0188f-8621-7a61-a206-bf5800173c36'
                   AND role = 'settlement'",
            )
            .fetch_one(&pool)
            .await
            .expect("zero-gross settlement legs after migrate");
            assert_eq!(zero_gross_settlement_legs, 0);
            let fifo_trade_fee_kinds: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM activities
                 WHERE id IN (
                   '01a0188f-861c-7b20-8609-535e345b7c42',
                   '01a0188f-861e-7e70-930b-5f4e2d6cda2d',
                   '01a0188f-861f-7c20-83d1-4abb57f8ddc0'
                 )
                   AND fee_kind IS NOT NULL",
            )
            .fetch_one(&pool)
            .await
            .expect("FIFO trade fee_kind after migrate");
            assert_eq!(fifo_trade_fee_kinds, 0);
            let foreign_keys = sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&pool)
                .await
                .expect("foreign key check");
            assert!(foreign_keys.is_empty());
            let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
                .fetch_one(&pool)
                .await
                .expect("integrity check");
            assert_eq!(integrity, "ok");
            pool.close().await;

            remove_database(&path);
            let snapshot = pre_migration_snapshot_path(&path, 3);
            remove_database(&snapshot);
        });
    }

    #[test]
    fn schema_002_database_is_snapshotted_before_migrate_to_003() {
        tauri::async_runtime::block_on(async {
            let path = test_path("snapshot-002");
            remove_database(&path);
            let pool = connect_writable(&path, true)
                .await
                .expect("schema 002 fixture should open");
            for version in [1_i64, 2] {
                let migration = super::MIGRATOR
                    .iter()
                    .find(|item| item.version == version)
                    .expect("migration 001 and 002 should exist")
                    .clone();
                let mut conn = pool.acquire().await.expect("connection");
                sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
                    .await
                    .expect("migration metadata table should be created");
                sqlx::migrate::Migrate::apply(&mut *conn, &migration)
                    .await
                    .expect("released schema should apply");
            }
            pool.close().await;

            let result = initialize_database(path.clone()).await;
            assert_eq!(result.status, DatabaseBootstrapStatus::Migrated);
            result
                .pool
                .expect("migrated startup should be writable")
                .close()
                .await;

            let snapshot = pre_migration_snapshot_path(&path, 2);
            assert!(snapshot.is_file(), "pre-migration snapshot should exist");
            assert_eq!(
                read_migration_version(&snapshot)
                    .await
                    .expect("snapshot should remain readable"),
                2
            );
            assert_eq!(
                read_migration_version(&path)
                    .await
                    .expect("migrated database should be readable"),
                4
            );

            remove_database(&path);
            remove_database(&snapshot);
        });
    }

    #[test]
    fn schema_002_snapshot_copy_failure_blocks_migration_without_writes() {
        tauri::async_runtime::block_on(async {
            let path = test_path("snapshot-002-fail");
            remove_database(&path);
            let pool = connect_writable(&path, true)
                .await
                .expect("schema 002 fixture should open");
            for version in [1_i64, 2] {
                let migration = super::MIGRATOR
                    .iter()
                    .find(|item| item.version == version)
                    .expect("migration 001 and 002 should exist")
                    .clone();
                let mut conn = pool.acquire().await.expect("connection");
                sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
                    .await
                    .expect("migration metadata table should be created");
                sqlx::migrate::Migrate::apply(&mut *conn, &migration)
                    .await
                    .expect("released schema should apply");
            }
            pool.close().await;

            let before_hash = stable_sqlite_hash(&path).await;
            let snapshot = pre_migration_snapshot_path(&path, 2);
            fs::create_dir_all(&snapshot).expect("blocking snapshot directory should be created");

            let result = initialize_database(path.clone()).await;
            assert_eq!(result.status, DatabaseBootstrapStatus::MigrationFailed);
            assert!(result.pool.is_none());
            let after_hash = stable_sqlite_hash(&path).await;
            assert_eq!(after_hash, before_hash);
            assert_eq!(
                read_migration_version(&path)
                    .await
                    .expect("blocked database should remain readable"),
                2
            );

            remove_database(&path);
            let _ = fs::remove_dir_all(snapshot);
        });
    }

    #[test]
    fn future_version_5_is_unchanged_and_writes_no_origin_snapshot_or_declaration() {
        tauri::async_runtime::block_on(async {
            let path = test_path("future-v5");
            remove_database(&path);
            let migrated = initialize_database(path.clone()).await;
            assert_eq!(migrated.status, DatabaseBootstrapStatus::Migrated);
            let pool = migrated.pool.expect("schema 4 database");
            sqlx::query(
                "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (5, 'future', CURRENT_TIMESTAMP, 1, zeroblob(32), 1)",
            )
            .execute(&pool)
            .await
            .expect("version 5 row should be inserted");
            pool.close().await;

            let before_hash = stable_sqlite_hash(&path).await;
            let before_mtime = file_mtime(&path);
            let before_rows = migration_rows(&path).await;

            let result = initialize_database(path.clone()).await;
            assert_eq!(
                result.status,
                DatabaseBootstrapStatus::UnsupportedNewerDatabase {
                    found: 5,
                    supported: 4,
                }
            );
            assert!(result.pool.is_none());
            assert_eq!(stable_sqlite_hash(&path).await, before_hash);
            assert_eq!(file_mtime(&path), before_mtime);
            assert_eq!(migration_rows(&path).await, before_rows);

            let pool = crate::infrastructure::database::connect_read_only(&path)
                .await
                .expect("future database remains readable");
            let origin_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'history_origins'",
            )
            .fetch_one(&pool)
            .await
            .expect("origin table probe");
            assert_eq!(origin_count, 1);
            let origins: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history_origins")
                .fetch_one(&pool)
                .await
                .expect("origin rows");
            assert_eq!(origins, 0);
            let snapshots: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM daily_valuation_snapshots")
                    .fetch_one(&pool)
                    .await
                    .expect("snapshot rows");
            assert_eq!(snapshots, 0);
            let declarations: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM cost_basis_declarations")
                    .fetch_one(&pool)
                    .await
                    .expect("declaration rows");
            assert_eq!(declarations, 0);
            pool.close().await;

            let state = AppState::initialize(path.clone()).await;
            crate::test_support::assert_activity_history_commands_write_nothing(
                &state,
                &path,
                before_hash,
            )
            .await;
            let declare = crate::application::cost_basis_service::declare_lot_cost_basis(
                &state,
                crate::application::cost_basis_service::DeclareLotCostBasisInput {
                    origin_holding_id: Some("30303030-3030-4303-8303-303030303030".to_owned()),
                    activity_leg_id: None,
                    instrument_id: "20202020-2020-4202-8202-202020202020".to_owned(),
                    declared_cost: "1500".to_owned(),
                    declared_currency: "USD".to_owned(),
                    acquired_on: None,
                    note: None,
                },
            )
            .await
            .expect_err("blocked declare");
            assert!(matches!(
                declare,
                crate::error::AppError::UnsupportedNewerDatabase {
                    found: 5,
                    supported: 4
                }
            ));
            let revoke = crate::application::cost_basis_service::revoke_lot_cost_basis(
                &state,
                crate::application::cost_basis_service::RevokeLotCostBasisInput {
                    origin_holding_id: Some("30303030-3030-4303-8303-303030303030".to_owned()),
                    activity_leg_id: None,
                },
            )
            .await
            .expect_err("blocked revoke");
            assert!(matches!(
                revoke,
                crate::error::AppError::UnsupportedNewerDatabase {
                    found: 5,
                    supported: 4
                }
            ));
            assert_eq!(stable_sqlite_hash(&path).await, before_hash);

            remove_database(&path);
        });
    }

    async fn assert_v012_origin_baseline(pool: &crate::infrastructure::database::SqlitePool) {
        let origin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history_origins")
            .fetch_one(pool)
            .await
            .expect("origin count");
        assert_eq!(origin_count, 1);
        let activity_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
            .fetch_one(pool)
            .await
            .expect("activity count");
        assert_eq!(activity_count, 0);
        let declarations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cost_basis_declarations")
            .fetch_one(pool)
            .await
            .expect("declaration count");
        assert_eq!(declarations, 0);
        let source: String = sqlx::query_scalar("SELECT source FROM history_origins")
            .fetch_one(pool)
            .await
            .expect("origin source");
        assert_eq!(source, "migrated_v012");
        let schema_version: i64 = sqlx::query_scalar("SELECT schema_version FROM history_origins")
            .fetch_one(pool)
            .await
            .expect("origin schema version");
        assert_eq!(schema_version, 3);

        let account_values: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM history_origin_account_values")
                .fetch_one(pool)
                .await
                .expect("origin account values");
        assert_eq!(account_values, 4);
        let cash_values: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM history_origin_cash_values")
                .fetch_one(pool)
                .await
                .expect("origin cash");
        assert_eq!(cash_values, 1);
        let holdings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history_origin_holdings")
            .fetch_one(pool)
            .await
            .expect("origin holdings");
        assert_eq!(holdings, 3);
        let states: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history_origin_account_states")
            .fetch_one(pool)
            .await
            .expect("origin account states");
        assert_eq!(states, 5);
        let ownership: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history_origin_ownership")
            .fetch_one(pool)
            .await
            .expect("origin ownership");
        assert_eq!(ownership, 6);

        let qqq: String = sqlx::query_scalar(
            "SELECT quantity FROM history_origin_holdings WHERE holding_id = '30303030-3030-4303-8303-303030303030'",
        )
        .fetch_one(pool)
        .await
        .expect("qqq quantity");
        assert_eq!(qqq, "3");
        let es3: String = sqlx::query_scalar(
            "SELECT quantity FROM history_origin_holdings WHERE holding_id = '31313131-3131-4313-8313-313131313131'",
        )
        .fetch_one(pool)
        .await
        .expect("es3 quantity");
        assert_eq!(es3, "1000");
        let qqq_observations: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM holding_quantity_values WHERE holding_id = '30303030-3030-4303-8303-303030303030'",
        )
        .fetch_one(pool)
        .await
        .expect("qqq quantity observations");
        assert_eq!(qqq_observations, 1);
        let quantity_with_activity: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM holding_quantity_values WHERE activity_id IS NOT NULL",
        )
        .fetch_one(pool)
        .await
        .expect("quantity observations with activity");
        assert_eq!(quantity_with_activity, 0);
        let cash: (String, String) = sqlx::query_as(
            "SELECT amount, currency FROM history_origin_cash_values WHERE account_id = '99999999-9999-4999-8999-999999999999'",
        )
        .fetch_one(pool)
        .await
        .expect("holdings cash");
        assert_eq!(cash.0, "5000");
        assert_eq!(cash.1, "SGD");
        let manual: String = sqlx::query_scalar(
            "SELECT amount FROM history_origin_account_values WHERE account_id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'",
        )
        .fetch_one(pool)
        .await
        .expect("manual value");
        assert_eq!(manual, "1000");
        let operating: String = sqlx::query_scalar(
            "SELECT amount FROM history_origin_account_values WHERE account_id = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb'",
        )
        .fetch_one(pool)
        .await
        .expect("operating cash");
        assert_eq!(operating, "0");
        let fx_prefs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fx_preference_observations")
            .fetch_one(pool)
            .await
            .expect("fx preference observations");
        assert_eq!(fx_prefs, 3);
        let instrument_prefs: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM instrument_preference_observations")
                .fetch_one(pool)
                .await
                .expect("instrument preference observations");
        assert_eq!(instrument_prefs, 4);
        let snapshot_state: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history_snapshot_state")
            .fetch_one(pool)
            .await
            .expect("snapshot state");
        assert_eq!(snapshot_state, 1);
        let foreign_keys = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(pool)
            .await
            .expect("foreign key check");
        assert!(foreign_keys.is_empty());
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(pool)
            .await
            .expect("integrity check");
        assert_eq!(integrity, "ok");
    }

    async fn assert_table_counts(
        pool: &crate::infrastructure::database::SqlitePool,
        expected: &[(&str, i64)],
    ) {
        for (table, expected) in expected {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(pool)
                .await
                .expect("representative row count");
            assert_eq!(count, *expected, "row count for {table}");
        }
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
