use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use sqlx::Row;
use uuid::Uuid;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

use crate::{
    domain::{BackupFormatVersion, BackupId, Timestamp},
    error::AppError,
    infrastructure::{
        database::{connect_read_only, SqlitePool},
        database_bootstrap::max_supported_migration,
    },
    state::{AppState, StoredBackupInspection},
};

pub const BACKUP_EXTENSION: &str = "nestworth-backup";
pub const BACKUP_FORMAT_ID: &str = "com.nestworth.backup";
pub const MANIFEST_ENTRY_NAME: &str = "manifest.json";
pub const DATABASE_ENTRY_NAME: &str = "database.sqlite3";
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
pub const MAX_DATABASE_BYTES: u64 = 512 * 1024 * 1024;
const INSPECTION_TOKEN_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateBackupInput {
    pub destination_path: String,
    pub overwrite_confirmed: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InspectBackupInput {
    pub source_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupManifestDto {
    pub format_id: String,
    pub format_version: String,
    pub backup_id: String,
    pub product_version: String,
    pub database_migration_version: i32,
    pub created_at: String,
    pub household_id: String,
    pub household_name: String,
    pub database_byte_length: i32,
    pub database_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupInspectionDto {
    pub inspection_token: String,
    pub manifest: BackupManifestDto,
    pub checksum_valid: bool,
    pub database_valid: bool,
    pub encrypted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileDigest {
    pub byte_length: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatabaseFacts {
    pub migration: i64,
    pub household_id: Option<String>,
    pub household_name: Option<String>,
    pub database_valid: bool,
}

pub(crate) struct OwnedTempDir {
    path: PathBuf,
}

impl OwnedTempDir {
    pub(crate) fn new(parent: &Path, label: &str) -> Result<Self, AppError> {
        for _ in 0..8 {
            let path = parent.join(format!(".nestworth-{label}-{}", Uuid::now_v7()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_error) => {
                    return Err(AppError::invalid_backup(
                        "Temporary storage is unavailable.",
                    ))
                }
            }
        }
        Err(AppError::invalid_backup(
            "Temporary storage could not be allocated.",
        ))
    }

    pub(crate) fn new_with_prefix(parent: &Path, prefix: &str) -> Result<Self, AppError> {
        for _ in 0..8 {
            let path = parent.join(format!("{prefix}{}", Uuid::now_v7()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_error) => {
                    return Err(AppError::invalid_backup(
                        "Temporary storage is unavailable.",
                    ))
                }
            }
        }
        Err(AppError::invalid_backup(
            "Temporary storage could not be allocated.",
        ))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for OwnedTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct OwnedTempFile {
    path: PathBuf,
    keep: bool,
}

impl OwnedTempFile {
    fn create(parent: &Path, label: &str) -> Result<Self, AppError> {
        for _ in 0..8 {
            let path = parent.join(format!(".nestworth-{label}-{}.tmp", Uuid::now_v7()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    drop(file);
                    return Ok(Self { path, keep: false });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_error) => {
                    return Err(AppError::invalid_backup("Destination is not writable."))
                }
            }
        }
        Err(AppError::invalid_backup(
            "A temporary destination could not be allocated.",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn keep(mut self) {
        self.keep = true;
    }
}

impl Drop for OwnedTempFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub async fn create_backup(
    state: &AppState,
    input: CreateBackupInput,
) -> Result<BackupManifestDto, AppError> {
    let destination = validate_destination(&input.destination_path, input.overwrite_confirmed)?;
    create_verified_backup_to(state, &destination).await
}

pub(crate) async fn create_verified_backup_to(
    state: &AppState,
    destination: &Path,
) -> Result<BackupManifestDto, AppError> {
    let database = state.writable_db()?.clone();
    let database_path = state.database_path().to_path_buf();
    let database_parent = database_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| AppError::invalid_backup("Application storage is unavailable."))?;
    let temporary_directory = OwnedTempDir::new(database_parent, "backup-create")?;
    let temporary_database = temporary_directory.path().join(DATABASE_ENTRY_NAME);

    vacuum_into(&database, &temporary_database).await?;
    let facts = inspect_database_copy(&temporary_database, None).await?;
    if facts.migration != max_supported_migration() {
        return Err(AppError::invalid_backup(
            "Only the active supported database can be backed up.",
        ));
    }
    let household_id = facts
        .household_id
        .ok_or_else(|| AppError::not_found("household", "current"))?;
    let household_name = facts
        .household_name
        .ok_or_else(|| AppError::not_found("household", "current"))?;
    let database_digest = hash_file(&temporary_database)?;
    let manifest = BackupManifestDto {
        format_id: BACKUP_FORMAT_ID.to_owned(),
        format_version: BackupFormatVersion::V1.as_str().to_owned(),
        backup_id: BackupId::new().to_string(),
        product_version: env!("CARGO_PKG_VERSION").to_owned(),
        database_migration_version: i32::try_from(facts.migration)
            .map_err(|_| AppError::invalid_backup("The database migration metadata is invalid."))?,
        created_at: Timestamp::now().to_rfc3339(),
        household_id,
        household_name,
        database_byte_length: i32::try_from(database_digest.byte_length)
            .map_err(|_| AppError::invalid_backup("The database snapshot is too large."))?,
        database_sha256: database_digest.sha256,
    };
    validate_manifest(&manifest)?;

    let destination_parent = path_parent(destination);
    let temporary_bundle = OwnedTempFile::create(destination_parent, "backup-destination")?;
    write_bundle(temporary_bundle.path(), &manifest, &temporary_database)?;
    let readback = read_bundle(temporary_bundle.path(), None)?;
    if readback.manifest != manifest || readback.database_digest != digest_from_manifest(&manifest)
    {
        return Err(AppError::backup_create_failed(
            "The written backup failed verification.",
        ));
    }
    sync_file(temporary_bundle.path())?;
    fs::rename(temporary_bundle.path(), destination).map_err(|_| {
        AppError::backup_create_failed("The backup destination could not be replaced.")
    })?;
    temporary_bundle.keep();
    sync_directory(destination_parent)?;

    tracing::info!(event = "backup.created", "backup created and verified");
    Ok(manifest)
}

pub async fn inspect_backup(
    state: &AppState,
    input: InspectBackupInput,
) -> Result<BackupInspectionDto, AppError> {
    let source = validate_source(&input.source_path)?;
    inspect_backup_at(state, &source.path, source.metadata).await
}

pub(crate) async fn inspect_backup_at(
    state: &AppState,
    source_path: &Path,
    source_metadata: SourceMetadata,
) -> Result<BackupInspectionDto, AppError> {
    let source_parent = state
        .database_path()
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| AppError::invalid_backup("Application storage is unavailable."))?;
    let temporary_directory = OwnedTempDir::new(source_parent, "backup-inspect")?;
    let staged_database = temporary_directory.path().join(DATABASE_ENTRY_NAME);
    let readback = read_bundle(source_path, Some(&staged_database))?;
    let facts = inspect_database_copy(
        &staged_database,
        Some(i64::from(readback.manifest.database_migration_version)),
    )
    .await?;
    if i64::from(readback.manifest.database_migration_version) <= max_supported_migration()
        && (facts.household_id.is_none() || facts.household_name.is_none())
    {
        return Err(AppError::invalid_backup(
            "The backup does not contain a valid Household.",
        ));
    }
    if i64::from(readback.manifest.database_migration_version) <= max_supported_migration()
        && (facts.household_id.as_deref() != Some(readback.manifest.household_id.as_str())
            || facts.household_name.as_deref() != Some(readback.manifest.household_name.as_str()))
    {
        return Err(AppError::invalid_backup(
            "The manifest Household does not match the database.",
        ));
    }

    let after_read = file_metadata(source_path)?;
    if !same_file_metadata(&source_metadata, &after_read) {
        return Err(AppError::invalid_backup(
            "The selected backup changed during inspection.",
        ));
    }
    let source_digest = hash_file(source_path)?;
    let token = state.issue_backup_inspection(StoredBackupInspection {
        canonical_path: source_path.to_path_buf(),
        file_size: after_read.len,
        modified_at: after_read.modified,
        file_device: after_read.device,
        file_inode: after_read.inode,
        sha256: source_digest.sha256,
        expires_at: std::time::Instant::now() + INSPECTION_TOKEN_TTL,
    });

    let checksum_valid = readback.database_digest.sha256 == readback.manifest.database_sha256
        && readback.database_digest.byte_length
            == u64::try_from(readback.manifest.database_byte_length).unwrap_or_default();
    let manifest = readback.manifest;
    Ok(BackupInspectionDto {
        inspection_token: token,
        manifest,
        checksum_valid,
        database_valid: facts.database_valid,
        encrypted: false,
    })
}

fn validate_destination(raw_path: &str, overwrite_confirmed: bool) -> Result<PathBuf, AppError> {
    if raw_path.trim().is_empty() {
        return Err(AppError::validation(
            "destinationPath",
            "A backup destination is required.",
        ));
    }
    let path = PathBuf::from(raw_path);
    if !has_backup_extension(&path) {
        return Err(AppError::invalid_backup(
            "The destination must use the .nestworth-backup extension.",
        ));
    }
    let parent = path_parent(&path);
    let parent_metadata = fs::metadata(parent)
        .map_err(|_| AppError::invalid_backup("The destination folder is unavailable."))?;
    if !parent_metadata.is_dir() {
        return Err(AppError::invalid_backup(
            "The destination folder is unavailable.",
        ));
    }
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || metadata.is_dir() {
                return Err(AppError::invalid_backup(
                    "The destination is not a regular file.",
                ));
            }
            if !overwrite_confirmed {
                return Err(AppError::validation(
                    "overwriteConfirmed",
                    "Overwriting an existing backup requires explicit confirmation.",
                ));
            }
            Ok(path)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path),
        Err(_error) => Err(AppError::invalid_backup(
            "The destination could not be inspected.",
        )),
    }
}

fn validate_source(raw_path: &str) -> Result<SourceFile, AppError> {
    if raw_path.trim().is_empty() {
        return Err(AppError::validation(
            "sourcePath",
            "A backup source is required.",
        ));
    }
    let raw = PathBuf::from(raw_path);
    if !has_backup_extension(&raw) {
        return Err(AppError::invalid_backup(
            "The selected file is not a Nestworth backup.",
        ));
    }
    let metadata = fs::symlink_metadata(&raw)
        .map_err(|_| AppError::invalid_backup("The selected backup is unavailable."))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::invalid_backup(
            "The selected backup is not a regular file.",
        ));
    }
    let path = fs::canonicalize(&raw)
        .map_err(|_| AppError::invalid_backup("The selected backup is unavailable."))?;
    let metadata = file_metadata(&path)?;
    Ok(SourceFile { path, metadata })
}

fn has_backup_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == BACKUP_EXTENSION)
}

fn path_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[derive(Debug, Clone)]
pub(crate) struct SourceMetadata {
    pub len: u64,
    pub modified: SystemTime,
    pub device: u64,
    pub inode: u64,
}

struct SourceFile {
    path: PathBuf,
    metadata: SourceMetadata,
}

pub(crate) fn file_metadata(path: &Path) -> Result<SourceMetadata, AppError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| AppError::invalid_backup("The selected backup is unavailable."))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::invalid_backup(
            "The selected backup is not a regular file.",
        ));
    }
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Ok(SourceMetadata {
        len: metadata.len(),
        modified: metadata.modified().map_err(|_| {
            AppError::invalid_backup("The selected backup metadata is unavailable.")
        })?,
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(not(unix))]
        device: 0,
        #[cfg(unix)]
        inode: metadata.ino(),
        #[cfg(not(unix))]
        inode: 0,
    })
}

