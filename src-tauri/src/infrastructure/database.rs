pub const DATABASE_FILENAME: &str = "nestworth.sqlite3";

use std::{path::Path, time::Duration};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Pool, Sqlite,
};
pub type SqlitePool = Pool<Sqlite>;

pub async fn connect_writable(
    path: &Path,
    create_if_missing: bool,
) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(create_if_missing)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));

    SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
}

pub async fn connect_read_only(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));

    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
}

pub async fn read_migration_version(path: &Path) -> Result<i64, sqlx::Error> {
    let pool = connect_read_only(path).await?;
    let result = async {
        let migration_table_exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations' LIMIT 1",
        )
        .fetch_optional(&pool)
        .await?
        .is_some();

        if !migration_table_exists {
            return Ok(0);
        }

        sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
        )
        .fetch_one(&pool)
        .await
    }
    .await;
    pool.close().await;
    result
}

pub async fn verify_sqlite_runtime(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let foreign_keys = sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
        .fetch_one(pool)
        .await?;
    if foreign_keys != 1 {
        return Err(sqlx::Error::Protocol(
            "SQLite foreign key enforcement is disabled".to_owned(),
        ));
    }

    let journal_mode = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
        .fetch_one(pool)
        .await?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(sqlx::Error::Protocol(
            "SQLite WAL mode is not enabled".to_owned(),
        ));
    }

    let foreign_key_violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await?;
    if !foreign_key_violations.is_empty() {
        return Err(sqlx::Error::Protocol(
            "SQLite foreign key check failed".to_owned(),
        ));
    }

    let integrity = sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
        .fetch_one(pool)
        .await?;
    if !integrity.eq_ignore_ascii_case("ok") {
        return Err(sqlx::Error::Protocol(
            "SQLite integrity check failed".to_owned(),
        ));
    }

    Ok(())
}

pub async fn ensure_app_settings(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let now = unix_timestamp_string();
    sqlx::query(
        "INSERT OR IGNORE INTO app_settings (id, language, appearance, created_at, updated_at) VALUES (1, 'system', 'system', ?, ?)",
    )
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

fn unix_timestamp_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::unix_timestamp_string;

    #[test]
    fn timestamp_is_non_empty() {
        assert!(!unix_timestamp_string().is_empty());
    }
}
