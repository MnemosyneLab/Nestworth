#[cfg(test)]
mod tests {
    #[test]
    fn default_capability_allows_dialog_open() {
        let raw = include_str!("../capabilities/default.json");
        assert!(
            raw.contains("dialog:allow-open"),
            "Phase 9 image picking needs the minimal dialog open permission"
        );
        assert!(
            !raw.contains("fs:default") && !raw.contains("dialog:default"),
            "do not grant broad filesystem or dialog defaults"
        );
    }
}