pub(crate) fn same_file_metadata(first: &SourceMetadata, second: &SourceMetadata) -> bool {
    first.len == second.len
        && first.modified == second.modified
        && first.device == second.device
        && first.inode == second.inode
}

async fn vacuum_into(database: &SqlitePool, destination: &Path) -> Result<(), AppError> {
    let literal = sqlite_string_literal(destination)?;
    let statement = format!("VACUUM INTO {literal}");
    sqlx::query(&statement)
        .execute(database)
        .await
        .map_err(|_| AppError::invalid_backup("The database snapshot could not be created."))?;
    Ok(())
}

fn sqlite_string_literal(path: &Path) -> Result<String, AppError> {
    let value = path
        .to_str()
        .ok_or_else(|| AppError::invalid_backup("Application storage is unavailable."))?;
    if value.contains('\0') {
        return Err(AppError::invalid_backup(
            "Application storage is unavailable.",
        ));
    }
    Ok(format!("'{}'", value.replace('\'', "''")))
}

pub(crate) async fn inspect_database_copy(
    path: &Path,
    expected_migration: Option<i64>,
) -> Result<DatabaseFacts, AppError> {
    let pool = connect_read_only(path)
        .await
        .map_err(|_| AppError::invalid_backup("The database entry is not readable."))?;
    let result = inspect_database_copy_in_pool(&pool, expected_migration).await;
    pool.close().await;
    result
}

