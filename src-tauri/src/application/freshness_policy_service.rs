use sqlx::{Sqlite, Transaction};

use crate::infrastructure::database::SqlitePool;

pub const DEFAULT_POLICIES: [(&str, i64); 4] = [
    ("account_value", 30),
    ("account_cash", 30),
    ("instrument_quote", 7),
    ("fx_quote", 7),
];

/// Make the four Household defaults available without changing user-owned
/// intervals. The transaction is intentionally all-or-nothing: a partially
/// initialized policy set must never be observable by a ready runtime.
pub async fn initialize_default_policies(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let household_id: Option<String> =
        sqlx::query_scalar("SELECT id FROM households ORDER BY created_at, id LIMIT 1")
            .fetch_optional(&mut *tx)
            .await?;

    if let Some(household_id) = household_id {
        ensure_default_policies_in_tx(&mut tx, &household_id).await?;
    }

    tx.commit().await
}

pub async fn ensure_default_policies_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
) -> Result<(), sqlx::Error> {
    let timestamp = crate::domain::Timestamp::now().to_rfc3339();
    for (kind, interval_days) in DEFAULT_POLICIES {
        let exists: i64 = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM freshness_policies
                WHERE household_id = ?
                  AND kind = ?
                  AND target_account_id IS NULL
                  AND target_instrument_id IS NULL
                  AND target_currency_a IS NULL
                  AND target_currency_b IS NULL
            )",
        )
        .bind(household_id)
        .bind(kind)
        .fetch_one(&mut **tx)
        .await?;

        if exists == 1 {
            continue;
        }

        sqlx::query(
            "INSERT INTO freshness_policies (
                id, household_id, kind, target_account_id, target_instrument_id,
                target_currency_a, target_currency_b, review_interval_days,
                created_at, updated_at
             ) VALUES (?, ?, ?, NULL, NULL, NULL, NULL, ?, ?, ?)",
        )
        .bind(crate::domain::FreshnessPolicyId::new().to_string())
        .bind(household_id)
        .bind(kind)
        .bind(interval_days)
        .bind(&timestamp)
        .bind(&timestamp)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::SystemTime,
    };

    use crate::infrastructure::{
        database::connect_writable,
        database_bootstrap::{initialize_database, DatabaseBootstrapStatus},
    };

    fn test_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "nestworth-phase2-policy-{name}-{}-{nonce}.sqlite3",
            std::process::id()
        ))
    }

    fn cleanup(path: &PathBuf) {
        let _ = fs::remove_file(path);
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = path.as_os_str().to_os_string();
            sidecar.push(suffix);
            let _ = fs::remove_file(PathBuf::from(sidecar));
        }
    }

    async fn add_household(path: &Path) {
        let pool = connect_writable(path, false).await.expect("database");
        sqlx::query(
            "INSERT INTO households (id, name, base_currency, created_at, updated_at)
             VALUES ('11111111-1111-4111-8111-111111111111', 'Fixture Household', 'CNY', ?, ?)",
        )
        .bind("2026-08-20T00:00:00.000Z")
        .bind("2026-08-20T00:00:00.000Z")
        .execute(&pool)
        .await
        .expect("household");
        pool.close().await;
    }

    #[test]
    fn existing_household_gets_four_defaults_once_and_reopen_is_idempotent() {
        tauri::async_runtime::block_on(async {
            let path = test_path("defaults");
            cleanup(&path);
            let first = initialize_database(path.clone()).await;
            first.pool.expect("fresh database").close().await;
            add_household(&path).await;

            let opened = initialize_database(path.clone()).await;
            assert_eq!(opened.status, DatabaseBootstrapStatus::Ready);
            let pool = opened.pool.expect("migrated database");
            let policies: Vec<(String, i64)> = sqlx::query_as(
                "SELECT kind, review_interval_days
                 FROM freshness_policies
                 WHERE target_account_id IS NULL
                   AND target_instrument_id IS NULL
                   AND target_currency_a IS NULL
                 ORDER BY kind",
            )
            .fetch_all(&pool)
            .await
            .expect("defaults");
            assert_eq!(
                policies,
                vec![
                    ("account_cash".to_owned(), 30),
                    ("account_value".to_owned(), 30),
                    ("fx_quote".to_owned(), 7),
                    ("instrument_quote".to_owned(), 7),
                ]
            );
            pool.close().await;

            let reopened = initialize_database(path.clone()).await;
            assert_eq!(reopened.status, DatabaseBootstrapStatus::Ready);
            let pool = reopened.pool.expect("reopened database");
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM freshness_policies")
                .fetch_one(&pool)
                .await
                .expect("policy count");
            assert_eq!(count, 4);
            pool.close().await;
            cleanup(&path);
        });
    }

    #[test]
    fn policy_initialization_failure_rolls_back_defaults_and_blocks_stably() {
        tauri::async_runtime::block_on(async {
            let path = test_path("failure");
            cleanup(&path);
            let first = initialize_database(path.clone()).await;
            first.pool.expect("fresh database").close().await;
            add_household(&path).await;

            let pool = connect_writable(&path, false).await.expect("database");
            sqlx::query(
                "CREATE TRIGGER fail_default_policy_insert
                 BEFORE INSERT ON freshness_policies
                 WHEN NEW.target_account_id IS NULL
                 BEGIN SELECT RAISE(ABORT, 'policy initializer test failure'); END",
            )
            .execute(&pool)
            .await
            .expect("test trigger");
            pool.close().await;

            let blocked = initialize_database(path.clone()).await;
            assert_eq!(
                blocked.status,
                DatabaseBootstrapStatus::PolicyInitializationFailed
            );
            assert!(blocked.pool.is_none());

            let check = connect_writable(&path, false).await.expect("database");
            let policy_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM freshness_policies")
                .fetch_one(&check)
                .await
                .expect("policy count");
            assert_eq!(policy_count, 0);
            check.close().await;

            let repeated = initialize_database(path.clone()).await;
            assert_eq!(
                repeated.status,
                DatabaseBootstrapStatus::PolicyInitializationFailed
            );
            assert!(repeated.pool.is_none());
            cleanup(&path);
        });
    }
}
