use crate::{
    application::{
        backup_service::{
            self, BackupInspectionDto, BackupManifestDto, CreateBackupInput, InspectBackupInput,
        },
        restore_service::{
            self, InspectRecoveryBackupInput, RecoveryBackupListDto, RestoreBackupInput,
            RestoreBackupResultDto,
        },
    },
    error::CommandError,
    state::AppState,
};

pub async fn create_backup_impl(
    state: &AppState,
    input: CreateBackupInput,
) -> Result<BackupManifestDto, CommandError> {
    backup_service::create_backup(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn inspect_backup_impl(
    state: &AppState,
    input: InspectBackupInput,
) -> Result<BackupInspectionDto, CommandError> {
    backup_service::inspect_backup(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn list_recovery_backups_impl(
    state: &AppState,
) -> Result<RecoveryBackupListDto, CommandError> {
    restore_service::list_recovery_backups(state)
        .await
        .map_err(CommandError::from)
}

pub async fn inspect_recovery_backup_impl(
    state: &AppState,
    input: InspectRecoveryBackupInput,
) -> Result<BackupInspectionDto, CommandError> {
    restore_service::inspect_recovery_backup(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn restore_backup_impl(
    state: &AppState,
    input: RestoreBackupInput,
) -> Result<RestoreBackupResultDto, CommandError> {
    restore_service::restore_backup(state, input)
        .await
        .map_err(CommandError::from)
}