async fn inspect_database_copy_in_pool(
    pool: &SqlitePool,
    expected_migration: Option<i64>,
) -> Result<DatabaseFacts, AppError> {
    sqlx::query("PRAGMA query_only = ON")
        .execute(pool)
        .await
        .map_err(|_| AppError::backup_corrupt("The database entry is not readable."))?;
    sqlx::query("PRAGMA trusted_schema = OFF")
        .execute(pool)
        .await
        .map_err(|_| AppError::backup_corrupt("The database entry is not readable."))?;

    reject_attached_schemas(pool).await?;
    reject_unexpected_schema_kinds(pool).await?;

    let migration = read_migration_version_in_pool(pool).await?;
    if migration <= 0 {
        return Err(AppError::invalid_backup(
            "The database migration metadata is invalid.",
        ));
    }
    if let Some(expected) = expected_migration {
        if migration != expected {
            return Err(AppError::invalid_backup(
                "The database migration metadata does not match the manifest.",
            ));
        }
    }

    if migration <= max_supported_migration() {
        verify_schema_fingerprint(pool, migration).await?;
    }

    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(pool)
        .await
        .map_err(|_| AppError::backup_corrupt("The database entry is not readable."))?;
    if !integrity.eq_ignore_ascii_case("ok") {
        return Err(AppError::backup_corrupt(
            "The database integrity check failed.",
        ));
    }
    let foreign_key_violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await
        .map_err(|_| AppError::backup_corrupt("The database foreign-key check failed."))?;
    if !foreign_key_violations.is_empty() {
        return Err(AppError::backup_corrupt(
            "The database foreign-key check failed.",
        ));
    }

    if migration <= max_supported_migration() {
        let household =
            sqlx::query("SELECT id, name FROM households ORDER BY created_at, id LIMIT 1")
                .fetch_optional(pool)
                .await
                .map_err(|_| AppError::invalid_backup("The Household record is invalid."))?
                .ok_or_else(|| AppError::invalid_backup("The database has no Household."))?;
        let household_id = household
            .try_get::<String, _>("id")
            .map_err(|_| AppError::invalid_backup("The Household record is invalid."))?;
        let household_name = household
            .try_get::<String, _>("name")
            .map_err(|_| AppError::invalid_backup("The Household record is invalid."))?;
        if household_id.trim().is_empty() || household_name.trim().is_empty() {
            return Err(AppError::invalid_backup("The Household record is invalid."));
        }
        Ok(DatabaseFacts {
            migration,
            household_id: Some(household_id),
            household_name: Some(household_name),
            database_valid: true,
        })
    } else {
        Ok(DatabaseFacts {
            migration,
            household_id: None,
            household_name: None,
            database_valid: true,
        })
    }
}

async fn read_migration_version_in_pool(pool: &SqlitePool) -> Result<i64, AppError> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations' LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| AppError::invalid_backup("The database migration metadata is invalid."))?;
    if exists.is_none() {
        return Err(AppError::invalid_backup(
            "The database migration metadata is invalid.",
        ));
    }
    sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1")
        .fetch_one(pool)
        .await
        .map_err(|_| AppError::invalid_backup("The database migration metadata is invalid."))
}

async fn reject_attached_schemas(pool: &SqlitePool) -> Result<(), AppError> {
    let rows = sqlx::query("PRAGMA database_list")
        .fetch_all(pool)
        .await
        .map_err(|_| AppError::restore_validation_failed("attached_schema"))?;
    for row in rows {
        let name = row
            .try_get::<String, _>("name")
            .map_err(|_| AppError::restore_validation_failed("attached_schema"))?;
        if name != "main" && name != "temp" {
            return Err(AppError::restore_validation_failed("attached_schema"));
        }
    }
    Ok(())
}

async fn reject_unexpected_schema_kinds(pool: &SqlitePool) -> Result<(), AppError> {
    let unexpected: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT type, name, sql FROM sqlite_master
         WHERE name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| AppError::restore_validation_failed("unexpected_object"))?;
    for (kind, _name, sql) in unexpected {
        let virtual_table = sql
            .as_deref()
            .is_some_and(|value| value.to_ascii_uppercase().contains("VIRTUAL TABLE"));
        if kind == "trigger" {
            return Err(AppError::restore_validation_failed("trigger"));
        }
        if kind == "view" {
            return Err(AppError::restore_validation_failed("view"));
        }
        if kind == "table" && virtual_table {
            return Err(AppError::restore_validation_failed("virtual_table"));
        }
        if kind != "table" && kind != "index" {
            return Err(AppError::restore_validation_failed("unexpected_object"));
        }
    }
    Ok(())
}

