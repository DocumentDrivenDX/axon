//! Tenant data model for the control plane.
//!
//! These types describe everything the control plane stores about a tenant.
//! None of them reference customer entity data — the whole point of the
//! control plane is to manage tenant instances without touching their
//! contents (FEAT-025 "Data Sovereignty").

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identifier for a tenant.
///
/// Generated as a UUIDv7 on provisioning so that ids are time-sortable and
/// globally unique across control-plane replicas.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TenantId(pub String);

impl TenantId {
    /// Generate a fresh tenant id (UUIDv7 string).
    pub fn generate() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    /// Wrap an existing id string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Deployment mode describes who operates the tenant instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    /// Axon runs the tenant instance in the vendor-operated fleet.
    Hosted,
    /// Customer runs the tenant instance in their own infrastructure.
    ///
    /// In this mode, the customer's instance registers with the control plane
    /// by calling the BYOC registration endpoint, and the control plane can
    /// only reach it on whatever endpoint the customer advertises.
    Byoc,
}

/// Backing storage configuration recorded for a tenant.
///
/// Only metadata is stored: connection URIs, region tags, etc. The control
/// plane does not open connections to these stores except for health probing
/// in the hosted model, and never reads entity data from them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackingStore {
    /// Ephemeral in-memory store (for tests and local development).
    Memory,
    /// SQLite file on the tenant instance.
    Sqlite { path: String },
    /// PostgreSQL cluster that the tenant instance connects to.
    ///
    /// The connection URI is stored but never opened by the control plane.
    Postgres { uri: String, region: Option<String> },
}

impl BackingStore {
    /// Validate that the backing store configuration is usable.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Memory => Ok(()),
            Self::Sqlite { path } => {
                if path.trim().is_empty() {
                    Err("sqlite backing store requires a non-empty path".into())
                } else {
                    Ok(())
                }
            }
            Self::Postgres { uri, .. } => {
                if uri.trim().is_empty() {
                    Err("postgres backing store requires a non-empty uri".into())
                } else if !uri.starts_with("postgres://") && !uri.starts_with("postgresql://") {
                    Err("postgres uri must start with postgres:// or postgresql://".into())
                } else {
                    Ok(())
                }
            }
        }
    }
}

/// Data retention policy applied when a tenant is deprovisioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataRetentionPolicy {
    /// Keep tenant data indefinitely after deprovisioning. The tenant instance
    /// remains accessible read-only through whatever operator controls access
    /// to the backing store. This is the default for BYOC deployments where
    /// Axon does not control the backing store lifecycle.
    Retain,
    /// Retain tenant data for the configured number of days after
    /// deprovisioning, then permanently delete.
    RetainForDays(u32),
    /// Immediately schedule the tenant data for deletion when the tenant
    /// transitions to `Terminated`.
    DeleteImmediately,
}

impl Default for DataRetentionPolicy {
    fn default() -> Self {
        Self::RetainForDays(30)
    }
}

/// Lifecycle state of a managed tenant.
///
/// State machine:
///
/// ```text
/// Provisioning ──> Active ──> Suspended ──> Active
///        │            │             │
///        │            └──> Deprovisioning ──> Terminated
///        │
///        └──> Failed
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantStatus {
    /// Tenant has been created but the backing store is still being set up.
    Provisioning,
    /// Tenant is live and accepting traffic.
    Active,
    /// Tenant is reachable but has been administratively paused. No traffic
    /// is routed until the operator reactivates it.
    Suspended,
    /// Tenant has been scheduled for deprovisioning. Data is retained
    /// according to its `DataRetentionPolicy` until the policy expires.
    Deprovisioning,
    /// Tenant has been fully decommissioned and any retained data has been
    /// released.
    Terminated,
    /// Tenant provisioning or deprovisioning failed. Operators must take
    /// manual action; the control plane will not automatically retry.
    Failed,
}

impl TenantStatus {
    /// Returns true if the tenant is considered "observable" — i.e. the
    /// control plane accepts health reports and metric samples for it.
    pub fn is_observable(self) -> bool {
        matches!(self, Self::Active | Self::Suspended | Self::Provisioning)
    }

    /// Returns true if the tenant is in a terminal state.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Terminated | Self::Failed)
    }
}

/// Parameters required to provision a new tenant.
///
/// This is what an operator or provisioning automation submits to the control
/// plane to create a new tenant record. The control plane assigns the
/// `TenantId` and initial `TenantStatus` — the caller does not pick them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantSpec {
    /// Operator-visible display name.
    pub name: String,
    /// Deployment mode (hosted vs BYOC).
    pub deployment_mode: DeploymentMode,
    /// Backing store configuration.
    pub backing_store: BackingStore,
    /// Data retention policy applied on deprovisioning.
    #[serde(default)]
    pub retention: DataRetentionPolicy,
    /// Operator-defined labels for grouping and filtering.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

