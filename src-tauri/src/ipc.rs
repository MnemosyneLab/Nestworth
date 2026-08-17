use tauri::State;
use tauri_specta::{collect_commands, Builder};

use crate::{
    commands::bootstrap::{bootstrap_impl, BootstrapDto},
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn bootstrap(
    state: State<'_, AppState>,
) -> Result<BootstrapDto, crate::error::CommandError> {
    bootstrap_impl(state).await
}

pub fn command_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![bootstrap])
}