pub(crate) async fn verify_schema_fingerprint(
    pool: &SqlitePool,
    migration: i64,
) -> Result<(), AppError> {
    let expected = expected_schema_objects_for(migration)
        .ok_or_else(|| AppError::restore_validation_failed("unsupported_schema"))?;
    let rows = sqlx::query(
        "SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| AppError::restore_validation_failed("unsupported_schema"))?;
    let actual = rows
        .into_iter()
        .map(|row| {
            Ok::<_, AppError>((
                row.try_get::<String, _>("type")
                    .map_err(|_| AppError::restore_validation_failed("unsupported_schema"))?,
                row.try_get::<String, _>("name")
                    .map_err(|_| AppError::restore_validation_failed("unsupported_schema"))?,
            ))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if actual != expected {
        return Err(AppError::restore_validation_failed("unsupported_schema"));
    }
    Ok(())
}

fn schema_objects(tables: &[&str], indexes: &[&str]) -> BTreeSet<(String, String)> {
    tables
        .iter()
        .map(|name| ("table".to_owned(), (*name).to_owned()))
        .chain(
            indexes
                .iter()
                .map(|name| ("index".to_owned(), (*name).to_owned())),
        )
        .collect()
}

pub(crate) fn expected_schema_objects_for(migration: i64) -> Option<BTreeSet<(String, String)>> {
    Some(match migration {
        1 => schema_objects(
            &[
                "_sqlx_migrations",
                "account_groups",
                "account_ownership",
                "account_values",
                "accounts",
                "app_settings",
                "households",
                "institutions",
                "media_assets",
                "members",
            ],
            &[
                "idx_account_values_latest",
                "idx_accounts_category",
                "idx_accounts_group",
                "idx_accounts_household",
                "idx_accounts_institution",
                "idx_groups_household",
                "idx_institutions_household",
                "idx_members_household",
                "idx_ownership_member",
            ],
        ),
        2 => schema_objects(
            &[
                "_sqlx_migrations",
                "account_cash_values",
                "account_groups",
                "account_ownership",
                "account_values",
                "accounts",
                "app_settings",
                "fx_quote_preferences",
                "fx_quotes",
                "holdings",
                "households",
                "institutions",
                "instrument_quotes",
                "instruments",
                "media_assets",
                "members",
            ],
            &[
                "idx_account_cash_values_latest",
                "idx_account_values_latest",
                "idx_accounts_category",
                "idx_accounts_group",
                "idx_accounts_household",
                "idx_accounts_institution",
                "idx_fx_quotes_latest",
                "idx_groups_household",
                "idx_holdings_account",
                "idx_holdings_active_pair",
                "idx_holdings_instrument",
                "idx_institutions_household",
                "idx_instrument_quotes_latest",
                "idx_instruments_household",
                "idx_instruments_provider_identity",
                "idx_members_household",
                "idx_ownership_member",
            ],
        ),
        3 => schema_objects(
            &[
                "_sqlx_migrations",
                "account_cash_values",
                "account_groups",
                "account_ownership",
                "account_state_observations",
                "account_state_ownership",
                "account_values",
                "accounts",
                "activities",
                "activity_legs",
                "app_settings",
                "daily_valuation_snapshot_items",
                "daily_valuation_snapshots",
                "fx_preference_observations",
                "fx_quote_preferences",
                "fx_quotes",
                "history_origin_account_states",
                "history_origin_account_values",
                "history_origin_cash_values",
                "history_origin_holdings",
                "history_origin_ownership",
                "history_origins",
                "history_snapshot_state",
                "holding_quantity_values",
                "holding_state_observations",
                "holdings",
                "households",
                "institutions",
                "instrument_preference_observations",
                "instrument_quotes",
                "instruments",
                "media_assets",
                "members",
            ],
            &[
                "idx_account_cash_values_activity",
                "idx_account_cash_values_latest",
                "idx_account_state_observations_latest",
                "idx_account_state_ownership_member",
                "idx_account_values_activity",
                "idx_account_values_latest",
                "idx_accounts_category",
                "idx_accounts_group",
                "idx_accounts_household",
                "idx_accounts_institution",
                "idx_activities_correction_group",
                "idx_activities_corrects",
                "idx_activities_household_cursor",
                "idx_activities_reverses",
                "idx_activity_legs_account",
                "idx_activity_legs_activity",
                "idx_activity_legs_instrument",
                "idx_daily_valuation_snapshot_items_account",
                "idx_daily_valuation_snapshot_items_instrument",
                "idx_daily_valuation_snapshots_latest",
                "idx_daily_valuation_snapshots_revision",
                "idx_fx_preference_observations_latest",
                "idx_fx_quotes_latest",
                "idx_groups_household",
                "idx_history_origins_household",
                "idx_holding_quantity_values_activity",
                "idx_holding_quantity_values_latest",
                "idx_holding_state_observations_latest",
                "idx_holdings_account",
                "idx_holdings_active_pair",
                "idx_holdings_instrument",
                "idx_institutions_household",
                "idx_instrument_preference_observations_latest",
                "idx_instrument_quotes_latest",
                "idx_instruments_household",
                "idx_instruments_provider_identity",
                "idx_members_household",
                "idx_ownership_member",
            ],
        ),
        4 => schema_objects(
            &[
                "_sqlx_migrations",
                "account_cash_values",
                "account_groups",
                "account_ownership",
                "account_state_observations",
                "account_state_ownership",
                "account_values",
                "accounts",
                "activities",
                "activity_legs",
                "app_settings",
                "cost_basis_declarations",
                "daily_valuation_snapshot_items",
                "daily_valuation_snapshots",
                "fx_preference_observations",
                "fx_quote_preferences",
                "fx_quotes",
                "history_origin_account_states",
                "history_origin_account_values",
                "history_origin_cash_values",
                "history_origin_holdings",
                "history_origin_ownership",
                "history_origins",
                "history_snapshot_state",
                "holding_quantity_values",
                "holding_state_observations",
                "holdings",
                "households",
                "institutions",
                "instrument_preference_observations",
                "instrument_quotes",
                "instruments",
                "media_assets",
                "members",
            ],
            &[
                "idx_account_cash_values_activity",
                "idx_account_cash_values_latest",
                "idx_account_state_observations_latest",
                "idx_account_state_ownership_member",
                "idx_account_values_activity",
                "idx_account_values_latest",
                "idx_accounts_category",
                "idx_accounts_group",
                "idx_accounts_household",
                "idx_accounts_institution",
                "idx_activities_correction_group",
                "idx_activities_corrects",
                "idx_activities_household_cursor",
                "idx_activities_reverses",
                "idx_activity_legs_account",
                "idx_activity_legs_activity",
                "idx_activity_legs_instrument",
                "idx_cost_basis_declarations_household",
                "idx_cost_basis_declarations_leg_lot",
                "idx_cost_basis_declarations_origin_lot",
                "idx_daily_valuation_snapshot_items_account",
                "idx_daily_valuation_snapshot_items_instrument",
                "idx_daily_valuation_snapshots_latest",
                "idx_daily_valuation_snapshots_revision",
                "idx_fx_preference_observations_latest",
                "idx_fx_quotes_latest",
                "idx_groups_household",
                "idx_history_origins_household",
                "idx_holding_quantity_values_activity",
                "idx_holding_quantity_values_latest",
                "idx_holding_state_observations_latest",
                "idx_holdings_account",
                "idx_holdings_active_pair",
                "idx_holdings_instrument",
                "idx_institutions_household",
                "idx_instrument_preference_observations_latest",
                "idx_instrument_quotes_latest",
                "idx_instruments_household",
                "idx_instruments_provider_identity",
                "idx_members_household",
                "idx_ownership_member",
            ],
        ),
        5 => expected_schema_objects_v5(),
        6 => expected_schema_objects(),
        _ => return None,
    })
}

fn expected_schema_objects_v5() -> BTreeSet<(String, String)> {
    let tables = [
        "_sqlx_migrations",
        "account_groups",
        "account_ownership",
        "account_state_observations",
        "account_state_ownership",
        "account_values",
        "account_cash_values",
        "accounts",
        "activities",
        "activity_legs",
        "app_settings",
        "benchmark_observations",
        "benchmarks",
        "cost_basis_declarations",
        "daily_valuation_snapshot_items",
        "daily_valuation_snapshots",
        "fx_preference_observations",
        "fx_quote_preferences",
        "fx_quotes",
        "freshness_policies",
        "history_origin_account_states",
        "history_origin_account_values",
        "history_origin_cash_values",
        "history_origin_holdings",
        "history_origin_ownership",
        "history_origins",
        "history_snapshot_state",
        "holding_quantity_values",
        "holding_state_observations",
        "holdings",
        "household_benchmark_preferences",
        "households",
        "import_batches",
        "import_items",
        "institutions",
        "instrument_preference_observations",
        "instrument_quotes",
        "instruments",
        "maintenance_snoozes",
        "media_assets",
        "members",
        "pending_activities",
        "recurring_activity_rules",
    ];
    let indexes = [
        "idx_account_cash_values_activity",
        "idx_account_cash_values_latest",
        "idx_account_state_observations_latest",
        "idx_account_state_ownership_member",
        "idx_account_values_activity",
        "idx_account_values_latest",
        "idx_accounts_category",
        "idx_accounts_group",
        "idx_accounts_household",
        "idx_accounts_institution",
        "idx_activities_correction_group",
        "idx_activities_corrects",
        "idx_activities_household_cursor",
        "idx_activities_reverses",
        "idx_activity_legs_account",
        "idx_activity_legs_activity",
        "idx_activity_legs_instrument",
        "idx_benchmark_observations_import",
        "idx_benchmark_observations_selection",
        "idx_benchmarks_household",
        "idx_cost_basis_declarations_household",
        "idx_cost_basis_declarations_leg_lot",
        "idx_cost_basis_declarations_origin_lot",
        "idx_daily_valuation_snapshot_items_account",
        "idx_daily_valuation_snapshot_items_instrument",
        "idx_daily_valuation_snapshots_latest",
        "idx_daily_valuation_snapshots_revision",
        "idx_fx_preference_observations_latest",
        "idx_fx_quotes_latest",
        "idx_freshness_policies_resolution",
        "idx_groups_household",
        "idx_history_origins_household",
        "idx_holding_quantity_values_activity",
        "idx_holding_quantity_values_latest",
        "idx_holding_state_observations_latest",
        "idx_holdings_account",
        "idx_holdings_instrument",
        "idx_holdings_active_pair",
        "idx_household_benchmark_preferences_benchmark",
        "idx_import_batches_household",
        "idx_import_items_batch_row",
        "idx_import_items_identity",
        "idx_import_items_target",
        "idx_instrument_preference_observations_latest",
        "idx_instrument_quotes_latest",
        "idx_instruments_household",
        "idx_instruments_provider_identity",
        "idx_institutions_household",
        "idx_maintenance_snoozes_lookup",
        "idx_members_household",
        "idx_ownership_member",
        "idx_pending_activities_due",
        "idx_pending_activities_posted_activity",
        "idx_pending_activities_rule",
        "idx_recurring_activity_rules_due",
        "idx_recurring_activity_rules_updated",
        "uq_freshness_policies_account_target",
        "uq_freshness_policies_default",
        "uq_freshness_policies_fx_target",
        "uq_freshness_policies_instrument_target",
    ];
    tables
        .into_iter()
        .map(|name| ("table".to_owned(), name.to_owned()))
        .chain(
            indexes
                .into_iter()
                .map(|name| ("index".to_owned(), name.to_owned())),
        )
        .collect()
}

fn expected_schema_objects() -> BTreeSet<(String, String)> {
    let mut objects = expected_schema_objects_v5();
    objects.insert(("table".to_owned(), "market_data_daily_coverage".to_owned()));
    objects.insert((
        "index".to_owned(),
        "idx_market_data_daily_coverage_lookup".to_owned(),
    ));
    objects
}

pub(crate) fn write_bundle(
    destination: &Path,
    manifest: &BackupManifestDto,
    database: &Path,
) -> Result<(), AppError> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(destination)
        .map_err(|_| AppError::invalid_backup("The temporary destination is not writable."))?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    let manifest_bytes = serde_json::to_vec(manifest)
        .map_err(|_| AppError::invalid_backup("The backup manifest could not be serialized."))?;
    writer
        .start_file(MANIFEST_ENTRY_NAME, options)
        .map_err(|_| AppError::invalid_backup("The backup archive could not be written."))?;
    writer
        .write_all(&manifest_bytes)
        .map_err(|_| AppError::invalid_backup("The backup archive could not be written."))?;
    writer
        .start_file(DATABASE_ENTRY_NAME, options)
        .map_err(|_| AppError::invalid_backup("The backup archive could not be written."))?;
    let mut source = File::open(database)
        .map_err(|_| AppError::invalid_backup("The database snapshot could not be read."))?;
    io::copy(&mut source, &mut writer)
        .map_err(|_| AppError::invalid_backup("The backup archive could not be written."))?;
    let file = writer
        .finish()
        .map_err(|_| AppError::invalid_backup("The backup archive could not be finalized."))?;
    file.sync_all()
        .map_err(|_| AppError::invalid_backup("The backup archive could not be synchronized."))?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleReadback {
    pub manifest: BackupManifestDto,
    pub database_digest: FileDigest,
}

pub(crate) fn read_bundle(path: &Path, staging: Option<&Path>) -> Result<BundleReadback, AppError> {
    let file = File::open(path)
        .map_err(|_| AppError::invalid_backup("The selected backup is not readable."))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|_| AppError::invalid_backup("The selected backup is not a valid ZIP archive."))?;
    if archive.len() != 2 {
        return Err(AppError::invalid_backup(
            "The backup must contain exactly two entries.",
        ));
    }

    let mut seen = BTreeSet::new();
    let mut manifest = None;
    let mut database_digest = None;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| AppError::invalid_backup("The backup entry could not be read."))?;
        let name = entry.name().to_owned();
        if !valid_archive_entry_name(&name) || !seen.insert(name.clone()) {
            return Err(AppError::invalid_backup(
                "The backup contains an unsafe or duplicate entry.",
            ));
        }
        if entry.encrypted() || !entry.is_file() {
            return Err(AppError::invalid_backup(
                "The backup contains an encrypted or non-regular entry.",
            ));
        }
        if entry.compression() != CompressionMethod::Stored {
            return Err(AppError::invalid_backup(
                "The backup uses an unsupported compression method.",
            ));
        }

        match (index, name.as_str()) {
            (0, MANIFEST_ENTRY_NAME) => {
                let declared = entry.size();
                if declared > MAX_MANIFEST_BYTES {
                    return Err(AppError::invalid_backup(
                        "The backup manifest is too large.",
                    ));
                }
                let bytes = read_bounded(&mut entry, declared, MAX_MANIFEST_BYTES)?;
                let parsed = serde_json::from_slice::<BackupManifestDto>(&bytes)
                    .map_err(|_| AppError::invalid_backup("The backup manifest is invalid."))?;
                validate_manifest(&parsed)?;
                manifest = Some(parsed);
            }
            (1, DATABASE_ENTRY_NAME) => {
                let expected = manifest
                    .as_ref()
                    .ok_or_else(|| AppError::invalid_backup("The backup manifest is missing."))?
                    .database_byte_length;
                let expected = u64::try_from(expected)
                    .map_err(|_| AppError::invalid_backup("The backup manifest is invalid."))?;
                if expected > MAX_DATABASE_BYTES || entry.size() != expected {
                    return Err(AppError::invalid_backup(
                        "The database entry size does not match the manifest.",
                    ));
                }
                let digest = if let Some(staging) = staging {
                    let mut output = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(staging)
                        .map_err(|_| AppError::invalid_backup("Backup staging is unavailable."))?;
                    copy_reader_hash(&mut entry, &mut output, expected)?
                } else {
                    hash_reader(&mut entry, expected)?
                };
                database_digest = Some(digest);
            }
            _ => {
                return Err(AppError::invalid_backup(
                    "The backup entries are not in the required order.",
                ));
            }
        }
    }
    let manifest =
        manifest.ok_or_else(|| AppError::invalid_backup("The backup manifest is missing."))?;
    let database_digest = database_digest
        .ok_or_else(|| AppError::invalid_backup("The database entry is missing."))?;
    if database_digest.byte_length
        != u64::try_from(manifest.database_byte_length).unwrap_or_default()
        || database_digest.sha256 != manifest.database_sha256
    {
        return Err(AppError::invalid_backup(
            "The database checksum does not match the manifest.",
        ));
    }
    Ok(BundleReadback {
        manifest,
        database_digest,
    })
}

