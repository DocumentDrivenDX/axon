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

/// An event recorded whenever an administrative lifecycle operation is
/// performed on a tenant.
///
/// Events are append-only and never mutated after creation, providing an
/// auditable trail of all control-plane actions. The `actor` field is a
/// placeholder until the control-plane authentication layer is implemented.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique event identifier (UUIDv7).
    pub id: String,
    /// The tenant this event concerns.
    pub tenant_id: TenantId,
    /// Operation name (e.g. `"provision"`, `"mark_active"`, `"deprovision"`).
    pub operation: String,
    /// Actor that triggered the operation. Placeholder `"operator"` until
    /// control-plane authentication lands.
    pub actor: String,
    /// Control-plane clock timestamp when the event was recorded.
    pub occurred_at_ms: u64,
    /// Tenant lifecycle state before the operation (`None` for provisioning).
    pub previous_status: Option<TenantStatus>,
    /// Tenant lifecycle state after the operation.
    pub new_status: Option<TenantStatus>,
}

/// Scope of an [`ObservationCredential`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationScope {
    /// Bearer may read health status and aggregate counts only.
    HealthOnly,
    /// Bearer may read all exported metric series.
    MetricsRead,
}

/// Shape of a short-lived credential for observing a tenant instance.
///
/// The control plane may issue these to operators or monitoring systems to
/// observe a specific tenant instance (e.g. scrape its metrics endpoint)
/// without granting full administrative access. Credentials expire after
/// `expires_at_ms` — any caller must reject them after that point.
///
/// # Implementation status
///
/// Credential issuance (`POST /tenants/{id}/observation-credentials`) is not
/// yet implemented. This type documents the intended contract so that shape
/// tests can run now and be wired to a real implementation in the follow-up
/// bead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationCredential {
    /// Unique credential identifier.
    pub id: String,
    /// Tenant whose instance this credential may observe.
    pub tenant_id: TenantId,
    /// Control-plane clock timestamp when the credential was issued.
    pub issued_at_ms: u64,
    /// Epoch ms after which this credential must be rejected as expired.
    pub expires_at_ms: u64,
    /// What the credential holder is permitted to observe.
    pub scope: ObservationScope,
}

impl ObservationCredential {
    /// Returns `true` if `now_ms` is at or past the credential's expiry.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at_ms
    }

    /// Returns the total lifetime of the credential in milliseconds
    /// (`expires_at_ms - issued_at_ms`, saturating at zero).
    pub fn ttl_ms(&self) -> u64 {
        self.expires_at_ms.saturating_sub(self.issued_at_ms)
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

    #[test]
    fn observation_credential_is_expired_at_boundary() {
        let cred = ObservationCredential {
            id: "cred-1".to_string(),
            tenant_id: TenantId::new("t-1"),
            issued_at_ms: 1_000,
            expires_at_ms: 4_600_000, // 1h TTL
            scope: ObservationScope::HealthOnly,
        };
        assert!(
            !cred.is_expired(4_599_999),
            "must be valid 1ms before expiry"
        );
        assert!(
            cred.is_expired(4_600_000),
            "must be expired at exact expiry"
        );
        assert!(cred.is_expired(4_600_001), "must be expired after expiry");
    }

    #[test]
    fn observation_credential_ttl_ms_matches_window() {
        let cred = ObservationCredential {
            id: "cred-2".to_string(),
            tenant_id: TenantId::new("t-1"),
            issued_at_ms: 1_000_000,
            expires_at_ms: 1_000_000 + 3_600_000,
            scope: ObservationScope::MetricsRead,
        };
        assert_eq!(cred.ttl_ms(), 3_600_000);
    }

    #[test]
    fn observation_credential_ttl_saturates_at_zero() {
        let cred = ObservationCredential {
            id: "cred-3".to_string(),
            tenant_id: TenantId::new("t-1"),
            issued_at_ms: 5_000,
            expires_at_ms: 1_000, // expires_at before issued_at (degenerate)
            scope: ObservationScope::HealthOnly,
        };
        assert_eq!(cred.ttl_ms(), 0, "saturating_sub must not wrap");
        assert!(cred.is_expired(5_000));
    }

    #[test]
    fn audit_event_fields_are_accessible() {
        let ev = AuditEvent {
            id: "ev-1".to_string(),
            tenant_id: TenantId::new("t-99"),
            operation: "provision".to_string(),
            actor: "operator".to_string(),
            occurred_at_ms: 42_000,
            previous_status: None,
            new_status: Some(TenantStatus::Provisioning),
        };
        assert_eq!(ev.operation, "provision");
        assert_eq!(ev.new_status, Some(TenantStatus::Provisioning));
        assert!(ev.previous_status.is_none());
    }
}
