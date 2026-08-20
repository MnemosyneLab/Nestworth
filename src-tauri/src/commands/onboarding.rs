use tauri::State;

use crate::{
    application::onboarding_service::{complete_onboarding, CompleteOnboardingInput},
    error::CommandError,
    state::AppState,
};

pub async fn complete_onboarding_impl(
    state: &State<'_, AppState>,
    input: CompleteOnboardingInput,
) -> Result<(), CommandError> {
    complete_onboarding(state, input)
        .await
        .map_err(CommandError::from)
}
