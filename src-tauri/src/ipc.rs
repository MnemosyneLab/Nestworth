use tauri_specta::{collect_commands, Builder};

pub fn command_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![])
}
