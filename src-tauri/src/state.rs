use std::path::{Path, PathBuf};

use sqlx::SqlitePool;

use crate::{
    error::AppError,
    infrastructure::database_bootstrap::{initialize_database, DatabaseBootstrapStatus},
};

pub enum DatabaseRuntime {
    Writable {
        db: SqlitePool,
    },
    Blocked {
        status: DatabaseBootstrapStatus,
        db_path: PathBuf,
    },
}

pub struct AppState {
    database: DatabaseRuntime,
    status: DatabaseBootstrapStatus,
    db_path: PathBuf,
}

impl AppState {
    pub async fn initialize(db_path: PathBuf) -> Self {
        let result = initialize_database(db_path.clone()).await;
        let status = result.status.clone();
        let database = match result.pool {
            Some(db) => DatabaseRuntime::Writable { db },
            None => DatabaseRuntime::Blocked {
                status: status.clone(),
                db_path: db_path.clone(),
            },
        };

        Self {
            database,
            status,
            db_path,
        }
    }
    pub fn unavailable(db_path: PathBuf) -> Self {
        let status = DatabaseBootstrapStatus::Unavailable;
        Self {
            database: DatabaseRuntime::Blocked {
                status: status.clone(),
                db_path: db_path.clone(),
            },
            status,
            db_path,
        }
    }

    pub fn writable_db(&self) -> Result<&SqlitePool, AppError> {
        match &self.database {
            DatabaseRuntime::Writable { db } => Ok(db),
            DatabaseRuntime::Blocked { status, .. } => Err(AppError::from_bootstrap_status(status)),
        }
    }

    pub fn bootstrap_status(&self) -> &DatabaseBootstrapStatus {
        &self.status
    }

    pub fn database_path(&self) -> &Path {
        &self.db_path
    }

    pub fn is_writable(&self) -> bool {
        matches!(self.database, DatabaseRuntime::Writable { .. })
    }

    #[cfg(test)]
    pub fn runtime(&self) -> &DatabaseRuntime {
        &self.database
    }
}
