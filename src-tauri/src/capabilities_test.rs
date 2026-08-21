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
        "delete_all_data",
        "get_history_origin",
        "confirm_history_timezone",
        "preview_activity",
        "list_activities",
        "get_activity",
        "create_activity",
        "create_pending_activity",
        "update_pending_activity",
        "list_pending_activities",
        "preview_pending_activity",
        "post_pending_activity",
        "skip_pending_activity",
        "list_recurring_activity_rules",
        "create_recurring_activity_rule",
        "update_recurring_activity_rule",
        "archive_recurring_activity_rule",
        "restore_recurring_activity_rule",
        "generate_due_pending_activities",
        "create_backup",
        "inspect_backup",
        "list_recovery_backups",
        "inspect_recovery_backup",
        "restore_backup",
        "list_maintenance_items",
        "list_freshness_policies",
        "update_freshness_policy",
        "snooze_maintenance_item",
        "reverse_activity",
        "correct_activity",
        "get_account_timeline",
        "get_history_status",
        "rebuild_history_snapshots",
        "get_net_worth_trend",
        "get_analytics_status",
        "get_performance_summary",
        "get_gain_summary",
        "list_holding_gain_summaries",
        "get_net_worth_attribution",
        "list_holding_lots",
        "list_unknown_basis_lots",
        "list_cost_basis_declarations",
        "declare_lot_cost_basis",
        "revoke_lot_cost_basis",
    ];

    const ALLOWED_PERMISSIONS: &[&str] = &[
        "core:default",
        "dialog:allow-open",
        "dialog:allow-save",
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
        "quantity",
        "symbol",
        "account_name",
        "instrument_symbol",
        "quote",
        "legs",
        "raw_legs",
        "unit_price",
        "unitPrice",
        "cost",
        "basis",
        "gain",
        "proceeds",
        "return",
        "rate",
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
    fn generated_bindings_include_activity_union_and_history_commands() {
        let source = include_str!("../../src/generated/tauri-bindings.ts");
        for needle in [
            "export type CreateActivityInput",
            "kind: \"deposit\"",
            "TAURI_INVOKE(\"create_backup\"",
            "TAURI_INVOKE(\"inspect_backup\"",
            "TAURI_INVOKE(\"list_recovery_backups\"",
            "TAURI_INVOKE(\"inspect_recovery_backup\"",
            "TAURI_INVOKE(\"restore_backup\"",
            "export type BackupManifestDto",
            "export type BackupInspectionDto",
            "export type RestoreBackupResultDto",
            "export type RecoveryBackupListDto",
            "TAURI_INVOKE(\"preview_activity\"",
            "TAURI_INVOKE(\"rebuild_history_snapshots\"",
            "TAURI_INVOKE(\"get_net_worth_trend\"",
            "TAURI_INVOKE(\"get_analytics_status\"",
            "TAURI_INVOKE(\"get_performance_summary\"",
            "TAURI_INVOKE(\"get_gain_summary\"",
            "TAURI_INVOKE(\"list_holding_gain_summaries\"",
            "TAURI_INVOKE(\"get_net_worth_attribution\"",
            "TAURI_INVOKE(\"list_holding_lots\"",
            "TAURI_INVOKE(\"list_unknown_basis_lots\"",
            "TAURI_INVOKE(\"list_cost_basis_declarations\"",
            "TAURI_INVOKE(\"declare_lot_cost_basis\"",
            "TAURI_INVOKE(\"revoke_lot_cost_basis\"",
            "SNAPSHOT_REBUILD_FAILED",
            "SNAPSHOT_REBUILD_REQUIRED",
            "ANALYTICS_PERIOD_UNAVAILABLE",
            "ANALYTICS_INPUT_INCOMPLETE",
            "RETURN_NOT_COMPUTABLE",
            "INVALID_COST_BASIS_DECLARATION",
            "COST_BASIS_LOT_NOT_FOUND",
            "kind: \"oneMonth\"",
            "kind: \"household\"",
            "export type LotRefDto",
            "\"originHolding\" | \"acquisition\"",
        ] {
            assert!(
                source.contains(needle),
                "generated bindings should contain {needle}"
            );
        }
        assert!(
            !source.contains("rawLegs") && !source.contains("resultingBalances"),
            "bindings must not expose raw ledger inputs"
        );
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
    fn frontend_does_not_gain_fs_http_clipboard_or_shell_capabilities() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src");
        let mut files = Vec::new();
        collect_source_files(&root, &["ts", "tsx"], &mut files);
        let mut violations = Vec::new();
        for path in files {
            let source = fs::read_to_string(&path).expect("frontend source should be readable");
            for needle in [
                "@tauri-apps/plugin-fs",
                "@tauri-apps/plugin-http",
                "@tauri-apps/plugin-clipboard",
                "@tauri-apps/plugin-shell",
                "@tauri-apps/plugin-opener",
            ] {
                if source.contains(needle) {
                    violations.push(format!("{}: {needle}", path.display()));
                }
            }
        }
        assert!(
            violations.is_empty(),
            "frontend must not gain filesystem, HTTP, clipboard, shell, or opener plugins: {violations:?}"
        );
    }

    #[test]
    fn tracing_macros_do_not_log_sensitive_fields() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        collect_source_files(&root, &["rs"], &mut files);
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

    fn collect_source_files(dir: &std::path::Path, extensions: &[&str], files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("source directory should be readable") {
            let entry = entry.expect("directory entry should be readable");
            let path = entry.path();
            if path.is_dir() {
                collect_source_files(&path, extensions, files);
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| extensions.contains(&ext))
            {
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
