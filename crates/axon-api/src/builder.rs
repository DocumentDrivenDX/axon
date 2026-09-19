use std::path::PathBuf;
use std::sync::Arc;

use axon_core::error::AxonError;
use axon_storage::{
    MemoryStorageAdapter, PostgresStorageAdapter, SqliteStorageAdapter, StorageAdapter,
};

use crate::handler::AxonHandler;

/// Storage backend selected for an Axon application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AxonStorageKind {
    Memory,
    Sqlite,
    Postgres,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AxonStorageSelection {
    Memory,
    SqlitePath(PathBuf),
    SqliteInMemory,
    PostgresDsn(String),
}

/// Supported application construction boundary for Axon handlers and storage.
#[derive(Clone, Debug, Default)]
pub struct AxonBuilder {
    storage: Option<AxonStorageSelection>,
}

impl AxonBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn memory(mut self) -> Self {
        self.storage = Some(AxonStorageSelection::Memory);
        self
    }

    pub fn sqlite_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.storage = Some(AxonStorageSelection::SqlitePath(path.into()));
        self
    }

    pub fn sqlite_in_memory(mut self) -> Self {
        self.storage = Some(AxonStorageSelection::SqliteInMemory);
        self
    }

    pub fn postgres_dsn(mut self, dsn: impl Into<String>) -> Self {
        self.storage = Some(AxonStorageSelection::PostgresDsn(dsn.into()));
        self
    }

    pub fn storage_kind(&self) -> Result<AxonStorageKind, AxonError> {
        self.validate()?;
        Ok(match self.selection()? {
            AxonStorageSelection::Memory => AxonStorageKind::Memory,
            AxonStorageSelection::SqlitePath(_) | AxonStorageSelection::SqliteInMemory => {
                AxonStorageKind::Sqlite
            }
            AxonStorageSelection::PostgresDsn(_) => AxonStorageKind::Postgres,
        })
    }

    pub fn validate(&self) -> Result<(), AxonError> {
        match self.selection()? {
            AxonStorageSelection::Memory | AxonStorageSelection::SqliteInMemory => Ok(()),
            AxonStorageSelection::SqlitePath(path) if path.as_os_str().is_empty() => Err(
                AxonError::InvalidArgument("sqlite storage requires a non-empty path".to_owned()),
            ),
            AxonStorageSelection::SqlitePath(path) if path.to_str().is_none() => {
                Err(AxonError::InvalidArgument(format!(
                    "sqlite storage path is not valid UTF-8: {}",
                    path.display()
                )))
            }
            AxonStorageSelection::SqlitePath(_) => Ok(()),
            AxonStorageSelection::PostgresDsn(dsn) if dsn.trim().is_empty() => Err(
                AxonError::InvalidArgument("postgres storage requires a non-empty DSN".to_owned()),
            ),
            AxonStorageSelection::PostgresDsn(_) => Ok(()),
        }
    }

    pub fn build_storage(&self) -> Result<Box<dyn StorageAdapter + Send + Sync>, AxonError> {
        self.validate()?;
        match self.selection()? {
            AxonStorageSelection::Memory => Ok(Box::new(Self::memory_storage())),
            AxonStorageSelection::SqlitePath(path) => {
                let path = path.to_str().ok_or_else(|| {
                    AxonError::InvalidArgument(format!(
                        "sqlite storage path is not valid UTF-8: {}",
                        path.display()
                    ))
                })?;
                Ok(Box::new(SqliteStorageAdapter::open(path)?))
            }
            AxonStorageSelection::SqliteInMemory => Ok(Box::new(Self::sqlite_memory_storage()?)),
            AxonStorageSelection::PostgresDsn(dsn) => {
                Ok(Box::new(PostgresStorageAdapter::connect(dsn)?))
            }
        }
    }

    pub fn build_shared_storage(&self) -> Result<Arc<dyn StorageAdapter + Send + Sync>, AxonError> {
        Ok(Arc::from(self.build_storage()?))
    }

    pub fn build_handler(
        &self,
    ) -> Result<AxonHandler<Box<dyn StorageAdapter + Send + Sync>>, AxonError> {
        Ok(AxonHandler::new(self.build_storage()?))
    }

    pub fn build_sqlite_storage(&self) -> Result<SqliteStorageAdapter, AxonError> {
        self.validate()?;
        match self.selection()? {
            AxonStorageSelection::SqlitePath(path) => {
                let path = path.to_str().ok_or_else(|| {
                    AxonError::InvalidArgument(format!(
                        "sqlite storage path is not valid UTF-8: {}",
                        path.display()
                    ))
                })?;
                SqliteStorageAdapter::open(path)
            }
            AxonStorageSelection::SqliteInMemory => Self::sqlite_memory_storage(),
            AxonStorageSelection::Memory | AxonStorageSelection::PostgresDsn(_) => Err(
                AxonError::InvalidArgument("selected storage is not SQLite".to_owned()),
            ),
        }
    }

    pub fn build_postgres_storage(&self) -> Result<PostgresStorageAdapter, AxonError> {
        self.validate()?;
        match self.selection()? {
            AxonStorageSelection::PostgresDsn(dsn) => PostgresStorageAdapter::connect(dsn),
            AxonStorageSelection::Memory
            | AxonStorageSelection::SqlitePath(_)
            | AxonStorageSelection::SqliteInMemory => Err(AxonError::InvalidArgument(
                "selected storage is not PostgreSQL".to_owned(),
            )),
        }
    }

    pub fn memory_storage() -> MemoryStorageAdapter {
        MemoryStorageAdapter::default()
    }

    fn sqlite_memory_storage() -> Result<SqliteStorageAdapter, AxonError> {
        SqliteStorageAdapter::open_in_memory()
    }

    fn selection(&self) -> Result<&AxonStorageSelection, AxonError> {
        self.storage.as_ref().ok_or_else(|| {
            AxonError::InvalidArgument(
                "AxonBuilder requires a storage backend selection".to_owned(),
            )
        })
    }
}