fn valid_archive_entry_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains('\\')
        && !name
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
}

fn read_bounded<R: Read>(reader: &mut R, declared: u64, maximum: u64) -> Result<Vec<u8>, AppError> {
    if declared > maximum {
        return Err(AppError::invalid_backup("The archive entry is too large."));
    }
    let capacity = usize::try_from(declared)
        .map_err(|_| AppError::invalid_backup("The archive entry is too large."))?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| AppError::invalid_backup("The archive entry is truncated."))?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| AppError::invalid_backup("The archive entry is too large."))?;
        if total > declared || total > maximum {
            return Err(AppError::invalid_backup("The archive entry is too large."));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    if total != declared {
        return Err(AppError::invalid_backup("The archive entry is truncated."));
    }
    Ok(bytes)
}

fn hash_reader<R: Read>(reader: &mut R, declared: u64) -> Result<FileDigest, AppError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| AppError::invalid_backup("The archive entry is truncated."))?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| AppError::invalid_backup("The archive entry is too large."))?;
        if total > declared || total > MAX_DATABASE_BYTES {
            return Err(AppError::invalid_backup("The archive entry is too large."));
        }
        hasher.update(&buffer[..read]);
    }
    if total != declared {
        return Err(AppError::invalid_backup("The archive entry is truncated."));
    }
    Ok(FileDigest {
        byte_length: total,
        sha256: hex_digest(hasher.finalize()),
    })
}

