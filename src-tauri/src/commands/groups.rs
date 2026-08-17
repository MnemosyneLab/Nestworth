use crate::{
    application::{
        group_service::{self, CreateGroupInput, GroupRecordDto, UpdateGroupInput},
        reference::{IdInput, ListFilterInput},
    },
    error::CommandError,
    state::AppState,
};

pub async fn list_groups_impl(
    state: &AppState,
    input: ListFilterInput,
) -> Result<Vec<GroupRecordDto>, CommandError> {
    group_service::list_groups(state, input.include_archived)
        .await
        .map_err(CommandError::from)
}

pub async fn create_group_impl(
    state: &AppState,
    input: CreateGroupInput,
) -> Result<GroupRecordDto, CommandError> {
    group_service::create_group(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn update_group_impl(
    state: &AppState,
    input: UpdateGroupInput,
) -> Result<GroupRecordDto, CommandError> {
    group_service::update_group(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn archive_group_impl(
    state: &AppState,
    input: IdInput,
) -> Result<GroupRecordDto, CommandError> {
    group_service::archive_group(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn restore_group_impl(
    state: &AppState,
    input: IdInput,
) -> Result<GroupRecordDto, CommandError> {
    group_service::restore_group(state, &input.id)
        .await
        .map_err(CommandError::from)
}
