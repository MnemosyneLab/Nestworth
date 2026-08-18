#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs, path::PathBuf};

    const ALLOWED_COMMANDS: &[&str] = &[
        "bootstrap",
        "complete_onboarding",
        "list_members",
        "create_member",
        "update_member",
        "archive_member",
        "restore_member",
        "list_institutions",
        "create_institution",
        "update_institution",
        "archive_institution",
        "restore_institution",
        "list_groups",
        "create_group",
        "update_group",
        "archive_group",
        "restore_group",
        "list_accounts",
        "get_account",
        "create_account",
        "update_account",
        "update_account_value",
        "archive_account",
        "restore_account",
        "get_overview",
        "set_member_avatar",
        "set_institution_logo",
        "set_group_logo",
        "set_account_logo",
        "set_instrument_logo",
        "list_instruments",
        "get_instrument",
        "create_instrument",
        "update_instrument",
        "archive_instrument",
        "restore_instrument",
        "list_holdings",
        "create_holding",
        "update_holding",
        "archive_holding",
        "restore_holding",
        "list_account_cash",
        "append_account_cash",
        "list_instrument_quotes",
        "append_manual_instrument_quote",
        "set_instrument_quote_preference",
        "list_required_fx",
        "list_fx_quotes",
        "append_manual_fx_quote",
        "set_fx_quote_preference",
        "get_portfolio",
        "search_provider_instruments",
        "refresh_instrument",
        "refresh_required_fx",
        "refresh_all",
        "get_media",
        "get_settings",
        "update_settings",
    ];

    const ALLOWED_PERMISSIONS: &[&str] = &[
        "core:default",
        "dialog:allow-open",
        "log:allow-log",
        "window-state:allow-filename",
        "window-state:allow-restore-state",
        "window-state:allow-save-window-state",
    ];

    const FORBIDDEN_TRACING_FIELDS: &[&str] = &[
        "amount",
        "note",
        "path",
        "data",
        "share_bps",
        "shareBps",
        "bytes",
        "mime",
        "mime_type",
        "mimeType",
        "balance",
        "percent",
        "ownership",
        "image",
        "password",
        "sql",
        "query",
    ];

    #[test]
    fn default_capability_is_the_minimal_production_allowlist() {
        let raw = include_str!("../capabilities/default.json");
        let value: serde_json::Value =
            serde_json::from_str(raw).expect("capability file should be JSON");
        let permissions = value["permissions"]
            .as_array()
            .expect("permissions should be an array")
            .iter()
            .map(|permission| {
                permission
                    .as_str()
                    .expect("permission identifiers should be strings")
                    .to_owned()
            })
            .collect::<Vec<_>>();

        assert_eq!(permissions, ALLOWED_PERMISSIONS);
        assert!(
            !raw.contains("fs:")
                && !raw.contains("opener")
                && !raw.contains("shell")
                && !raw.contains("clipboard")
                && !raw.contains("http")
                && !raw.contains("dialog:default")
                && !raw.contains("dialog:allow-save")
                && !raw.contains("dialog:allow-message"),
            "do not grant filesystem, opener, shell, clipboard, HTTP, or broad dialog access"
        );
    }

    #[test]
    fn production_csp_is_self_ipc_and_data_images_only() {
        let raw = include_str!("../tauri.conf.json");
        let value: serde_json::Value =
            serde_json::from_str(raw).expect("tauri.conf.json should be JSON");
        let csp = value["app"]["security"]["csp"]
            .as_str()
            .expect("production CSP should be a string");

        assert!(csp.contains("default-src 'self'"));
        assert!(csp.contains("img-src 'self' data:"));
        assert!(csp.contains("connect-src ipc: http://ipc.localhost"));
        assert!(csp.contains("object-src 'none'"));
        assert!(csp.contains("frame-src 'none'"));
        assert!(csp.contains("base-uri 'none'"));
        assert!(
            !csp.contains("https:") && !csp.contains("*"),
            "production CSP must not allow arbitrary remote content"
        );
    }

    #[test]
    fn rust_plugins_stay_limited_to_dialog_window_state_and_log() {
        let lib = include_str!("lib.rs");
        assert!(lib.contains("tauri_plugin_dialog::init()"));
        assert!(lib.contains("tauri_plugin_window_state::Builder"));
        assert!(lib.contains("tauri_plugin_log::Builder"));
        assert!(!lib.contains("tauri_plugin_fs"));
        assert!(!lib.contains("tauri_plugin_opener"));
        assert!(!lib.contains("tauri_plugin_shell"));
        assert!(!lib.contains("tauri_plugin_http"));
        assert!(!lib.contains("tauri_plugin_clipboard"));
    }

    #[test]
    fn generated_ipc_matches_the_frozen_command_allowlist() {
        let expected = ALLOWED_COMMANDS
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>();
        let rust_commands = parse_collect_commands(include_str!("ipc.rs"));
        let generated_commands =
            parse_generated_invokes(include_str!("../../src/generated/tauri-bindings.ts"));

        assert_eq!(rust_commands, expected, "ipc.rs collect_commands drifted");
        assert_eq!(
            generated_commands, expected,
            "generated TypeScript command names drifted"
        );
    }

    #[test]
    fn tracing_macros_do_not_log_sensitive_fields() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        collect_rust_files(&root, &mut files);
        let mut violations = Vec::new();

        for path in files {
            if path.file_name().and_then(|name| name.to_str()) == Some("capabilities_test.rs") {
                continue;
            }
            let source = fs::read_to_string(&path).expect("rust source should be readable");
            for block in tracing_blocks(&source) {
                for field in FORBIDDEN_TRACING_FIELDS {
                    let needle = format!("{field} =");
                    if block.contains(&needle) {
                        violations.push(format!("{}: {field}", path.display()));
                    }
                }
                if block.contains("error =") {
                    violations.push(format!("{}: error payload", path.display()));
                }
            }
        }

        assert!(
            violations.is_empty(),
            "tracing must not include sensitive fields or error payloads: {violations:?}"
        );
    }

    fn parse_collect_commands(source: &str) -> BTreeSet<String> {
        let start = source
            .find("collect_commands![")
            .expect("command_builder should collect commands");
        let body = &source[start + "collect_commands![".len()..];
        let end = body.find(']').expect("collect_commands list should close");
        body[..end]
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn parse_generated_invokes(source: &str) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        let mut rest = source;
        while let Some(start) = rest.find("TAURI_INVOKE(\"") {
            rest = &rest[start + "TAURI_INVOKE(\"".len()..];
            let end = rest.find('"').expect("invoke name should close");
            names.insert(rest[..end].to_owned());
            rest = &rest[end + 1..];
        }
        names
    }

    fn collect_rust_files(dir: &std::path::Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("source directory should be readable") {
            let entry = entry.expect("directory entry should be readable");
            let path = entry.path();
            if path.is_dir() {
                collect_rust_files(&path, files);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }

    fn tracing_blocks(source: &str) -> Vec<String> {
        let mut blocks = Vec::new();
        let mut rest = source;
        while let Some(start) = rest.find("tracing::") {
            rest = &rest[start..];
            if let Some(end) = rest.find(");") {
                blocks.push(rest[..=end + 1].to_owned());
                rest = &rest[end + 2..];
            } else {
                break;
            }
        }
        blocks
    }
}