impl TenantSpec {
    /// Validate a tenant spec. Returns a message suitable for
    /// [`ControlPlaneError::InvalidArgument`].
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("tenant name must not be empty".into());
        }
        self.backing_store.validate()?;
        Ok(())
    }
}

/// Full tenant record stored by the control plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tenant {
    pub id: TenantId,
    pub spec: TenantSpec,
    pub status: TenantStatus,
    /// Millisecond epoch timestamp when the tenant record was created.
    pub created_at_ms: u64,
    /// Millisecond epoch timestamp of the most recent state transition.
    pub updated_at_ms: u64,
    /// Endpoint at which the control plane (or operators) can reach the
    /// tenant instance. For BYOC tenants this is populated by the tenant's
    /// own registration call; for hosted tenants it is set on provisioning.
    pub instance_endpoint: Option<String>,
    /// Most recent health report received for this tenant, if any.
    pub last_health: Option<HealthReport>,
}

/// Coarse health classification used by the dashboard.
///
/// Health is reported *by* the tenant instance (push model) or scraped from
/// a metrics endpoint (pull model) — the control plane never derives health
/// by inspecting tenant entity data.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    #[default]
    Unknown,
}

/// A single health report for a tenant instance.
///
/// Contains aggregate metric samples only — never entity data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthReport {
    /// When the report was generated by the tenant instance.
    pub reported_at_ms: u64,
    /// Overall health classification.
    pub status: HealthStatus,
    /// Build version of the tenant instance that emitted the report.
    pub instance_version: Option<String>,
    /// Storage bytes in use (operator-visible capacity signal).
    pub storage_bytes: Option<u64>,
    /// Open client connections at report time.
    pub open_connections: Option<u32>,
    /// p99 request latency in milliseconds.
    pub p99_latency_ms: Option<u32>,
    /// Error rate over the report window (0.0 – 1.0).
    pub error_rate: Option<f32>,
}

impl HealthReport {
    /// Validate numeric ranges. Returns `Err` with a message for
    /// [`ControlPlaneError::InvalidArgument`].
    pub fn validate(&self) -> Result<(), String> {
        if let Some(rate) = self.error_rate {
            if !(0.0..=1.0).contains(&rate) || rate.is_nan() {
                return Err(format!("error_rate must be within [0.0, 1.0]; got {rate}"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_id_roundtrips_through_json() {
        let id = TenantId::new("abc");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"abc\"");
        let parsed: TenantId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn generated_tenant_ids_are_unique() {
        let a = TenantId::generate();
        let b = TenantId::generate();
        assert_ne!(a, b);
    }

    #[test]
    fn backing_store_memory_always_valid() {
        BackingStore::Memory.validate().unwrap();
    }

    #[test]
    fn backing_store_sqlite_requires_path() {
        assert!(BackingStore::Sqlite {
            path: String::new()
        }
        .validate()
        .is_err());
        BackingStore::Sqlite {
            path: "/tmp/x.db".into(),
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn backing_store_postgres_requires_scheme() {
        assert!(BackingStore::Postgres {
            uri: "localhost/db".into(),
            region: None,
        }
        .validate()
        .is_err());
        BackingStore::Postgres {
            uri: "postgres://user@host/db".into(),
            region: Some("us-east-1".into()),
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn tenant_spec_rejects_empty_name() {
        let spec = TenantSpec {
            name: "  ".into(),
            deployment_mode: DeploymentMode::Hosted,
            backing_store: BackingStore::Memory,
            retention: DataRetentionPolicy::default(),
            labels: BTreeMap::new(),
        };
        assert!(spec.validate().is_err());
    }

    #[test]
    fn tenant_status_classification() {
        assert!(TenantStatus::Active.is_observable());
        assert!(TenantStatus::Suspended.is_observable());
        assert!(TenantStatus::Provisioning.is_observable());
        assert!(!TenantStatus::Terminated.is_observable());
        assert!(TenantStatus::Terminated.is_terminal());
        assert!(TenantStatus::Failed.is_terminal());
        assert!(!TenantStatus::Active.is_terminal());
    }

    #[test]
    fn health_report_rejects_nonsense_error_rate() {
        let bad = HealthReport {
            reported_at_ms: 1,
            status: HealthStatus::Degraded,
            instance_version: None,
            storage_bytes: None,
            open_connections: None,
            p99_latency_ms: None,
            error_rate: Some(1.5),
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn data_retention_default_is_thirty_days() {
        assert_eq!(
            DataRetentionPolicy::default(),
            DataRetentionPolicy::RetainForDays(30)
        );
    }
}