fn copy_reader_hash<R: Read>(
    reader: &mut R,
    output: &mut File,
    declared: u64,
) -> Result<FileDigest, AppError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| AppError::invalid_backup("The archive entry is truncated."))?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| AppError::invalid_backup("The archive entry is too large."))?;
        if total > declared || total > MAX_DATABASE_BYTES {
            return Err(AppError::invalid_backup("The archive entry is too large."));
        }
        output
            .write_all(&buffer[..read])
            .map_err(|_| AppError::invalid_backup("Backup staging is unavailable."))?;
        hasher.update(&buffer[..read]);
    }
    if total != declared {
        return Err(AppError::invalid_backup("The archive entry is truncated."));
    }
    output
        .sync_all()
        .map_err(|_| AppError::invalid_backup("Backup staging is unavailable."))?;
    Ok(FileDigest {
        byte_length: total,
        sha256: hex_digest(hasher.finalize()),
    })
}

pub(crate) fn hash_file(path: &Path) -> Result<FileDigest, AppError> {
    let mut file = File::open(path)
        .map_err(|_| AppError::invalid_backup("The database snapshot could not be read."))?;
    let metadata = file
        .metadata()
        .map_err(|_| AppError::invalid_backup("The database snapshot metadata is unavailable."))?;
    if metadata.len() > MAX_DATABASE_BYTES {
        return Err(AppError::invalid_backup(
            "The database snapshot is too large.",
        ));
    }
    hash_reader(&mut file, metadata.len())
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn digest_from_manifest(manifest: &BackupManifestDto) -> FileDigest {
    FileDigest {
        byte_length: u64::try_from(manifest.database_byte_length).unwrap_or_default(),
        sha256: manifest.database_sha256.clone(),
    }
}

fn validate_manifest(manifest: &BackupManifestDto) -> Result<(), AppError> {
    if manifest.format_id != BACKUP_FORMAT_ID {
        return Err(AppError::invalid_backup(
            "The backup format is not supported.",
        ));
    }
    BackupFormatVersion::parse(&manifest.format_version)?;
    BackupId::parse(&manifest.backup_id)
        .map_err(|_| AppError::invalid_backup("The backup identifier is invalid."))?;
    if manifest.product_version.trim().is_empty()
        || manifest.database_migration_version <= 0
        || manifest.database_byte_length < 0
        || u64::try_from(manifest.database_byte_length).unwrap_or_default() > MAX_DATABASE_BYTES
        || manifest.household_name.trim().is_empty()
        || !is_sha256(&manifest.database_sha256)
    {
        return Err(AppError::invalid_backup("The backup manifest is invalid."));
    }
    Timestamp::parse(&manifest.created_at)
        .map_err(|_| AppError::invalid_backup("The backup timestamp is invalid."))?;
    crate::domain::HouseholdId::parse(&manifest.household_id)
        .map_err(|_| AppError::invalid_backup("The Household identifier is invalid."))?;
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn sync_file(path: &Path) -> Result<(), AppError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| AppError::invalid_backup("The backup archive could not be synchronized."))
}

