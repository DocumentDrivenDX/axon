//! Persistence layer for control plane metadata.
//!
//! FEAT-025 requires the control plane to be backed by its own PostgreSQL
//! database. That adapter lives behind a trait so tests (and air-gapped
//! development) can use an in-memory implementation, and so a real Postgres
//! adapter can be plugged in later without changing the service layer.
//!
//! Crucially, *none* of these methods expose entity data from a tenant's
//! own database — the trait only exchanges tenant metadata and health
//! reports. That boundary is part of the FEAT-025 data-sovereignty contract.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::ControlPlaneError;
use crate::model::{HealthReport, Tenant, TenantId};

/// Storage backend for the control plane.
///
/// Implementations are expected to be cheap to clone (typically an `Arc`
/// around the real state).
pub trait ControlPlaneStore: Send + Sync {
    /// Insert a new tenant. Returns [`ControlPlaneError::TenantAlreadyExists`]
    /// if a tenant with the same id is already stored.
    fn insert(&self, tenant: Tenant) -> Result<(), ControlPlaneError>;

    /// Replace an existing tenant record. Returns
    /// [`ControlPlaneError::TenantNotFound`] when the id is unknown.
    fn update(&self, tenant: Tenant) -> Result<(), ControlPlaneError>;

    /// Fetch a tenant by id.
    fn get(&self, id: &TenantId) -> Result<Tenant, ControlPlaneError>;

    /// List all tenants in deterministic order.
    ///
    /// Implementations MUST sort by `created_at_ms` ascending with `id` as
    /// tiebreaker so that tests and dashboards see a stable ordering.
    fn list(&self) -> Result<Vec<Tenant>, ControlPlaneError>;

    /// Record a health report against a tenant. The default implementation
    /// fetches, mutates, and replaces the record — backing stores that can
    /// do this in one round-trip should override it.
    fn record_health(
        &self,
        id: &TenantId,
        report: HealthReport,
    ) -> Result<(), ControlPlaneError> {
        let mut tenant = self.get(id)?;
        tenant.last_health = Some(report);
        self.update(tenant)
    }
}

/// In-memory control plane store.
///
/// Used by tests, local development, and air-gapped scenarios where
/// spinning up PostgreSQL is impractical. Not durable across restarts.
#[derive(Debug, Default)]
pub struct InMemoryControlPlaneStore {
    tenants: Mutex<HashMap<TenantId, Tenant>>,
}

impl InMemoryControlPlaneStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ControlPlaneStore for InMemoryControlPlaneStore {
    fn insert(&self, tenant: Tenant) -> Result<(), ControlPlaneError> {
        let mut guard = self
            .tenants
            .lock()
            .map_err(|e| ControlPlaneError::Store(format!("mutex poisoned: {e}")))?;
        if guard.contains_key(&tenant.id) {
            return Err(ControlPlaneError::TenantAlreadyExists(
                tenant.id.to_string(),
            ));
        }
        guard.insert(tenant.id.clone(), tenant);
        Ok(())
    }

    fn update(&self, tenant: Tenant) -> Result<(), ControlPlaneError> {
        let mut guard = self
            .tenants
            .lock()
            .map_err(|e| ControlPlaneError::Store(format!("mutex poisoned: {e}")))?;
        if !guard.contains_key(&tenant.id) {
            return Err(ControlPlaneError::TenantNotFound(tenant.id.to_string()));
        }
        guard.insert(tenant.id.clone(), tenant);
        Ok(())
    }

    fn get(&self, id: &TenantId) -> Result<Tenant, ControlPlaneError> {
        let guard = self
            .tenants
            .lock()
            .map_err(|e| ControlPlaneError::Store(format!("mutex poisoned: {e}")))?;
        guard
            .get(id)
            .cloned()
            .ok_or_else(|| ControlPlaneError::TenantNotFound(id.to_string()))
    }

    fn list(&self) -> Result<Vec<Tenant>, ControlPlaneError> {
        let guard = self
            .tenants
            .lock()
            .map_err(|e| ControlPlaneError::Store(format!("mutex poisoned: {e}")))?;
        let mut out: Vec<Tenant> = guard.values().cloned().collect();
        out.sort_by(|a, b| {
            a.created_at_ms
                .cmp(&b.created_at_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        BackingStore, DataRetentionPolicy, DeploymentMode, HealthStatus, TenantSpec, TenantStatus,
    };
    use std::collections::BTreeMap;

    fn make_tenant(id: &str, created_at_ms: u64) -> Tenant {
        Tenant {
            id: TenantId::new(id),
            spec: TenantSpec {
                name: id.into(),
                deployment_mode: DeploymentMode::Hosted,
                backing_store: BackingStore::Memory,
                retention: DataRetentionPolicy::default(),
                labels: BTreeMap::new(),
            },
            status: TenantStatus::Provisioning,
            created_at_ms,
            updated_at_ms: created_at_ms,
            instance_endpoint: None,
            last_health: None,
        }
    }

    #[test]
    fn insert_then_get() {
        let store = InMemoryControlPlaneStore::new();
        store.insert(make_tenant("t-1", 100)).unwrap();
        let got = store.get(&TenantId::new("t-1")).unwrap();
        assert_eq!(got.spec.name, "t-1");
    }

    #[test]
    fn insert_duplicate_errors() {
        let store = InMemoryControlPlaneStore::new();
        store.insert(make_tenant("t-1", 100)).unwrap();
        let err = store.insert(make_tenant("t-1", 200)).unwrap_err();
        assert!(matches!(err, ControlPlaneError::TenantAlreadyExists(_)));
    }

    #[test]
    fn update_unknown_errors() {
        let store = InMemoryControlPlaneStore::new();
        let err = store.update(make_tenant("t-missing", 1)).unwrap_err();
        assert!(matches!(err, ControlPlaneError::TenantNotFound(_)));
    }

    #[test]
    fn list_is_sorted_by_created_at() {
        let store = InMemoryControlPlaneStore::new();
        store.insert(make_tenant("t-b", 200)).unwrap();
        store.insert(make_tenant("t-a", 100)).unwrap();
        store.insert(make_tenant("t-c", 300)).unwrap();
        let listed = store.list().unwrap();
        let ids: Vec<_> = listed.iter().map(|t| t.id.to_string()).collect();
        assert_eq!(ids, vec!["t-a", "t-b", "t-c"]);
    }

    #[test]
    fn record_health_sets_last_health() {
        let store = InMemoryControlPlaneStore::new();
        store.insert(make_tenant("t-1", 100)).unwrap();
        store
            .record_health(
                &TenantId::new("t-1"),
                HealthReport {
                    reported_at_ms: 500,
                    status: HealthStatus::Healthy,
                    instance_version: Some("0.1.0".into()),
                    storage_bytes: Some(1024),
                    open_connections: Some(2),
                    p99_latency_ms: Some(12),
                    error_rate: Some(0.0),
                },
            )
            .unwrap();
        let got = store.get(&TenantId::new("t-1")).unwrap();
        assert_eq!(got.last_health.unwrap().status, HealthStatus::Healthy);
    }
}
