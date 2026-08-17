use crate::{
    application::{
        account_service::AccountRecordDto,
        group_service::GroupRecordDto,
        institution_service::InstitutionRecordDto,
        media_service::{self, GetMediaInput, MediaAssetDto, SetMediaInput},
        member_service::MemberRecordDto,
    },
    error::CommandError,
    state::AppState,
};

pub async fn set_member_avatar_impl(
    state: &AppState,
    input: SetMediaInput,
) -> Result<MemberRecordDto, CommandError> {
    media_service::set_member_avatar(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn set_institution_logo_impl(
    state: &AppState,
    input: SetMediaInput,
) -> Result<InstitutionRecordDto, CommandError> {
    media_service::set_institution_logo(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn set_group_logo_impl(
    state: &AppState,
    input: SetMediaInput,
) -> Result<GroupRecordDto, CommandError> {
    media_service::set_group_logo(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn set_account_logo_impl(
    state: &AppState,
    input: SetMediaInput,
) -> Result<AccountRecordDto, CommandError> {
    media_service::set_account_logo(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn get_media_impl(
    state: &AppState,
    input: GetMediaInput,
) -> Result<MediaAssetDto, CommandError> {
    media_service::get_media(state, input)
        .await
        .map_err(CommandError::from)
}
