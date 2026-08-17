use std::{env, fs, path::PathBuf, process};

use nestworth_lib::ipc::command_builder;
use specta_typescript::Typescript;

fn bindings_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/generated/tauri-bindings.ts")
}

fn main() {
    let check = env::args().any(|arg| arg == "--check");
    let output = bindings_path();
    let temporary = env::temp_dir().join(format!("nestworth-tauri-bindings-{}.ts", process::id()));

    command_builder()
        .export(Typescript::default(), &temporary)
        .expect("failed to export TypeScript bindings");

    let generated = fs::read(&temporary).expect("failed to read generated bindings");
    let _ = fs::remove_file(&temporary);

    if check {
        let committed = fs::read(&output).unwrap_or_default();
        if generated != committed {
            eprintln!("generated IPC bindings differ from {}", output.display());
            process::exit(1);
        }
        return;
    }

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("failed to create bindings directory");
    }
    fs::write(output, generated).expect("failed to write TypeScript bindings");
}