pub(crate) fn sync_directory(path: &Path) -> Result<(), AppError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| AppError::invalid_backup("The backup destination could not be synchronized."))
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, OpenOptions},
        io::{Cursor, Write},
        path::PathBuf,
        time::{Duration, SystemTime},
    };

    use sha2::{Digest, Sha256};
    use sqlx::Row;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    use super::{
        copy_reader_hash, digest_from_manifest, expected_schema_objects, hash_reader, read_bundle,
        sqlite_string_literal, validate_destination, validate_manifest, BackupManifestDto,
        BACKUP_EXTENSION, BACKUP_FORMAT_ID, DATABASE_ENTRY_NAME, MANIFEST_ENTRY_NAME,
        MAX_MANIFEST_BYTES,
    };
    use crate::{
        application::onboarding_service::complete_onboarding,
        state::{AppState, StoredBackupInspection},
        test_support::{test_path, valid_onboarding_input},
    };

    fn fixture_manifest() -> BackupManifestDto {
        BackupManifestDto {
            format_id: BACKUP_FORMAT_ID.to_owned(),
            format_version: "1".to_owned(),
            backup_id: "01a0188f-9100-7000-8000-000000000001".to_owned(),
            product_version: "0.1.5".to_owned(),
            database_migration_version: 5,
            created_at: "2026-08-20T00:00:00.000Z".to_owned(),
            household_id: "11111111-1111-4111-8111-111111111111".to_owned(),
            household_name: "Fixture Household".to_owned(),
            database_byte_length: 16,
            database_sha256: "3f8434aff012ffcb594ad7527d6042841ce94f499887f86ab828f84a9a275f91"
                .to_owned(),
        }
    }

    #[test]
    fn manifest_serialization_matches_the_phase_zero_golden_order() {
        let bytes = serde_json::to_vec(&fixture_manifest()).expect("manifest should serialize");
        assert_eq!(
            String::from_utf8(bytes).expect("manifest should be UTF-8"),
            r#"{"formatId":"com.nestworth.backup","formatVersion":"1","backupId":"01a0188f-9100-7000-8000-000000000001","productVersion":"0.1.5","databaseMigrationVersion":5,"createdAt":"2026-08-20T00:00:00.000Z","householdId":"11111111-1111-4111-8111-111111111111","householdName":"Fixture Household","databaseByteLength":16,"databaseSha256":"3f8434aff012ffcb594ad7527d6042841ce94f499887f86ab828f84a9a275f91"}"#
        );
    }

    #[test]
    fn sqlite_literal_escapes_quotes_without_interpolating_a_path_as_sql() {
        let path = PathBuf::from("/tmp/nestworth-'; DROP TABLE households; --.sqlite3");
        let literal = sqlite_string_literal(&path).expect("literal should be generated");
        assert_eq!(
            literal,
            "'/tmp/nestworth-''; DROP TABLE households; --.sqlite3'"
        );
    }

    #[test]
    fn manifest_rejects_wrong_identity_and_oversized_values() {
        let mut manifest = fixture_manifest();
        manifest.format_id = "wrong".to_owned();
        assert!(validate_manifest(&manifest).is_err());
        let mut manifest = fixture_manifest();
        manifest.database_byte_length = i32::try_from(super::MAX_DATABASE_BYTES).unwrap() + 1;
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn archive_reader_requires_the_exact_two_entry_boundary() {
        let root = test_path("phase5", "archive-boundary");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("fixture directory");
        let database = root.join("database.sqlite3");
        fs::write(&database, b"SQLite format 3\0").expect("database fixture");
        let mut manifest = fixture_manifest();
        manifest.database_byte_length = 16;
        let mut hash = Sha256::new();
        hash.update(b"SQLite format 3\0");
        manifest.database_sha256 = super::hex_digest(hash.finalize());
        let archive = root.join("fixture.nestworth-backup");
        let file = fs::File::create(&archive).expect("archive file");
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer
            .start_file(MANIFEST_ENTRY_NAME, options)
            .expect("manifest entry");
        writer
            .write_all(&serde_json::to_vec(&manifest).expect("manifest bytes"))
            .expect("manifest");
        writer
            .start_file(DATABASE_ENTRY_NAME, options)
            .expect("database entry");
        writer.write_all(b"SQLite format 3\0").expect("database");
        writer.start_file("extra", options).expect("extra entry");
        writer.write_all(b"extra").expect("extra");
        writer.finish().expect("archive finish");
        assert!(read_bundle(&archive, None).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_reader_rejects_trailing_decompressed_bytes() {
        let mut reader = Cursor::new(b"abcdef".to_vec());
        assert!(hash_reader(&mut reader, 5).is_err());
        let mut reader = Cursor::new(b"abcdef".to_vec());
        let mut output = tempfile_for_test();
        assert!(copy_reader_hash(&mut reader, &mut output, 5).is_err());
    }

    #[test]
    fn schema_fingerprint_has_no_views_or_triggers() {
        assert!(expected_schema_objects()
            .iter()
            .all(|(kind, _)| kind == "table" || kind == "index"));
    }

    #[test]
    fn inspection_records_are_process_local_and_expire() {
        let state = AppState::unavailable(test_path("phase5", "tokens"));
        let token = state.issue_backup_inspection(StoredBackupInspection {
            canonical_path: PathBuf::from("/app-owned/backup.nestworth-backup"),
            file_size: 1,
            modified_at: SystemTime::now(),
            file_device: 1,
            file_inode: 1,
            sha256: "0".repeat(64),
            expires_at: std::time::Instant::now() + Duration::from_secs(60),
        });
        assert_eq!(token.len(), 36);
        assert_eq!(state.backup_inspection_count(), 1);
        assert!(state.backup_inspection(&token).is_some());
        let _ = digest_from_manifest(&fixture_manifest());
    }

    #[test]
    fn creates_and_inspects_an_onboarded_database_without_exposing_a_path() {
        tauri::async_runtime::block_on(async {
            let root = test_path("phase5", "create-inspect");
            let _ = fs::remove_file(&root);
            let state = AppState::initialize(root.clone()).await;
            complete_onboarding(&state, valid_onboarding_input())
                .await
                .expect("onboarding should succeed");
            let destination = root.with_extension(BACKUP_EXTENSION);
            let manifest = super::create_backup(
                &state,
                super::CreateBackupInput {
                    destination_path: destination.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                },
            )
            .await
            .expect("backup should succeed");
            assert_eq!(manifest.format_id, BACKUP_FORMAT_ID);
            assert!(destination.is_file());

            let inspection = super::inspect_backup(
                &state,
                super::InspectBackupInput {
                    source_path: destination.to_string_lossy().into_owned(),
                },
            )
            .await
            .expect("inspection should succeed");
            assert!(inspection.checksum_valid);
            assert!(inspection.database_valid);
            assert!(!inspection.inspection_token.is_empty());
            assert_eq!(state.backup_inspection_count(), 1);

            drop(state);
            let _ = fs::remove_file(destination);
            let _ = fs::remove_file(&root);
            let _ = fs::remove_file(format!("{}-wal", root.display()));
            let _ = fs::remove_file(format!("{}-shm", root.display()));
        });
    }

    #[test]
    fn vacuum_snapshot_contains_committed_blob_but_not_an_uncommitted_row() {
        tauri::async_runtime::block_on(async {
            let root = test_path("phase5", "wal-consistency");
            let _ = fs::remove_file(&root);
            let state = AppState::initialize(root.clone()).await;
            complete_onboarding(&state, valid_onboarding_input())
                .await
                .expect("onboarding should succeed");
            let database = state.writable_db().expect("database should be writable");
            let household_id: String = sqlx::query_scalar("SELECT id FROM households LIMIT 1")
                .fetch_one(database)
                .await
                .expect("household should exist");
            let media_id = uuid::Uuid::now_v7().to_string();
            sqlx::query(
                "INSERT INTO media_assets (id, household_id, mime_type, data, created_at) VALUES (?, ?, 'image/png', ?, '2026-08-20T00:00:00.000Z')",
            )
            .bind(&media_id)
            .bind(&household_id)
            .bind(vec![1_u8, 2, 3, 4])
            .execute(database)
            .await
            .expect("committed media should be inserted");
            let mut uncommitted = database.begin().await.expect("write transaction");
            let pending_member = uuid::Uuid::now_v7().to_string();
            sqlx::query(
                "INSERT INTO members (id, household_id, name, sort_order, created_at, updated_at) VALUES (?, ?, 'Uncommitted', 99, '2026-08-20T00:00:00.000Z', '2026-08-20T00:00:00.000Z')",
            )
            .bind(&pending_member)
            .bind(&household_id)
            .execute(&mut *uncommitted)
            .await
            .expect("uncommitted member should be inserted");

            let destination = root.with_extension(BACKUP_EXTENSION);
            super::create_backup(
                &state,
                super::CreateBackupInput {
                    destination_path: destination.to_string_lossy().into_owned(),
                    overwrite_confirmed: false,
                },
            )
            .await
            .expect("backup should succeed while a write transaction is open");

            let staging = root.with_extension("staging");
            fs::create_dir_all(&staging).expect("staging directory");
            let staged_database = staging.join(DATABASE_ENTRY_NAME);
            read_bundle(&destination, Some(&staged_database)).expect("archive should read back");
            let copy = crate::infrastructure::database::connect_read_only(&staged_database)
                .await
                .expect("staged database should open read-only");
            let member_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM members WHERE id = ?")
                .bind(&pending_member)
                .fetch_one(&copy)
                .await
                .expect("staged member query");
            let blob: Vec<u8> = sqlx::query("SELECT data FROM media_assets WHERE id = ?")
                .bind(&media_id)
                .fetch_one(&copy)
                .await
                .expect("staged media query")
                .try_get("data")
                .expect("staged media bytes");
            copy.close().await;
            assert_eq!(member_count, 0);
            assert_eq!(blob, vec![1_u8, 2, 3, 4]);

            drop(uncommitted);
            drop(state);
            let _ = fs::remove_dir_all(staging);
            let _ = fs::remove_file(destination);
            let _ = fs::remove_file(root);
        });
    }

    #[test]
    fn destination_validation_requires_extension_and_overwrite_confirmation() {
        let root = test_path("phase5", "destination-validation");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("fixture directory");
        let existing = root.join("existing.nestworth-backup");
        fs::write(&existing, b"keep").expect("existing destination");
        assert!(validate_destination(&root.join("wrong.zip").to_string_lossy(), false).is_err());
        assert!(validate_destination(&existing.to_string_lossy(), false).is_err());
        assert_eq!(fs::read(&existing).expect("existing bytes"), b"keep");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_reader_rejects_unsafe_names_bad_checksum_and_oversized_manifest() {
        let root = test_path("phase5", "archive-safety");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("fixture directory");
        let database = b"SQLite format 3\0";
        let mut manifest = fixture_manifest();
        manifest.database_byte_length = database.len() as i32;
        manifest.database_sha256 = super::hex_digest(Sha256::digest(database));

        let traversal = root.join("traversal.nestworth-backup");
        write_archive(
            &traversal,
            &[
                ("../manifest.json", serde_json::to_vec(&manifest).unwrap()),
                (DATABASE_ENTRY_NAME, database.to_vec()),
            ],
        );
        assert!(read_bundle(&traversal, None).is_err());

        let absolute = root.join("absolute.nestworth-backup");
        write_archive(
            &absolute,
            &[
                ("/manifest.json", serde_json::to_vec(&manifest).unwrap()),
                (DATABASE_ENTRY_NAME, database.to_vec()),
            ],
        );
        assert!(read_bundle(&absolute, None).is_err());

        let wrong_checksum = root.join("checksum.nestworth-backup");
        let mut wrong = manifest.clone();
        wrong.database_sha256 = "0".repeat(64);
        write_archive(
            &wrong_checksum,
            &[
                (MANIFEST_ENTRY_NAME, serde_json::to_vec(&wrong).unwrap()),
                (DATABASE_ENTRY_NAME, database.to_vec()),
            ],
        );
        assert!(read_bundle(&wrong_checksum, None).is_err());

        let oversized = root.join("oversized.nestworth-backup");
        write_archive(
            &oversized,
            &[
                (
                    MANIFEST_ENTRY_NAME,
                    vec![b'x'; MAX_MANIFEST_BYTES as usize + 1],
                ),
                (DATABASE_ENTRY_NAME, database.to_vec()),
            ],
        );
        assert!(read_bundle(&oversized, None).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_reader_rejects_symlinks_and_truncated_archives() {
        let root = test_path("phase5", "archive-symlink-truncated");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("fixture directory");
        let symlink_archive = root.join("symlink.nestworth-backup");
        let file = fs::File::create(&symlink_archive).expect("archive file");
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer
            .add_symlink(MANIFEST_ENTRY_NAME, "target", options)
            .expect("symlink entry");
        writer
            .start_file(DATABASE_ENTRY_NAME, options)
            .expect("database entry");
        writer.write_all(b"database").expect("database bytes");
        writer.finish().expect("archive finish");
        assert!(read_bundle(&symlink_archive, None).is_err());

        let truncated = root.join("truncated.nestworth-backup");
        let manifest = fixture_manifest();
        write_archive(
            &truncated,
            &[
                (MANIFEST_ENTRY_NAME, serde_json::to_vec(&manifest).unwrap()),
                (DATABASE_ENTRY_NAME, b"SQLite format 3\0".to_vec()),
            ],
        );
        let length = fs::metadata(&truncated).expect("archive metadata").len();
        let file = OpenOptions::new()
            .write(true)
            .open(&truncated)
            .expect("archive should reopen");
        file.set_len(length - 1).expect("archive should truncate");
        assert!(read_bundle(&truncated, None).is_err());
        let _ = fs::remove_dir_all(root);
    }

    fn write_archive(path: &std::path::Path, entries: &[(&str, Vec<u8>)]) {
        let file = fs::File::create(path).expect("archive file");
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, bytes) in entries {
            writer.start_file(*name, options).expect("archive entry");
            writer.write_all(bytes).expect("archive bytes");
        }
        writer.finish().expect("archive finish");
    }

    fn tempfile_for_test() -> fs::File {
        let path = test_path("phase5", "reader-output");
        let _ = fs::remove_file(&path);
        fs::File::create(path).expect("temporary output")
    }
}
