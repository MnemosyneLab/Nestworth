use crate::{
    application::backup_service::{
        self, BackupInspectionDto, BackupManifestDto, CreateBackupInput, InspectBackupInput,
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
