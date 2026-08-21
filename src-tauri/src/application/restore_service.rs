use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use super::{
    backup_service::{
        create_verified_backup_to, file_metadata, hash_file, inspect_backup_at,
        inspect_database_copy, read_bundle, same_file_metadata, sync_directory, sync_file,
        verify_schema_fingerprint, BackupInspectionDto, OwnedTempDir, SourceMetadata,
        BACKUP_EXTENSION, DATABASE_ENTRY_NAME,
    },
    freshness_policy_service::initialize_default_policies,
    history_origin::{initialize_history_origin_if_needed, HISTORY_ORIGIN_SCHEMA_VERSION},
};
use crate::{
    domain::{BackupId, Timestamp},
    error::{AppError, RestartReason},
    infrastructure::{
        database::{connect_writable, ensure_app_settings, verify_sqlite_runtime, SqlitePool},
        database_bootstrap::{max_supported_migration, MIGRATOR},
    },
    state::{AppState, RestoreFault},
};

pub const RECOVERY_BACKUP_EXPLANATION: &str = "Recovery backups are created automatically before Restore. Nestworth never deletes them automatically. Delete All Data removes only application-owned recovery copies, never a backup you selected elsewhere.";

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoreBackupInput {
    pub inspection_token: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoreBackupResultDto {
    pub restart_required: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InspectRecoveryBackupInput {
    pub backup_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryBackupSummaryDto {
    pub id: String,
    pub created_at: String,
    pub household_name: String,
    pub database_migration_version: i32,
    pub product_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryBackupListDto {
    pub items: Vec<RecoveryBackupSummaryDto>,
    pub explanation: String,
}

pub async fn restore_backup(
    state: &AppState,
    input: RestoreBackupInput,
) -> Result<RestoreBackupResultDto, AppError> {
    if !input.confirmed {
        return Err(AppError::validation(
            "confirmed",
            "Restoring a backup requires explicit confirmation.",
        ));
    }

    let operation = state.acquire_exclusive_operation().await?;
    let inspection = state
        .backup_inspection(&input.inspection_token)
        .ok_or_else(|| {
            AppError::invalid_backup("The backup inspection has expired. Inspect the backup again.")
        })?;
    revalidate_inspection_source(&inspection)?;

    let database_path = state.database_path().to_path_buf();
    let parent = database_parent(&database_path)?;
    let database_name = file_name(&database_path)?;
    let staging_dir =
        OwnedTempDir::new_with_prefix(parent, &format!("{database_name}.restore-staging-"))?;
    let staged_database = staging_dir.path().join(DATABASE_ENTRY_NAME);
    let readback = read_bundle(&inspection.canonical_path, Some(&staged_database))?;
    if i64::from(readback.manifest.database_migration_version) > max_supported_migration() {
        return Err(AppError::backup_unsupported_version());
    }

    inspect_database_copy(
        &staged_database,
        Some(i64::from(readback.manifest.database_migration_version)),
    )
    .await?;
    migrate_staged_database(&staged_database).await?;
    revalidate_inspection_source(&inspection)?;

    let pool = state.writable_db()?.clone();
    let recovery = create_recovery_backup(state, parent, database_name).await?;
    operation.mark_restart_required(RestartReason::Restore)?;

    if state.restore_fault() == RestoreFault::Close {
        return Err(AppError::restore_validation_failed("close"));
    }

    pool.close().await;
    remove_sidecars(&database_path)?;

    replace_live_database(
        state,
        &database_path,
        parent,
        database_name,
        &staged_database,
        &recovery,
    )?;

    tracing::info!(event = "restore.completed", "restore replaced the database");
    Ok(RestoreBackupResultDto {
        restart_required: true,
        reason: RestartReason::Restore.as_str().to_owned(),
    })
}

pub async fn list_recovery_backups(state: &AppState) -> Result<RecoveryBackupListDto, AppError> {
    let _ = state.writable_db()?;
    let database_path = state.database_path();
    let parent = database_parent(database_path)?;
    let prefix = recovery_prefix(database_path)?;
    let mut items = Vec::new();
    let entries = fs::read_dir(parent)
        .map_err(|_| AppError::invalid_backup("Application storage is unavailable."))?;
    for entry in entries {
        let entry =
            entry.map_err(|_| AppError::invalid_backup("Application storage is unavailable."))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(&prefix) || !name.ends_with(&format!(".{BACKUP_EXTENSION}")) {
            continue;
        }
        let Ok(readback) = read_bundle(&path, None) else {
            continue;
        };
        items.push(RecoveryBackupSummaryDto {
            id: readback.manifest.backup_id,
            created_at: readback.manifest.created_at,
            household_name: readback.manifest.household_name,
            database_migration_version: readback.manifest.database_migration_version,
            product_version: readback.manifest.product_version,
        });
    }
    items.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then(right.id.cmp(&left.id))
    });
    Ok(RecoveryBackupListDto {
        items,
        explanation: RECOVERY_BACKUP_EXPLANATION.to_owned(),
    })
}

pub async fn inspect_recovery_backup(
    state: &AppState,
    input: InspectRecoveryBackupInput,
) -> Result<BackupInspectionDto, AppError> {
    let backup_id = BackupId::parse(&input.backup_id)
        .map_err(|_| AppError::invalid_backup("The recovery backup identifier is invalid."))?;
    let path = find_recovery_backup(state, &backup_id.to_string()).await?;
    let metadata = file_metadata(&path)?;
    inspect_backup_at(state, &path, metadata).await
}

fn revalidate_inspection_source(
    inspection: &crate::state::StoredBackupInspection,
) -> Result<(), AppError> {
    let metadata = file_metadata(&inspection.canonical_path).map_err(|_| {
        AppError::invalid_backup("The backup inspection has expired. Inspect the backup again.")
    })?;
    let expected = SourceMetadata {
        len: inspection.file_size,
        modified: inspection.modified_at,
        device: inspection.file_device,
        inode: inspection.file_inode,
    };
    let sha256 = hash_file(&inspection.canonical_path)?.sha256;
    if !same_file_metadata(&metadata, &expected) || sha256 != inspection.sha256 {
        return Err(AppError::invalid_backup(
            "The selected backup changed. Inspect the backup again.",
        ));
    }
    Ok(())
}

async fn migrate_staged_database(path: &Path) -> Result<(), AppError> {
    let pool = connect_writable(path, false)
        .await
        .map_err(|_| AppError::restore_validation_failed("schema"))?;
    let result = migrate_staged_database_in_pool(&pool).await;
    let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&pool)
        .await;
    pool.close().await;
    remove_sidecars(path)?;
    result
}

async fn migrate_staged_database_in_pool(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::query("PRAGMA trusted_schema = OFF")
        .execute(pool)
        .await
        .map_err(|_| AppError::restore_validation_failed("schema"))?;
    MIGRATOR
        .run(pool)
        .await
        .map_err(|_| AppError::restore_validation_failed("schema"))?;
    verify_sqlite_runtime(pool)
        .await
        .map_err(|_| AppError::restore_validation_failed("integrity"))?;
    ensure_app_settings(pool)
        .await
        .map_err(|_| AppError::restore_validation_failed("schema"))?;
    initialize_history_origin_if_needed(pool, HISTORY_ORIGIN_SCHEMA_VERSION)
        .await
        .map_err(|_| AppError::restore_validation_failed("origin"))?;
    initialize_default_policies(pool)
        .await
        .map_err(|_| AppError::restore_validation_failed("policy"))?;
    verify_schema_fingerprint(pool, max_supported_migration()).await?;
    validate_application_consistency(pool).await?;
    Ok(())
}

async fn validate_application_consistency(pool: &SqlitePool) -> Result<(), AppError> {
    let origin_complete: i64 = sqlx::query_scalar(
        "SELECT CASE
            WHEN EXISTS(SELECT 1 FROM history_origins)
             AND EXISTS(SELECT 1 FROM history_snapshot_state)
            THEN 1 ELSE 0 END",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| AppError::restore_validation_failed("origin"))?;
    if origin_complete != 1 {
        return Err(AppError::restore_validation_failed("origin"));
    }

    let activities_without_legs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM activities a
         WHERE NOT EXISTS (SELECT 1 FROM activity_legs l WHERE l.activity_id = a.id)",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| AppError::restore_validation_failed("ledger"))?;
    if activities_without_legs > 0 {
        return Err(AppError::restore_validation_failed("ledger"));
    }

    let projection_mismatch: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM holdings h
         WHERE EXISTS (SELECT 1 FROM holding_quantity_values v WHERE v.holding_id = h.id)
           AND h.quantity != (
                SELECT v.quantity FROM holding_quantity_values v
                WHERE v.holding_id = h.id
                ORDER BY v.effective_at DESC, v.created_at DESC, v.id DESC
                LIMIT 1
           )",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| AppError::restore_validation_failed("projection"))?;
    if projection_mismatch > 0 {
        return Err(AppError::restore_validation_failed("projection"));
    }

    let orphan_snapshot_items: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM daily_valuation_snapshot_items i
         WHERE NOT EXISTS (
            SELECT 1 FROM daily_valuation_snapshots s WHERE s.id = i.snapshot_id
         )",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| AppError::restore_validation_failed("snapshot"))?;
    if orphan_snapshot_items > 0 {
        return Err(AppError::restore_validation_failed("snapshot"));
    }

    let broken_declarations: i64 = sqlx::query_scalar(
        "SELECT (
            SELECT COUNT(*) FROM cost_basis_declarations d
            JOIN holdings h ON h.id = d.origin_holding_id
            WHERE d.origin_holding_id IS NOT NULL AND h.instrument_id != d.instrument_id
        ) + (
            SELECT COUNT(*) FROM cost_basis_declarations d
            JOIN activity_legs l ON l.id = d.activity_leg_id
            WHERE d.activity_leg_id IS NOT NULL
              AND l.instrument_id IS NOT NULL
              AND l.instrument_id != d.instrument_id
        )",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| AppError::restore_validation_failed("declaration"))?;
    if broken_declarations > 0 {
        return Err(AppError::restore_validation_failed("declaration"));
    }

    let broken_pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pending_activities p
         WHERE p.recurring_rule_id IS NOT NULL
           AND NOT EXISTS (
                SELECT 1 FROM recurring_activity_rules r WHERE r.id = p.recurring_rule_id
           )",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| AppError::restore_validation_failed("automation"))?;
    if broken_pending > 0 {
        return Err(AppError::restore_validation_failed("automation"));
    }

    let broken_benchmarks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM benchmark_observations o
         WHERE NOT EXISTS (SELECT 1 FROM benchmarks b WHERE b.id = o.benchmark_id)",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| AppError::restore_validation_failed("benchmark"))?;
    if broken_benchmarks > 0 {
        return Err(AppError::restore_validation_failed("benchmark"));
    }

    Ok(())
}

async fn create_recovery_backup(
    state: &AppState,
    parent: &Path,
    database_name: &str,
) -> Result<PathBuf, AppError> {
    let backup_id = BackupId::new();
    let created_at = Timestamp::now();
    let destination = parent.join(recovery_file_name(
        database_name,
        &backup_id.to_string(),
        &created_at,
    ));
    let written = create_verified_backup_to(state, &destination).await?;
    let readback = read_bundle(&destination, None)
        .map_err(|_| AppError::backup_create_failed("The recovery backup failed verification."))?;
    if readback.manifest != written
        || readback.database_digest.sha256 != written.database_sha256
        || readback.database_digest.byte_length
            != u64::try_from(written.database_byte_length).unwrap_or_default()
    {
        let _ = fs::remove_file(&destination);
        return Err(AppError::backup_create_failed(
            "The recovery backup failed verification.",
        ));
    }
    Ok(destination)
}

fn replace_live_database(
    state: &AppState,
    database_path: &Path,
    parent: &Path,
    database_name: &str,
    staged_database: &Path,
    recovery_bundle: &Path,
) -> Result<(), AppError> {
    let rollback = parent.join(format!(
        "{database_name}.restore-rollback-{}",
        Uuid::now_v7()
    ));
    copy_regular_file(database_path, &rollback)
        .map_err(|_| AppError::restore_validation_failed("replace"))?;
    sync_file(&rollback)?;

    if state.restore_fault() == RestoreFault::Rename {
        let _ = fs::remove_file(&rollback);
        return Err(AppError::restore_validation_failed("replace"));
    }

    if let Err(_error) = fs::rename(staged_database, database_path) {
        restore_live_from_recovery(database_path, parent, recovery_bundle)?;
        let _ = fs::remove_file(&rollback);
        return Err(AppError::restore_validation_failed("replace"));
    }

    if state.restore_fault() == RestoreFault::Fsync {
        restore_live_from_recovery(database_path, parent, recovery_bundle)?;
        let _ = fs::remove_file(&rollback);
        return Err(AppError::restore_validation_failed("replace"));
    }

    if let Err(_error) = sync_file(database_path).and_then(|_| sync_directory(parent)) {
        restore_live_from_recovery(database_path, parent, recovery_bundle)?;
        let _ = fs::remove_file(&rollback);
        return Err(AppError::restore_validation_failed("replace"));
    }

    let _ = fs::remove_file(&rollback);
    Ok(())
}

fn restore_live_from_recovery(
    database_path: &Path,
    parent: &Path,
    recovery_bundle: &Path,
) -> Result<(), AppError> {
    let extracted = parent.join(format!(
        "{}.restore-recovery-extract-{}",
        file_name(database_path)?,
        Uuid::now_v7()
    ));
    read_bundle(recovery_bundle, Some(&extracted))
        .map_err(|_| AppError::backup_corrupt("The recovery backup could not be restored."))?;
    fs::rename(&extracted, database_path)
        .map_err(|_| AppError::backup_corrupt("The recovery backup could not be restored."))?;
    sync_file(database_path)?;
    sync_directory(parent)?;
    Ok(())
}

fn copy_regular_file(source: &Path, destination: &Path) -> io::Result<()> {
    let mut input = fs::File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    io::copy(&mut input, &mut output)?;
    output.sync_all()?;
    Ok(())
}

async fn find_recovery_backup(state: &AppState, backup_id: &str) -> Result<PathBuf, AppError> {
    let database_path = state.database_path();
    let parent = database_parent(database_path)?;
    let prefix = recovery_prefix(database_path)?;
    let entries = fs::read_dir(parent)
        .map_err(|_| AppError::invalid_backup("Application storage is unavailable."))?;
    for entry in entries {
        let path = entry
            .map_err(|_| AppError::invalid_backup("Application storage is unavailable."))?
            .path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !name.starts_with(&prefix) {
            continue;
        }
        if let Ok(readback) = read_bundle(&path, None) {
            if readback.manifest.backup_id == backup_id {
                return Ok(path);
            }
        }
    }
    Err(AppError::not_found("recovery_backup", backup_id))
}

fn recovery_file_name(database_name: &str, backup_id: &str, created_at: &Timestamp) -> String {
    let stamp = created_at.as_utc().format("%Y%m%dT%H%M%SZ");
    format!("{database_name}.recovery-{stamp}-{backup_id}.{BACKUP_EXTENSION}")
}

fn recovery_prefix(database_path: &Path) -> Result<String, AppError> {
    Ok(format!("{}.recovery-", file_name(database_path)?))
}

fn database_parent(database_path: &Path) -> Result<&Path, AppError> {
    database_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| AppError::invalid_backup("Application storage is unavailable."))
}

fn file_name(path: &Path) -> Result<&str, AppError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::invalid_backup("Application storage is unavailable."))
}

