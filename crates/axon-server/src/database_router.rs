//! DatabaseRouter: (tenant_id, database_name) -> storage adapter handle.
//!
//! Unlike the legacy TenantRouter which maps a single db_name slug to a
//! storage handle, DatabaseRouter respects ADR-018's 2-level hierarchy:
//! a single (tenant, database) pair identifies a unique adapter, and
//! the same database name under different tenants MUST resolve to
//! distinct adapters.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axon_core::auth::TenantId;
use axon_core::error::AxonError;
use axon_storage::{MemoryStorageAdapter, StorageAdapter};

/// Trait that produces a fresh storage adapter for a given (tenant, database) pair.
///
/// In production this provisions a per-database SQLite file or a per-database
/// Postgres schema. For tests, a MemoryAdapterFactory returns in-memory adapters.
pub trait DatabaseAdapterFactory {
    fn create_adapter(
        &self,
        tenant_id: TenantId,
        database_name: &str,
    ) -> Result<Arc<dyn StorageAdapter + Send + Sync>, AxonError>;
}

pub struct DatabaseRouter {
    cache: Mutex<HashMap<(TenantId, String), Arc<dyn StorageAdapter + Send + Sync>>>,
    factory: Arc<dyn DatabaseAdapterFactory + Send + Sync>,
}

impl DatabaseRouter {
    pub fn new(factory: Arc<dyn DatabaseAdapterFactory + Send + Sync>) -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            factory,
        }
    }

    /// Resolve a (tenant, database) pair to its storage adapter.
    /// Caches per-pair so repeated resolves return the same Arc<dyn StorageAdapter>.
    pub fn resolve(
        &self,
        tenant_id: TenantId,
        database_name: &str,
    ) -> Result<Arc<dyn StorageAdapter + Send + Sync>, AxonError> {
        let key = (tenant_id.clone(), database_name.to_string());
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| AxonError::Storage("cache mutex poisoned".into()))?;
        if let Some(handle) = cache.get(&key) {
            return Ok(Arc::clone(handle));
        }
        let handle = self.factory.create_adapter(tenant_id, database_name)?;
        cache.insert(key, Arc::clone(&handle));
        Ok(handle)
    }

    /// Drop a cached entry (used when a database is deleted).
    pub fn evict(&self, tenant_id: TenantId, database_name: &str) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(&(tenant_id, database_name.to_string()));
        }
    }
}

pub struct MemoryAdapterFactory;

impl DatabaseAdapterFactory for MemoryAdapterFactory {
    fn create_adapter(
        &self,
        _tenant_id: TenantId,
        _database_name: &str,
    ) -> Result<Arc<dyn StorageAdapter + Send + Sync>, AxonError> {
        Ok(Arc::new(MemoryStorageAdapter::default()))
    }
}
