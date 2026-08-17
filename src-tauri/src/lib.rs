pub mod ipc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let command_builder = ipc::command_builder();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_log::Builder::default().build())
        .invoke_handler(command_builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
