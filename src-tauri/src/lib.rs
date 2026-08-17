use std::path::PathBuf;

use tauri::Manager;

pub mod commands;
pub mod domain;
pub mod error;
pub mod infrastructure;
pub mod ipc;
pub mod state;

fn init_tracing() {
    let subscriber = tracing_subscriber::fmt().with_ansi(false).finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();
    tracing::info!(event = "app.start", version = env!("CARGO_PKG_VERSION"));

    let command_builder = ipc::command_builder();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_log::Builder::default().build())
        .setup(|app| {
            let app_state = match app.path().app_data_dir() {
                Ok(path) => tauri::async_runtime::block_on(state::AppState::initialize(
                    path.join(infrastructure::database::DATABASE_FILENAME),
                )),
                Err(error) => {
                    tracing::error!(error = ?error, "failed to resolve application data directory");
                    state::AppState::unavailable(PathBuf::from("<unavailable>"))
                }
            };
            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(command_builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
