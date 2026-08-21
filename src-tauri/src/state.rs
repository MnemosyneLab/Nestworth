use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::{Instant, SystemTime},
};

use sqlx::SqlitePool;
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

use crate::{
    application::providers::{FxAdapter, QuoteAdapter},
    error::{AppError, RestartReason},
    infrastructure::database_bootstrap::{initialize_database, DatabaseBootstrapStatus},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    Ready,
    RestartRequired { reason: RestartReason },
}

pub struct OperationGate {
    operations: Arc<RwLock<()>>,
    terminal_reason: Arc<AtomicU8>,
}

impl OperationGate {
    const READY: u8 = 0;
    const RESET_REQUIRED: u8 = 1;
    const RESTORE_REQUIRED: u8 = 2;

    #[must_use]
    pub fn new() -> Self {
        Self {
            operations: Arc::new(RwLock::new(())),
            terminal_reason: Arc::new(AtomicU8::new(Self::READY)),
        }
    }

    pub async fn acquire_shared(&self) -> Result<SharedOperationPermit, AppError> {
        let guard = Arc::clone(&self.operations).read_owned().await;
        if let Some(reason) = self.restart_reason() {
            drop(guard);
            return Err(AppError::AppRestartRequired { reason });
        }
        Ok(SharedOperationPermit { _guard: guard })
    }

    pub async fn acquire_exclusive(&self) -> Result<ExclusiveOperationPermit, AppError> {
        let guard = Arc::clone(&self.operations).write_owned().await;
        if let Some(reason) = self.restart_reason() {
            drop(guard);
            return Err(AppError::AppRestartRequired { reason });
        }
        Ok(ExclusiveOperationPermit {
            _guard: guard,
            terminal_reason: Arc::clone(&self.terminal_reason),
        })
    }

    fn restart_reason(&self) -> Option<RestartReason> {
        match self.terminal_reason.load(Ordering::Acquire) {
            Self::RESET_REQUIRED => Some(RestartReason::Reset),
            Self::RESTORE_REQUIRED => Some(RestartReason::Restore),
            _ => None,
        }
    }

    pub fn check_available(&self) -> Result<(), AppError> {
        self.restart_reason().map_or(Ok(()), |reason| {
            Err(AppError::AppRestartRequired { reason })
        })
    }

    #[must_use]
    pub fn runtime_state(&self) -> RuntimeState {
        self.restart_reason().map_or(RuntimeState::Ready, |reason| {
            RuntimeState::RestartRequired { reason }
        })
    }
}

