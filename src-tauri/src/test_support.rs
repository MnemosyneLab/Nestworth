use std::{
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::{
    application::onboarding_service::{
        complete_onboarding, CompleteOnboardingInput, OnboardingMemberInput,
    },
    infrastructure::database::connect_writable,
    state::AppState,
};

pub const UNKNOWN_UUID: &str = "00000000-0000-7000-8000-000000000001";

pub fn test_path(phase: &str, name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "nestworth-{phase}-{name}-{}-{nonce}",
        std::process::id()
    ))
}

pub fn file_hash(path: &Path) -> u64 {
    let bytes = fs::read(path).expect("database fixture should exist");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

pub fn valid_onboarding_input() -> CompleteOnboardingInput {
    CompleteOnboardingInput {
        household_name: "Wang Family".to_owned(),
        base_currency: "CNY".to_owned(),
        members: vec![
            OnboardingMemberInput {
                name: "Walt".to_owned(),
            },
            OnboardingMemberInput {
                name: "Spouse".to_owned(),
            },
        ],
    }
}

pub async fn initialize_state(path: PathBuf) -> AppState {
    let _ = fs::remove_file(&path);
    AppState::initialize(path).await
}

pub async fn onboarded_state(name: &str) -> (AppState, PathBuf) {
    let path = test_path("phase5", name);
    let state = initialize_state(path.clone()).await;
    complete_onboarding(&state, valid_onboarding_input())
        .await
        .expect("onboarding should succeed");
    (state, path)
}

pub async fn blocked_future_state(name: &str) -> (AppState, PathBuf, u64) {
    let path = test_path("phase5-future", name);
    let _ = fs::remove_file(&path);
    let pool = connect_writable(&path, true)
        .await
        .expect("fixture database should open");
    sqlx::query(
        "CREATE TABLE _sqlx_migrations (version BIGINT PRIMARY KEY NOT NULL, description TEXT NOT NULL, installed_on TIMESTAMP NOT NULL, success BOOLEAN NOT NULL, checksum BLOB NOT NULL, execution_time BIGINT NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("migration metadata table should be created");
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (999, 'future', CURRENT_TIMESTAMP, 1, zeroblob(32), 1)",
    )
    .execute(&pool)
    .await
    .expect("future migration row should be inserted");
    pool.close().await;

    let state = AppState::initialize(path.clone()).await;
    let before_hash = file_hash(&path);
    (state, path, before_hash)
}

pub fn cleanup(path: &Path) {
    fs::remove_file(path).expect("test database should be removable");
}
