use crate::{
    application::{
        member_service::{self, CreateMemberInput, MemberRecordDto, UpdateMemberInput},
        reference::{IdInput, ListFilterInput},
    },
    error::CommandError,
    state::AppState,
};

pub async fn list_members_impl(
    state: &AppState,
    input: ListFilterInput,
) -> Result<Vec<MemberRecordDto>, CommandError> {
    member_service::list_members(state, input.include_archived)
        .await
        .map_err(CommandError::from)
}

pub async fn create_member_impl(
    state: &AppState,
    input: CreateMemberInput,
) -> Result<MemberRecordDto, CommandError> {
    member_service::create_member(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn update_member_impl(
    state: &AppState,
    input: UpdateMemberInput,
) -> Result<MemberRecordDto, CommandError> {
    member_service::update_member(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn archive_member_impl(
    state: &AppState,
    input: IdInput,
) -> Result<MemberRecordDto, CommandError> {
    member_service::archive_member(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn restore_member_impl(
    state: &AppState,
    input: IdInput,
) -> Result<MemberRecordDto, CommandError> {
    member_service::restore_member(state, &input.id)
        .await
        .map_err(CommandError::from)
}