impl Default for OperationGate {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SharedOperationPermit {
    _guard: OwnedRwLockReadGuard<()>,
}

pub struct ExclusiveOperationPermit {
    _guard: OwnedRwLockWriteGuard<()>,
    terminal_reason: Arc<AtomicU8>,
}

impl ExclusiveOperationPermit {
    pub fn mark_restart_required(&self, reason: RestartReason) -> Result<(), AppError> {
        let value = match reason {
            RestartReason::Reset => OperationGate::RESET_REQUIRED,
            RestartReason::Restore => OperationGate::RESTORE_REQUIRED,
        };
        self.terminal_reason.store(value, Ordering::Release);
        Ok(())
    }
}

pub enum DatabaseRuntime {
    Writable {
        db: SqlitePool,
    },
    Blocked {
        status: DatabaseBootstrapStatus,
        db_path: PathBuf,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct StoredBackupInspection {
    pub canonical_path: PathBuf,
    pub file_size: u64,
    pub modified_at: SystemTime,
    pub file_device: u64,
    pub file_inode: u64,
    pub sha256: String,
    pub expires_at: Instant,
}

pub struct AppState {
    database: DatabaseRuntime,
    status: DatabaseBootstrapStatus,
    db_path: PathBuf,
    quote_provider: QuoteAdapter,
    fx_provider: FxAdapter,
    operation_gate: Arc<OperationGate>,
    backup_inspections: Mutex<HashMap<String, StoredBackupInspection>>,
}

impl AppState {
    pub async fn initialize(db_path: PathBuf) -> Self {
        Self::initialize_with_providers(
            db_path,
            QuoteAdapter::Unconfigured,
            FxAdapter::Unconfigured,
        )
        .await
    }

    pub async fn initialize_with_providers(
        db_path: PathBuf,
        quote_provider: QuoteAdapter,
        fx_provider: FxAdapter,
    ) -> Self {
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
            quote_provider,
            fx_provider,
            operation_gate: Arc::new(OperationGate::new()),
            backup_inspections: Mutex::new(HashMap::new()),
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
            quote_provider: QuoteAdapter::Unconfigured,
            fx_provider: FxAdapter::Unconfigured,
            operation_gate: Arc::new(OperationGate::new()),
            backup_inspections: Mutex::new(HashMap::new()),
        }
    }

    pub fn writable_db(&self) -> Result<&SqlitePool, AppError> {
        self.operation_gate.check_available()?;
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
            && self.operation_gate.runtime_state() == RuntimeState::Ready
    }

    pub async fn acquire_shared_operation(&self) -> Result<SharedOperationPermit, AppError> {
        self.operation_gate.acquire_shared().await
    }

    pub async fn acquire_exclusive_operation(&self) -> Result<ExclusiveOperationPermit, AppError> {
        self.operation_gate.acquire_exclusive().await
    }

    #[must_use]
    pub fn runtime_state(&self) -> RuntimeState {
        self.operation_gate.runtime_state()
    }

    pub fn quote_provider(&self) -> &QuoteAdapter {
        &self.quote_provider
    }

    pub fn fx_provider(&self) -> &FxAdapter {
        &self.fx_provider
    }

    pub(crate) fn issue_backup_inspection(&self, inspection: StoredBackupInspection) -> String {
        let token = uuid::Uuid::now_v7().to_string();
        let now = Instant::now();
        let mut inspections = self
            .backup_inspections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inspections.retain(|_, value| value.expires_at > now);
        inspections.insert(token.clone(), inspection);
        token
    }

    #[allow(dead_code)]
    pub(crate) fn backup_inspection(&self, token: &str) -> Option<StoredBackupInspection> {
        let now = Instant::now();
        let mut inspections = self
            .backup_inspections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inspections.retain(|_, value| value.expires_at > now);
        inspections.get(token).cloned()
    }

    #[cfg(test)]
    pub(crate) fn backup_inspection_count(&self) -> usize {
        self.backup_inspections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    #[cfg(test)]
    pub fn runtime(&self) -> &DatabaseRuntime {
        &self.database
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{OperationGate, RestartReason, RuntimeState};

    #[test]
    fn shared_operations_coexist_and_exclusive_waits_for_quiescence() {
        tauri::async_runtime::block_on(async {
            let gate = std::sync::Arc::new(OperationGate::new());
            let first = gate.acquire_shared().await.expect("first shared permit");
            let second = gate.acquire_shared().await.expect("second shared permit");
            let gate_for_task = std::sync::Arc::clone(&gate);
            let mut exclusive_task =
                tokio::spawn(async move { gate_for_task.acquire_exclusive().await });

            tokio::task::yield_now().await;
            assert!(
                tokio::time::timeout(Duration::from_millis(10), &mut exclusive_task)
                    .await
                    .is_err()
            );

            drop(first);
            assert!(
                tokio::time::timeout(Duration::from_millis(10), &mut exclusive_task)
                    .await
                    .is_err()
            );

            drop(second);
            let exclusive = tokio::time::timeout(Duration::from_secs(1), exclusive_task)
                .await
                .expect("exclusive permit should become available")
                .expect("exclusive task should not panic")
                .expect("exclusive permit");
            drop(exclusive);
        });
    }

    #[test]
    fn terminal_state_rejects_later_operations() {
        tauri::async_runtime::block_on(async {
            let gate = OperationGate::new();
            let exclusive = gate.acquire_exclusive().await.expect("exclusive permit");
            exclusive
                .mark_restart_required(RestartReason::Reset)
                .expect("terminal transition");
            assert_eq!(
                gate.runtime_state(),
                RuntimeState::RestartRequired {
                    reason: RestartReason::Reset
                }
            );
            drop(exclusive);

            let error = match gate.acquire_shared().await {
                Ok(_) => panic!("shared operation must be rejected"),
                Err(error) => error,
            };
            assert!(matches!(
                error,
                crate::error::AppError::AppRestartRequired {
                    reason: RestartReason::Reset
                }
            ));
            assert!(gate.check_available().is_err());
        });
    }
}