fn remove_sidecars(database_path: &Path) -> Result<(), AppError> {
    for suffix in ["-wal", "-shm"] {
        let mut path = database_path.as_os_str().to_os_string();
        path.push(suffix);
        let path = PathBuf::from(path);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_error) => return Err(AppError::restore_validation_failed("replace")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::File, io::Write, time::Duration};

    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    use crate::{
        application::{
            backup_service::{
                inspect_backup, write_bundle, BackupManifestDto, CreateBackupInput,
                InspectBackupInput, BACKUP_FORMAT_ID, MANIFEST_ENTRY_NAME,
            },
            settings_service::{delete_all_data, get_settings, DeleteAllDataInput},
        },
        error::{AppError, RestartReason},
        infrastructure::database::{connect_writable, read_migration_version},
        state::AppState,
        test_support::{onboarded_state, test_path},
    };

    fn file_bytes(path: &Path) -> Vec<u8> {
        fs::read(path).expect("file bytes")
    }

    fn sha256_file(path: &Path) -> String {
        hash_file(path).expect("hash").sha256
    }

    async fn apply_migrations(pool: &SqlitePool, versions: impl IntoIterator<Item = i64>) {
        for version in versions {
            let migration = MIGRATOR
                .iter()
                .find(|item| item.version == version)
                .unwrap_or_else(|| panic!("migration {version} should exist"))
                .clone();
            let mut conn = pool.acquire().await.expect("connection");
            sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
                .await
                .expect("migration table");
            sqlx::migrate::Migrate::apply(&mut *conn, &migration)
                .await
                .unwrap_or_else(|_| panic!("migration {version} should apply"));
        }
    }

    async fn load_schema_fixture(path: &Path, version: i64) -> SqlitePool {
        let _ = fs::remove_file(path);
        let pool = connect_writable(path, true)
            .await
            .expect("fixture database");
        let schema_for_fixture = version.min(4);
        apply_migrations(&pool, 1..=schema_for_fixture).await;
        let fixture = match schema_for_fixture {
            1 => include_str!("../../test-fixtures/v0.1.1.sql"),
            2 => include_str!("../../test-fixtures/v0.1.2.sql"),
            3 => include_str!("../../test-fixtures/v0.1.3.sql"),
            4 => include_str!("../../test-fixtures/v0.1.4.sql"),
            _ => panic!("unsupported fixture version"),
        };
        sqlx::raw_sql(fixture)
            .execute(&pool)
            .await
            .expect("fixture should load");
        if version == 5 {
            apply_migrations(&pool, [5]).await;
        }
        let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&pool)
            .await;
        pool
    }

    async fn wrap_sqlite_as_backup_raw(
        sqlite: &Path,
        bundle: &Path,
        migration: i32,
    ) -> BackupManifestDto {
        let pool = connect_writable(sqlite, false)
            .await
            .expect("sqlite should open");
        let household: (String, String) = sqlx::query_as("SELECT id, name FROM households LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("household");
        pool.close().await;
        let digest = hash_file(sqlite).expect("sqlite hash");
        let manifest = BackupManifestDto {
            format_id: BACKUP_FORMAT_ID.to_owned(),
            format_version: "1".to_owned(),
            backup_id: BackupId::new().to_string(),
            product_version: env!("CARGO_PKG_VERSION").to_owned(),
            database_migration_version: migration,
            created_at: Timestamp::now().to_rfc3339(),
            household_id: household.0,
            household_name: household.1,
            database_byte_length: i32::try_from(digest.byte_length).expect("size"),
            database_sha256: digest.sha256,
        };
        write_bundle(bundle, &manifest, sqlite).expect("bundle should write");
        manifest
    }

    async fn wrap_sqlite_as_backup(sqlite: &Path, bundle: &Path) -> BackupManifestDto {
        let facts = inspect_database_copy(sqlite, None)
            .await
            .expect("sqlite should inspect");
        let digest = hash_file(sqlite).expect("sqlite hash");
        let manifest = BackupManifestDto {
            format_id: BACKUP_FORMAT_ID.to_owned(),
            format_version: "1".to_owned(),
            backup_id: BackupId::new().to_string(),
            product_version: env!("CARGO_PKG_VERSION").to_owned(),
            database_migration_version: i32::try_from(facts.migration).expect("migration"),
            created_at: Timestamp::now().to_rfc3339(),
            household_id: facts.household_id.expect("household id"),
            household_name: facts.household_name.expect("household name"),
            database_byte_length: i32::try_from(digest.byte_length).expect("size"),
            database_sha256: digest.sha256,
        };
        write_bundle(bundle, &manifest, sqlite).expect("bundle should write");
        manifest
    }

    async fn immutable_evidence(pool: &SqlitePool) -> (String, String, String, String) {
        let activities: String = sqlx::query_scalar(
            "SELECT COALESCE(GROUP_CONCAT(id, ','), '')
             FROM (SELECT id FROM activities ORDER BY id)",
        )
        .fetch_one(pool)
        .await
        .expect("activity evidence");
        let legs: String = sqlx::query_scalar(
            "SELECT COALESCE(GROUP_CONCAT(id, ','), '')
             FROM (SELECT id FROM activity_legs ORDER BY id)",
        )
        .fetch_one(pool)
        .await
        .expect("leg evidence");
        let origins: String = sqlx::query_scalar(
            "SELECT COALESCE(GROUP_CONCAT(id, ','), '')
             FROM (SELECT id FROM history_origins ORDER BY id)",
        )
        .fetch_one(pool)
        .await
        .expect("origin evidence");
        let declarations: String = sqlx::query_scalar(
            "SELECT COALESCE(GROUP_CONCAT(id, ','), '')
             FROM (SELECT id FROM cost_basis_declarations ORDER BY id)",
        )
        .fetch_one(pool)
        .await
        .expect("declaration evidence");
        (activities, legs, origins, declarations)
    }

    async fn inspect_and_restore(
        state: &AppState,
        bundle: &Path,
    ) -> Result<RestoreBackupResultDto, AppError> {
        let inspection = inspect_backup(
            state,
            InspectBackupInput {
                source_path: bundle.to_string_lossy().into_owned(),
            },
        )
        .await?;
        restore_backup(
            state,
            RestoreBackupInput {
                inspection_token: inspection.inspection_token,
                confirmed: true,
            },
        )
        .await
    }

    fn cleanup_state_files(path: &Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(format!("{}-wal", path.display()));
        let _ = fs::remove_file(format!("{}-shm", path.display()));
        if let Some(parent) = path.parent() {
            if let Ok(entries) = fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    let stem = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("");
                    if name.starts_with(&format!("{stem}.recovery-"))
                        || name.starts_with(&format!("{stem}.restore-"))
                        || name.starts_with(&format!("{stem}.pre-migrate-"))
                    {
                        let _ = fs::remove_file(entry.path());
                        let _ = fs::remove_dir_all(entry.path());
                    }
                }
            }
        }
    }

    #[test]
    fn schema_fingerprints_match_migrated_empty_databases() {
        tauri::async_runtime::block_on(async {
            for version in 1..=5 {
                let path = test_path("phase6", &format!("fingerprint-{version}"));
                let _ = fs::remove_file(&path);
                let pool = connect_writable(&path, true).await.expect("empty db");
                apply_migrations(&pool, 1..=version).await;
                verify_schema_fingerprint(&pool, version)
                    .await
                    .unwrap_or_else(|_| panic!("schema {version} fingerprint"));
                pool.close().await;
                cleanup_state_files(&path);
            }
        });
    }

    #[test]
    fn schema_one_through_five_bundles_restore_and_reopen_at_schema_five() {
        tauri::async_runtime::block_on(async {
            for version in 1..=5 {
                let (state, live) = onboarded_state(&format!("restore-schema-{version}")).await;
                let fixture_path = test_path("phase6", &format!("src-schema-{version}"));
                let pool = load_schema_fixture(&fixture_path, version).await;
                pool.close().await;
                let bundle = fixture_path.with_extension(BACKUP_EXTENSION);
                let _ = wrap_sqlite_as_backup(&fixture_path, &bundle).await;
                let before_bundle = file_bytes(&bundle);
                inspect_and_restore(&state, &bundle)
                    .await
                    .unwrap_or_else(|error| panic!("schema {version} restore: {error:?}"));
                assert_eq!(file_bytes(&bundle), before_bundle);
                drop(state);

                let reopened = AppState::initialize(live.clone()).await;
                assert!(reopened.is_writable());
                let version_now = read_migration_version(&live)
                    .await
                    .expect("reopened version");
                assert_eq!(version_now, 5, "schema {version} must reopen at 5");
                let household: String = sqlx::query_scalar("SELECT name FROM households LIMIT 1")
                    .fetch_one(reopened.writable_db().expect("writable"))
                    .await
                    .expect("household");
                assert_eq!(household, "Fixture Household");
                drop(reopened);
                cleanup_state_files(&live);
                cleanup_state_files(&fixture_path);
                let _ = fs::remove_file(bundle);
            }
        });
    }

    #[test]
    fn rejected_bundles_do_not_mutate_the_active_database() {
        tauri::async_runtime::block_on(async {
            for label in [
                "future",
                "wrong-product",
                "corrupt",
                "unexpected-object",
                "trigger",
                "view",
                "virtual-table",
                "incomplete-origin",
                "broken-ledger",
                "projection-mismatch",
                "broken-declaration",
            ] {
                let (state, live) = onboarded_state(&format!("reject-{label}")).await;
                let before = sha256_file(&live);
                let root = test_path("phase6", &format!("reject-src-{label}"));
                let bundle = make_rejected_bundle(&root, label).await;
                let before_bundle = file_bytes(&bundle);
                let result = inspect_and_restore(&state, &bundle).await;
                assert!(result.is_err(), "{label} must be rejected: {result:?}");
                assert_eq!(sha256_file(&live), before, "{label} must not mutate live");
                assert_eq!(file_bytes(&bundle), before_bundle);
                assert!(state.is_writable(), "{label} must leave runtime ready");
                drop(state);
                cleanup_state_files(&live);
                let _ = fs::remove_file(bundle);
            }
        });
    }

    async fn make_rejected_bundle(root: &Path, label: &str) -> PathBuf {
        match label {
            "future" => make_future_bundle(root).await,
            "wrong-product" => make_wrong_product_bundle(root).await,
            "corrupt" => make_corrupt_bundle(root),
            "unexpected-object" => make_unexpected_object_bundle(root).await,
            "trigger" => make_trigger_bundle(root).await,
            "view" => make_view_bundle(root).await,
            "virtual-table" => make_virtual_table_bundle(root).await,
            "incomplete-origin" => make_incomplete_origin_bundle(root).await,
            "broken-ledger" => make_broken_ledger_bundle(root).await,
            "projection-mismatch" => make_projection_mismatch_bundle(root).await,
            "broken-declaration" => make_broken_declaration_bundle(root).await,
            _ => panic!("unknown rejection case {label}"),
        }
    }

    async fn make_future_bundle(root: &Path) -> PathBuf {
        let sqlite = root.with_extension("sqlite3");
        let pool = load_schema_fixture(&sqlite, 5).await;
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (6, 'future', CURRENT_TIMESTAMP, 1, zeroblob(32), 1)",
        )
        .execute(&pool)
        .await
        .expect("future migration");
        pool.close().await;
        let bundle = root.with_extension(BACKUP_EXTENSION);
        let _ = wrap_sqlite_as_backup_raw(&sqlite, &bundle, 6).await;
        let _ = fs::remove_file(sqlite);
        bundle
    }

    async fn make_wrong_product_bundle(root: &Path) -> PathBuf {
        let sqlite = root.with_extension("sqlite3");
        let pool = load_schema_fixture(&sqlite, 5).await;
        pool.close().await;
        let bundle = root.with_extension(BACKUP_EXTENSION);
        let mut manifest = wrap_sqlite_as_backup(&sqlite, &bundle).await;
        manifest.format_id = "com.other.backup".to_owned();
        write_bundle(&bundle, &manifest, &sqlite).expect("rewrite");
        let _ = fs::remove_file(sqlite);
        bundle
    }

    fn make_corrupt_bundle(root: &Path) -> PathBuf {
        let bundle = root.with_extension(BACKUP_EXTENSION);
        fs::write(&bundle, b"not a zip").expect("corrupt bytes");
        bundle
    }

    async fn make_unexpected_object_bundle(root: &Path) -> PathBuf {
        let sqlite = root.with_extension("sqlite3");
        let pool = load_schema_fixture(&sqlite, 5).await;
        sqlx::query("CREATE TABLE extra_evil (id TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .expect("extra table");
        pool.close().await;
        let bundle = root.with_extension(BACKUP_EXTENSION);
        let _ = wrap_sqlite_as_backup_raw(&sqlite, &bundle, 5).await;
        let _ = fs::remove_file(sqlite);
        bundle
    }

    async fn make_trigger_bundle(root: &Path) -> PathBuf {
        let sqlite = root.with_extension("sqlite3");
        let pool = load_schema_fixture(&sqlite, 5).await;
        sqlx::query("CREATE TRIGGER extra_trigger AFTER INSERT ON members BEGIN SELECT 1; END")
            .execute(&pool)
            .await
            .expect("trigger");
        pool.close().await;
        let bundle = root.with_extension(BACKUP_EXTENSION);
        let _ = wrap_sqlite_as_backup_raw(&sqlite, &bundle, 5).await;
        let _ = fs::remove_file(sqlite);
        bundle
    }

    async fn make_view_bundle(root: &Path) -> PathBuf {
        let sqlite = root.with_extension("sqlite3");
        let pool = load_schema_fixture(&sqlite, 5).await;
        sqlx::query("CREATE VIEW extra_view AS SELECT id FROM households")
            .execute(&pool)
            .await
            .expect("view");
        pool.close().await;
        let bundle = root.with_extension(BACKUP_EXTENSION);
        let _ = wrap_sqlite_as_backup_raw(&sqlite, &bundle, 5).await;
        let _ = fs::remove_file(sqlite);
        bundle
    }

    async fn make_virtual_table_bundle(root: &Path) -> PathBuf {
        let sqlite = root.with_extension("sqlite3");
        let pool = load_schema_fixture(&sqlite, 5).await;
        sqlx::query("CREATE VIRTUAL TABLE extra_fts USING fts5(content)")
            .execute(&pool)
            .await
            .expect("virtual table");
        pool.close().await;
        let bundle = root.with_extension(BACKUP_EXTENSION);
        let _ = wrap_sqlite_as_backup_raw(&sqlite, &bundle, 5).await;
        let _ = fs::remove_file(sqlite);
        bundle
    }

    async fn make_incomplete_origin_bundle(root: &Path) -> PathBuf {
        let sqlite = root.with_extension("sqlite3");
        let pool = load_schema_fixture(&sqlite, 4).await;
        sqlx::query("DELETE FROM history_snapshot_state")
            .execute(&pool)
            .await
            .expect("delete snapshot state");
        pool.close().await;
        let bundle = root.with_extension(BACKUP_EXTENSION);
        let _ = wrap_sqlite_as_backup(&sqlite, &bundle).await;
        let _ = fs::remove_file(sqlite);
        bundle
    }

    async fn make_broken_ledger_bundle(root: &Path) -> PathBuf {
        let sqlite = root.with_extension("sqlite3");
        let pool = load_schema_fixture(&sqlite, 4).await;
        sqlx::query(
            "DELETE FROM activity_legs WHERE activity_id = (
                SELECT a.id FROM activities a
                WHERE NOT EXISTS (
                    SELECT 1 FROM cost_basis_declarations d
                    JOIN activity_legs l ON l.id = d.activity_leg_id
                    WHERE l.activity_id = a.id
                )
                LIMIT 1
            )",
        )
        .execute(&pool)
        .await
        .expect("delete unreferenced legs");
        pool.close().await;
        let bundle = root.with_extension(BACKUP_EXTENSION);
        let _ = wrap_sqlite_as_backup(&sqlite, &bundle).await;
        let _ = fs::remove_file(sqlite);
        bundle
    }

    async fn make_projection_mismatch_bundle(root: &Path) -> PathBuf {
        let sqlite = root.with_extension("sqlite3");
        let pool = load_schema_fixture(&sqlite, 4).await;
        sqlx::query(
            "UPDATE holdings SET quantity = quantity || '9' WHERE id IN (SELECT holding_id FROM holding_quantity_values LIMIT 1)",
        )
        .execute(&pool)
        .await
        .expect("mismatch quantity");
        pool.close().await;
        let bundle = root.with_extension(BACKUP_EXTENSION);
        let _ = wrap_sqlite_as_backup(&sqlite, &bundle).await;
        let _ = fs::remove_file(sqlite);
        bundle
    }

    async fn make_broken_declaration_bundle(root: &Path) -> PathBuf {
        let sqlite = root.with_extension("sqlite3");
        let pool = load_schema_fixture(&sqlite, 4).await;
        sqlx::query(
            "UPDATE cost_basis_declarations
             SET instrument_id = (
                SELECT id FROM instruments
                WHERE id != cost_basis_declarations.instrument_id
                LIMIT 1
             )
             WHERE id = (
                SELECT id FROM cost_basis_declarations
                WHERE origin_holding_id IS NOT NULL
                LIMIT 1
             )",
        )
        .execute(&pool)
        .await
        .expect("break declaration");
        pool.close().await;
        let bundle = root.with_extension(BACKUP_EXTENSION);
        let _ = wrap_sqlite_as_backup(&sqlite, &bundle).await;
        let _ = fs::remove_file(sqlite);
        bundle
    }

    #[test]
    fn inspection_token_identity_changes_require_reinspection() {
        tauri::async_runtime::block_on(async {
            let (state, live) = onboarded_state("token-identity").await;
            let destination = live.with_extension(BACKUP_EXTENSION);
            crate::application::backup_service::create_backup(
                &state,
                CreateBackupInput {
                    destination_path: destination.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                },
            )
            .await
            .expect("backup");
            let inspection = inspect_backup(
                &state,
                InspectBackupInput {
                    source_path: destination.to_string_lossy().into_owned(),
                },
            )
            .await
            .expect("inspect");

            state.expire_backup_inspection(&inspection.inspection_token);
            let expired = restore_backup(
                &state,
                RestoreBackupInput {
                    inspection_token: inspection.inspection_token.clone(),
                    confirmed: true,
                },
            )
            .await;
            assert!(expired.is_err());

            let inspection = inspect_backup(
                &state,
                InspectBackupInput {
                    source_path: destination.to_string_lossy().into_owned(),
                },
            )
            .await
            .expect("reinspect");
            let original = file_bytes(&destination);
            fs::write(&destination, b"replaced").expect("replace source");
            let replaced = restore_backup(
                &state,
                RestoreBackupInput {
                    inspection_token: inspection.inspection_token,
                    confirmed: true,
                },
            )
            .await;
            assert!(replaced.is_err());
            fs::write(&destination, &original).expect("restore source bytes");

            let inspection = inspect_backup(
                &state,
                InspectBackupInput {
                    source_path: destination.to_string_lossy().into_owned(),
                },
            )
            .await
            .expect("reinspect size");
            let mut bytes = file_bytes(&destination);
            bytes.push(0);
            fs::write(&destination, bytes).expect("size change");
            let size_changed = restore_backup(
                &state,
                RestoreBackupInput {
                    inspection_token: inspection.inspection_token,
                    confirmed: true,
                },
            )
            .await;
            assert!(size_changed.is_err());

            fs::write(&destination, original.clone()).expect("restore original backup");
            let inspection = inspect_backup(
                &state,
                InspectBackupInput {
                    source_path: destination.to_string_lossy().into_owned(),
                },
            )
            .await
            .expect("reinspect mtime");
            let file = File::options()
                .write(true)
                .open(&destination)
                .expect("open backup");
            file.set_modified(std::time::SystemTime::now() + Duration::from_secs(5))
                .expect("mtime change");
            drop(file);
            let mtime_changed = restore_backup(
                &state,
                RestoreBackupInput {
                    inspection_token: inspection.inspection_token,
                    confirmed: true,
                },
            )
            .await;
            assert!(mtime_changed.is_err());

            let inspection = inspect_backup(
                &state,
                InspectBackupInput {
                    source_path: destination.to_string_lossy().into_owned(),
                },
            )
            .await
            .expect("reinspect checksum");
            let mut checksum_bytes = file_bytes(&destination);
            let last = checksum_bytes.len() - 1;
            checksum_bytes[last] ^= 0xff;
            fs::write(&destination, checksum_bytes).expect("checksum change");
            let checksum_changed = restore_backup(
                &state,
                RestoreBackupInput {
                    inspection_token: inspection.inspection_token,
                    confirmed: true,
                },
            )
            .await;
            assert!(checksum_changed.is_err());

            drop(state);
            cleanup_state_files(&live);
            let _ = fs::remove_file(destination);
        });
    }

    #[test]
    fn confirmation_and_recovery_are_required_before_live_mutation() {
        tauri::async_runtime::block_on(async {
            let (state, live) = onboarded_state("confirm-required").await;
            let destination = live.with_extension(BACKUP_EXTENSION);
            crate::application::backup_service::create_backup(
                &state,
                CreateBackupInput {
                    destination_path: destination.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                },
            )
            .await
            .expect("backup");
            let before = sha256_file(&live);
            let inspection = inspect_backup(
                &state,
                InspectBackupInput {
                    source_path: destination.to_string_lossy().into_owned(),
                },
            )
            .await
            .expect("inspect");
            let error = restore_backup(
                &state,
                RestoreBackupInput {
                    inspection_token: inspection.inspection_token,
                    confirmed: false,
                },
            )
            .await
            .expect_err("confirmation required");
            assert!(matches!(error, AppError::Validation { field, .. } if field == "confirmed"));
            assert_eq!(sha256_file(&live), before);
            drop(state);
            cleanup_state_files(&live);
            let _ = fs::remove_file(destination);
        });
    }

    #[test]
    fn exclusive_restore_waits_for_ordinary_commands_then_blocks_later_work() {
        tauri::async_runtime::block_on(async {
            let (state, live) = onboarded_state("exclusive-restore").await;
            let destination = live.with_extension(BACKUP_EXTENSION);
            crate::application::backup_service::create_backup(
                &state,
                CreateBackupInput {
                    destination_path: destination.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                },
            )
            .await
            .expect("backup");
            let inspection = inspect_backup(
                &state,
                InspectBackupInput {
                    source_path: destination.to_string_lossy().into_owned(),
                },
            )
            .await
            .expect("inspect");
            {
                let shared = state.acquire_shared_operation().await.expect("shared");
                let restore_fut = restore_backup(
                    &state,
                    RestoreBackupInput {
                        inspection_token: inspection.inspection_token,
                        confirmed: true,
                    },
                );
                tokio::pin!(restore_fut);
                assert!(
                    tokio::time::timeout(Duration::from_millis(50), &mut restore_fut)
                        .await
                        .is_err(),
                    "exclusive restore must wait for ordinary work"
                );
                drop(shared);
                restore_fut.await.expect("restore after quiescence");
            }
            let later = get_settings(&state).await;
            assert!(matches!(
                later,
                Err(AppError::AppRestartRequired {
                    reason: RestartReason::Restore
                })
            ));
            drop(state);
            cleanup_state_files(&live);
            let _ = fs::remove_file(destination);
        });
    }

    #[test]
    fn injected_close_rename_and_fsync_failures_preserve_or_recover_current_data() {
        tauri::async_runtime::block_on(async {
            for fault in [
                RestoreFault::Close,
                RestoreFault::Rename,
                RestoreFault::Fsync,
            ] {
                let (state, live) = onboarded_state(&format!("fault-{fault:?}")).await;
                let destination = live.with_extension(BACKUP_EXTENSION);
                crate::application::backup_service::create_backup(
                    &state,
                    CreateBackupInput {
                        destination_path: destination.to_string_lossy().into_owned(),
                        overwrite_confirmed: false,
                    },
                )
                .await
                .expect("backup");
                let inspection = inspect_backup(
                    &state,
                    InspectBackupInput {
                        source_path: destination.to_string_lossy().into_owned(),
                    },
                )
                .await
                .expect("inspect");
                state.set_restore_fault(fault);
                let result = restore_backup(
                    &state,
                    RestoreBackupInput {
                        inspection_token: inspection.inspection_token,
                        confirmed: true,
                    },
                )
                .await;
                assert!(result.is_err(), "{fault:?} must fail");
                let recovered = connect_writable(&live, false)
                    .await
                    .expect("current or recovered database should open");
                let name: String = sqlx::query_scalar("SELECT name FROM households LIMIT 1")
                    .fetch_one(&recovered)
                    .await
                    .expect("household");
                recovered.close().await;
                assert_eq!(
                    name, "Wang Family",
                    "{fault:?} must keep verified current household data"
                );
                assert!(matches!(
                    state.writable_db(),
                    Err(AppError::AppRestartRequired {
                        reason: RestartReason::Restore
                    })
                ));
                drop(state);
                cleanup_state_files(&live);
                let _ = fs::remove_file(destination);
            }
        });
    }

    #[test]
    fn successful_restore_requires_restart_and_next_bootstrap_reads_fixture_goldens() {
        tauri::async_runtime::block_on(async {
            let (state, live) = onboarded_state("restore-goldens").await;
            let fixture_path = test_path("phase6", "golden-src");
            let pool = load_schema_fixture(&fixture_path, 4).await;
            pool.close().await;
            let bundle = fixture_path.with_extension(BACKUP_EXTENSION);
            let before_bundle = {
                let _ = wrap_sqlite_as_backup(&fixture_path, &bundle).await;
                file_bytes(&bundle)
            };
            inspect_and_restore(&state, &bundle)
                .await
                .expect("restore schema 4");
            assert_eq!(file_bytes(&bundle), before_bundle);
            let later = get_settings(&state).await;
            assert!(matches!(
                later,
                Err(AppError::AppRestartRequired {
                    reason: RestartReason::Restore
                })
            ));
            drop(state);

            let reopened = AppState::initialize(live.clone()).await;
            let database = reopened.writable_db().expect("restored database");
            assert_eq!(read_migration_version(&live).await.expect("version"), 5);
            let origin: (String, i64, String) = sqlx::query_as(
                "SELECT timezone, timezone_confirmed, origin_local_date FROM history_origins LIMIT 1",
            )
            .fetch_one(database)
            .await
            .expect("origin");
            assert_eq!(origin.0, "Asia/Singapore");
            assert_eq!(origin.1, 1);
            assert_eq!(origin.2, "2026-01-02");
            let declaration_counts: Vec<i64> = sqlx::query_scalar(
                "SELECT COUNT(*) FROM cost_basis_declarations WHERE is_revocation = 0
                 UNION ALL
                 SELECT COUNT(*) FROM cost_basis_declarations WHERE is_revocation = 1",
            )
            .fetch_all(database)
            .await
            .expect("declarations");
            assert_eq!(declaration_counts, vec![2, 1]);
            let policies: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM freshness_policies")
                .fetch_one(database)
                .await
                .expect("policies");
            assert!(policies >= 4);
            drop(reopened);
            cleanup_state_files(&live);
            cleanup_state_files(&fixture_path);
            let _ = fs::remove_file(bundle);
        });
    }

    #[test]
    fn restore_round_trip_preserves_media_and_immutable_evidence() {
        tauri::async_runtime::block_on(async {
            let (state, live) = onboarded_state("restore-media-evidence").await;
            let fixture_path = test_path("phase11", "media-evidence-source");
            let pool = load_schema_fixture(&fixture_path, 5).await;
            let (household_id, member_id): (String, String) = sqlx::query_as(
                "SELECT h.id, m.id
                 FROM households h
                 JOIN members m ON m.household_id = h.id
                 ORDER BY m.id
                 LIMIT 1",
            )
            .fetch_one(&pool)
            .await
            .expect("fixture member");
            let media_id = Uuid::now_v7().to_string();
            let media_bytes = vec![0_u8, 1, 2, 3, 255, 254];
            sqlx::query(
                "INSERT INTO media_assets (id, household_id, mime_type, data, created_at)
                 VALUES (?, ?, 'image/png', ?, '2026-08-20T00:00:00.000Z')",
            )
            .bind(&media_id)
            .bind(&household_id)
            .bind(&media_bytes)
            .execute(&pool)
            .await
            .expect("media");
            sqlx::query("UPDATE members SET avatar_asset_id = ? WHERE id = ?")
                .bind(&media_id)
                .bind(&member_id)
                .execute(&pool)
                .await
                .expect("member media reference");
            let evidence = immutable_evidence(&pool).await;
            pool.close().await;

            let bundle = fixture_path.with_extension(BACKUP_EXTENSION);
            wrap_sqlite_as_backup(&fixture_path, &bundle).await;
            inspect_and_restore(&state, &bundle)
                .await
                .expect("restore media fixture");
            drop(state);

            let reopened = AppState::initialize(live.clone()).await;
            let database = reopened.writable_db().expect("restored database");
            let restored_media: Vec<u8> =
                sqlx::query_scalar("SELECT data FROM media_assets WHERE id = ?")
                    .bind(&media_id)
                    .fetch_one(database)
                    .await
                    .expect("restored media");
            assert_eq!(restored_media, media_bytes);
            let restored_reference: String =
                sqlx::query_scalar("SELECT avatar_asset_id FROM members WHERE id = ?")
                    .bind(&member_id)
                    .fetch_one(database)
                    .await
                    .expect("restored media reference");
            assert_eq!(restored_reference, media_id);
            assert_eq!(immutable_evidence(database).await, evidence);
            drop(reopened);

            cleanup_state_files(&live);
            cleanup_state_files(&fixture_path);
            let _ = fs::remove_file(bundle);
        });
    }

    #[test]
    fn recovery_backups_are_listed_inspected_and_restorable_without_paths() {
        tauri::async_runtime::block_on(async {
            let (state, live) = onboarded_state("recovery-list").await;
            let destination = live.with_extension(BACKUP_EXTENSION);
            crate::application::backup_service::create_backup(
                &state,
                CreateBackupInput {
                    destination_path: destination.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                },
            )
            .await
            .expect("user backup");
            inspect_and_restore(&state, &destination)
                .await
                .expect("first restore");
            drop(state);

            let reopened = AppState::initialize(live.clone()).await;
            let listed = list_recovery_backups(&reopened)
                .await
                .expect("list recovery");
            assert_eq!(listed.items.len(), 1);
            assert_eq!(listed.explanation, RECOVERY_BACKUP_EXPLANATION);
            assert!(!listed.explanation.contains('/'));
            let recovery_id = listed.items[0].id.clone();
            let inspection = inspect_recovery_backup(
                &reopened,
                InspectRecoveryBackupInput {
                    backup_id: recovery_id,
                },
            )
            .await
            .expect("inspect recovery");
            assert!(inspection.checksum_valid);
            assert!(!inspection.inspection_token.contains('/'));
            restore_backup(
                &reopened,
                RestoreBackupInput {
                    inspection_token: inspection.inspection_token,
                    confirmed: true,
                },
            )
            .await
            .expect("restore recovery backup");
            drop(reopened);
            cleanup_state_files(&live);
            let _ = fs::remove_file(destination);
        });
    }

    #[test]
    fn delete_all_data_removes_recovery_copies_but_not_user_selected_backups() {
        tauri::async_runtime::block_on(async {
            let (state, live) = onboarded_state("reset-recovery").await;
            let user_backup =
                test_path("phase6", "user-selected-backup").with_extension(BACKUP_EXTENSION);
            crate::application::backup_service::create_backup(
                &state,
                CreateBackupInput {
                    destination_path: user_backup.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                },
            )
            .await
            .expect("user backup");
            inspect_and_restore(&state, &user_backup)
                .await
                .expect("restore to create recovery");
            drop(state);

            let reopened = AppState::initialize(live.clone()).await;
            let listed = list_recovery_backups(&reopened)
                .await
                .expect("recovery exists");
            assert_eq!(listed.items.len(), 1);
            delete_all_data(&reopened, DeleteAllDataInput { confirmed: true })
                .await
                .expect("reset");
            assert!(user_backup.is_file(), "user backup must survive reset");
            let parent = live.parent().expect("parent");
            let leftover: Vec<_> = fs::read_dir(parent)
                .expect("dir")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| {
                    name.starts_with(&format!(
                        "{}.recovery-",
                        live.file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("")
                    ))
                })
                .collect();
            assert!(
                leftover.is_empty(),
                "recovery copies must be removed: {leftover:?}"
            );
            drop(reopened);
            let _ = fs::remove_file(user_backup);
            cleanup_state_files(&live);
        });
    }

    #[test]
    fn structurally_invalid_archive_is_rejected_without_touching_sqlite() {
        tauri::async_runtime::block_on(async {
            let (state, live) = onboarded_state("struct-invalid").await;
            let before = sha256_file(&live);
            let bundle = live.with_extension(BACKUP_EXTENSION);
            let file = File::create(&bundle).expect("archive");
            let mut writer = ZipWriter::new(file);
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            writer
                .start_file(MANIFEST_ENTRY_NAME, options)
                .expect("manifest");
            writer.write_all(b"{}").expect("bytes");
            writer.finish().expect("finish");
            let result = inspect_and_restore(&state, &bundle).await;
            assert!(result.is_err());
            assert_eq!(sha256_file(&live), before);
            drop(state);
            cleanup_state_files(&live);
            let _ = fs::remove_file(bundle);
        });
    }
}
