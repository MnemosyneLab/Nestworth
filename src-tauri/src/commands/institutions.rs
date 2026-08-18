use crate::{
    application::{
        institution_service::{
            self, CreateInstitutionInput, InstitutionRecordDto, UpdateInstitutionInput,
        },
        reference::{IdInput, ListFilterInput},
    },
    error::CommandError,
    state::AppState,
};

pub async fn list_institutions_impl(
    state: &AppState,
    input: ListFilterInput,
) -> Result<Vec<InstitutionRecordDto>, CommandError> {
    institution_service::list_institutions(state, input.include_archived)
        .await
        .map_err(CommandError::from)
}

pub async fn create_institution_impl(
    state: &AppState,
    input: CreateInstitutionInput,
) -> Result<InstitutionRecordDto, CommandError> {
    institution_service::create_institution(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn update_institution_impl(
    state: &AppState,
    input: UpdateInstitutionInput,
) -> Result<InstitutionRecordDto, CommandError> {
    institution_service::update_institution(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn archive_institution_impl(
    state: &AppState,
    input: IdInput,
) -> Result<InstitutionRecordDto, CommandError> {
    institution_service::archive_institution(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn restore_institution_impl(
    state: &AppState,
    input: IdInput,
) -> Result<InstitutionRecordDto, CommandError> {
    institution_service::restore_institution(state, &input.id)
        .await
        .map_err(CommandError::from)
}
