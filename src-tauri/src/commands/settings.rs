use crate::{
    application::settings_service::{self, AppSettingsDto, UpdateSettingsInput},
    error::CommandError,
    state::AppState,
};

pub async fn get_settings_impl(state: &AppState) -> Result<AppSettingsDto, CommandError> {
    settings_service::get_settings(state)
        .await
        .map_err(CommandError::from)
}

pub async fn update_settings_impl(
    state: &AppState,
    input: UpdateSettingsInput,
) -> Result<AppSettingsDto, CommandError> {
    settings_service::update_settings(state, input)
        .await
        .map_err(CommandError::from)
}
