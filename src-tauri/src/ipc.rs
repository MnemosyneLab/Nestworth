use tauri::State;
use tauri_specta::{collect_commands, Builder};

use crate::{
    application::onboarding_service::CompleteOnboardingInput,
    commands::{
        bootstrap::{bootstrap_impl, BootstrapDto},
        onboarding::complete_onboarding_impl,
    },
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn bootstrap(
    state: State<'_, AppState>,
) -> Result<BootstrapDto, crate::error::CommandError> {
    bootstrap_impl(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn complete_onboarding(
    state: State<'_, AppState>,
    input: CompleteOnboardingInput,
) -> Result<(), crate::error::CommandError> {
    complete_onboarding_impl(state, input).await
}

pub fn command_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![bootstrap, complete_onboarding])
}
