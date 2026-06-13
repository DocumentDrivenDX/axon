//! Control plane business logic.
//!
//! Enforces tenant lifecycle transitions, BYOC registration rules, and the
//! data-sovereignty boundary (never reads entity data from a tenant).

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use crate::error::ControlPlaneError;
use crate::model::{
    AuditEvent, DeploymentMode, HealthReport, ObservationCredential, ObservationScope, Tenant,
    TenantId, TenantSpec, TenantStatus,
};
use crate::store::ControlPlaneStore;

/// Clock abstraction so tests can inject deterministic timestamps.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// Default clock: reads the system wall clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
}

/// Control plane service — the layer the HTTP API calls into.
///
/// Holds a store, a clock, and an in-memory audit log. Cheap to clone —
/// all three fields are `Arc`-wrapped so clones share the same state.
#[derive(Clone)]
pub struct ControlPlaneService {
    store: Arc<dyn ControlPlaneStore>,
    clock: Arc<dyn Clock>,
    audit_log: Arc<Mutex<Vec<AuditEvent>>>,
}

impl std::fmt::Debug for ControlPlaneService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlPlaneService").finish()
    }
}

impl ControlPlaneService {
    /// Construct a service with the system clock.
    pub fn new(store: Arc<dyn ControlPlaneStore>) -> Self {
        Self {
            store,
            clock: Arc::new(SystemClock),
            audit_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Construct a service with a custom clock (used by tests).
    pub fn with_clock(store: Arc<dyn ControlPlaneStore>, clock: Arc<dyn Clock>) -> Self {
        Self {
            store,
            clock,
            audit_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Return a snapshot of all audit events recorded so far, in insertion order.
    pub fn audit_events(&self) -> Vec<AuditEvent> {
        match self.audit_log.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => Vec::new(),
        }
    }

    fn emit_audit(&self, event: AuditEvent) {
        if let Ok(mut log) = self.audit_log.lock() {
            log.push(event);
        }
    }

    /// Provision a new tenant.
    ///
    /// Allocates a fresh [`TenantId`], validates the spec, and inserts the
    /// tenant in the `Provisioning` state. The caller (or automation) is
    /// expected to call [`Self::mark_active`] once the backing store is
    /// actually usable.
    pub fn provision_tenant(&self, spec: TenantSpec) -> Result<Tenant, ControlPlaneError> {
        spec.validate()
            .map_err(ControlPlaneError::InvalidArgument)?;
        let now = self.clock.now_ms();
        let tenant = Tenant {
            id: TenantId::generate(),
            spec,
            status: TenantStatus::Provisioning,
            created_at_ms: now,
            updated_at_ms: now,
            instance_endpoint: None,
            last_health: None,
        };
        self.store.insert(tenant.clone())?;
        self.emit_audit(AuditEvent {
            id: Uuid::now_v7().to_string(),
            tenant_id: tenant.id.clone(),
            operation: "provision".to_string(),
            actor: "operator".to_string(),
            occurred_at_ms: now,
            previous_status: None,
            new_status: Some(TenantStatus::Provisioning),
        });
        tracing::info!(tenant_id = %tenant.id, "tenant provisioning started");
        Ok(tenant)
    }

    /// Fetch a tenant by id.
    pub fn get_tenant(&self, id: &TenantId) -> Result<Tenant, ControlPlaneError> {
        self.store.get(id)
    }

    /// List all tenants (sorted by creation time).
    pub fn list_tenants(&self) -> Result<Vec<Tenant>, ControlPlaneError> {
        self.store.list()
    }

    /// Move a tenant from `Provisioning` to `Active`.
    ///
    /// For hosted tenants this is called by the provisioning automation once
    /// the backing store is set up. For BYOC tenants it is called implicitly
    /// by [`Self::register_byoc_instance`] so the customer's instance
    /// appears in the dashboard as soon as it registers.
    pub fn mark_active(
        &self,
        id: &TenantId,
        instance_endpoint: Option<String>,
    ) -> Result<Tenant, ControlPlaneError> {
        self.transition(id, "mark_active", |tenant| {
            if !matches!(
                tenant.status,
                TenantStatus::Provisioning | TenantStatus::Suspended
            ) {
                return Err(ControlPlaneError::InvalidState {
                    tenant_id: tenant.id.to_string(),
                    current: tenant.status,
                    operation: "mark_active".into(),
                });
            }
            tenant.status = TenantStatus::Active;
            if instance_endpoint.is_some() {
                tenant.instance_endpoint = instance_endpoint.clone();
            }
            Ok(())
        })
    }

    /// Administratively suspend an active tenant.
    pub fn suspend(&self, id: &TenantId) -> Result<Tenant, ControlPlaneError> {
        self.transition(id, "suspend", |tenant| {
            if tenant.status != TenantStatus::Active {
                return Err(ControlPlaneError::InvalidState {
                    tenant_id: tenant.id.to_string(),
                    current: tenant.status,
                    operation: "suspend".into(),
                });
            }
            tenant.status = TenantStatus::Suspended;
            Ok(())
        })
    }

    /// Begin deprovisioning a tenant.
    ///
    /// Moves the tenant into the `Deprovisioning` state. The retention policy
    /// declared on the spec governs when the tenant actually transitions to
    /// `Terminated` — that transition is driven by operator tooling, not by
    /// this call.
    pub fn deprovision(&self, id: &TenantId) -> Result<Tenant, ControlPlaneError> {
        self.transition(id, "deprovision", |tenant| {
            if tenant.status.is_terminal() || tenant.status == TenantStatus::Deprovisioning {
                return Err(ControlPlaneError::InvalidState {
                    tenant_id: tenant.id.to_string(),
                    current: tenant.status,
                    operation: "deprovision".into(),
                });
            }
            tenant.status = TenantStatus::Deprovisioning;
            Ok(())
        })
    }

    /// Mark a deprovisioned tenant as fully terminated (retention expired).
    pub fn terminate(&self, id: &TenantId) -> Result<Tenant, ControlPlaneError> {
        self.transition(id, "terminate", |tenant| {
            if tenant.status != TenantStatus::Deprovisioning {
                return Err(ControlPlaneError::InvalidState {
                    tenant_id: tenant.id.to_string(),
                    current: tenant.status,
                    operation: "terminate".into(),
                });
            }
            tenant.status = TenantStatus::Terminated;
            Ok(())
        })
    }

    /// Register a BYOC instance against an existing tenant.
    ///
    /// This is the endpoint a customer-hosted Axon instance calls to announce
    /// itself to the control plane. The tenant must have been pre-provisioned
    /// in `Provisioning` state with `DeploymentMode::Byoc`. Registration
    /// records the reachable endpoint and promotes the tenant to `Active`.
    pub fn register_byoc_instance(
        &self,
        id: &TenantId,
        instance_endpoint: String,
    ) -> Result<Tenant, ControlPlaneError> {
        if instance_endpoint.trim().is_empty() {
            return Err(ControlPlaneError::InvalidArgument(
                "instance_endpoint must not be empty".into(),
            ));
        }
        self.transition(id, "register_byoc", |tenant| {
            if tenant.spec.deployment_mode != DeploymentMode::Byoc {
                return Err(ControlPlaneError::InvalidArgument(format!(
                    "tenant {} is not a BYOC deployment",
                    tenant.id
                )));
            }
            if tenant.status.is_terminal() {
                return Err(ControlPlaneError::InvalidState {
                    tenant_id: tenant.id.to_string(),
                    current: tenant.status,
                    operation: "register_byoc".into(),
                });
            }
            tenant.status = TenantStatus::Active;
            tenant.instance_endpoint = Some(instance_endpoint.clone());
            Ok(())
        })
    }

    /// Record a health report for a tenant.
    ///
    /// Rejected if the tenant is in a non-observable state (e.g. already
    /// terminated), so stale agents cannot resurrect decommissioned tenants.
    pub fn record_health(
        &self,
        id: &TenantId,
        mut report: HealthReport,
    ) -> Result<Tenant, ControlPlaneError> {
        report
            .validate()
            .map_err(ControlPlaneError::InvalidArgument)?;
        // Stamp the report with the control plane's clock if the tenant did
        // not provide one. This keeps out-of-sync instance clocks from
        // producing "reports from the future" in dashboards.
        if report.reported_at_ms == 0 {
            report.reported_at_ms = self.clock.now_ms();
        }
        let mut tenant = self.store.get(id)?;
        if !tenant.status.is_observable() {
            return Err(ControlPlaneError::InvalidState {
                tenant_id: tenant.id.to_string(),
                current: tenant.status,
                operation: "record_health".into(),
            });
        }
        tenant.last_health = Some(report);
        tenant.updated_at_ms = self.clock.now_ms();
        self.store.update(tenant.clone())?;
        Ok(tenant)
    }

    /// Issue a short-lived observation credential for the given tenant.
    ///
    /// `ttl_ms` must be ≤ 24 hours (86_400_000 ms). Returns
    /// [`ControlPlaneError::InvalidArgument`] if the TTL is too large.
    pub fn issue_observation_credential(
        &self,
        tenant_id: &TenantId,
        scope: ObservationScope,
        ttl_ms: u64,
    ) -> Result<ObservationCredential, ControlPlaneError> {
        const MAX_TTL_MS: u64 = 24 * 3_600_000;
        if ttl_ms > MAX_TTL_MS {
            return Err(ControlPlaneError::InvalidArgument(format!(
                "ttl_ms {ttl_ms} exceeds the maximum of {MAX_TTL_MS}ms (24h)"
            )));
        }
        // Ensure tenant exists — propagates TenantNotFound if not.
        self.store.get(tenant_id)?;
        let now = self.clock.now_ms();
        let cred = ObservationCredential {
            id: Uuid::now_v7().to_string(),
            tenant_id: tenant_id.clone(),
            issued_at_ms: now,
            expires_at_ms: now.saturating_add(ttl_ms),
            scope,
        };
        self.store.insert_credential(cred.clone())?;
        tracing::info!(
            tenant_id = %tenant_id,
            cred_id = %cred.id,
            ttl_ms,
            "observation credential issued"
        );
        Ok(cred)
    }

    /// Retrieve a credential and verify it has not expired.
    ///
    /// Returns [`ControlPlaneError::CredentialNotFound`] when the credential
    /// does not exist or belongs to a different tenant.
    /// Returns [`ControlPlaneError::CredentialExpired`] when `now_ms` is at or
    /// past `expires_at_ms`.
    pub fn get_observation_credential(
        &self,
        tenant_id: &TenantId,
        cred_id: &str,
    ) -> Result<ObservationCredential, ControlPlaneError> {
        let cred = self
            .store
            .get_credential(cred_id)
            .map_err(|_| ControlPlaneError::CredentialNotFound(cred_id.to_string()))?;
        if cred.tenant_id != *tenant_id {
            return Err(ControlPlaneError::CredentialNotFound(cred_id.to_string()));
        }
        let now = self.clock.now_ms();
        if cred.is_expired(now) {
            return Err(ControlPlaneError::CredentialExpired(cred_id.to_string()));
        }
        Ok(cred)
    }

    /// Revoke an observation credential, removing it from the store.
    ///
    /// Returns [`ControlPlaneError::CredentialNotFound`] when the credential
    /// does not exist or belongs to a different tenant.
    pub fn revoke_observation_credential(
        &self,
        tenant_id: &TenantId,
        cred_id: &str,
    ) -> Result<(), ControlPlaneError> {
        let cred = self
            .store
            .get_credential(cred_id)
            .map_err(|_| ControlPlaneError::CredentialNotFound(cred_id.to_string()))?;
        if cred.tenant_id != *tenant_id {
            return Err(ControlPlaneError::CredentialNotFound(cred_id.to_string()));
        }
        self.store.delete_credential(cred_id)?;
        tracing::info!(tenant_id = %tenant_id, cred_id, "observation credential revoked");
        Ok(())
    }

    /// Shared transition helper.
    ///
    /// Reads the tenant, runs a caller-supplied mutator, and writes the
    /// result back. The mutator is responsible for validating that the
    /// transition is legal from the current state. Emits an audit event
    /// recording the before/after lifecycle state.
    fn transition<F>(
        &self,
        id: &TenantId,
        op: &'static str,
        mutate: F,
    ) -> Result<Tenant, ControlPlaneError>
    where
        F: FnOnce(&mut Tenant) -> Result<(), ControlPlaneError>,
    {
        let mut tenant = self.store.get(id)?;
        let before_status = tenant.status;
        mutate(&mut tenant)?;
        tenant.updated_at_ms = self.clock.now_ms();
        self.store.update(tenant.clone())?;
        self.emit_audit(AuditEvent {
            id: Uuid::now_v7().to_string(),
            tenant_id: tenant.id.clone(),
            operation: op.to_string(),
            actor: "operator".to_string(),
            occurred_at_ms: tenant.updated_at_ms,
            previous_status: Some(before_status),
            new_status: Some(tenant.status),
        });
        tracing::info!(tenant_id = %tenant.id, status = ?tenant.status, op, "tenant transition");
        Ok(tenant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BackingStore, DataRetentionPolicy, HealthStatus};
    use crate::store::InMemoryControlPlaneStore;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Monotonic test clock — each read advances by 1ms.
    #[derive(Debug, Default)]
    struct TickClock(AtomicU64);

    impl Clock for TickClock {
        fn now_ms(&self) -> u64 {
            self.0.fetch_add(1, Ordering::SeqCst) + 1
        }
    }

    fn service() -> ControlPlaneService {
        let store: Arc<dyn ControlPlaneStore> = Arc::new(InMemoryControlPlaneStore::new());
        let clock: Arc<dyn Clock> = Arc::new(TickClock::default());
        ControlPlaneService::with_clock(store, clock)
    }

    fn hosted_spec(name: &str) -> TenantSpec {
        TenantSpec {
            name: name.into(),
            deployment_mode: DeploymentMode::Hosted,
            backing_store: BackingStore::Memory,
            retention: DataRetentionPolicy::default(),
            labels: BTreeMap::new(),
        }
    }

    fn byoc_spec(name: &str) -> TenantSpec {
        TenantSpec {
            name: name.into(),
            deployment_mode: DeploymentMode::Byoc,
            backing_store: BackingStore::Postgres {
                uri: "postgres://byoc@customer.example/axon".into(),
                region: Some("customer-vpc".into()),
            },
            retention: DataRetentionPolicy::Retain,
            labels: BTreeMap::new(),
        }
    }

    #[test]
    fn provision_tenant_starts_in_provisioning_state() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("prod")).unwrap();
        assert_eq!(t.status, TenantStatus::Provisioning);
        assert!(!t.id.as_str().is_empty());
        assert!(t.created_at_ms > 0);
    }

    #[test]
    fn provision_rejects_empty_name() {
        let svc = service();
        let err = svc.provision_tenant(hosted_spec("   ")).unwrap_err();
        assert!(matches!(err, ControlPlaneError::InvalidArgument(_)));
    }

    #[test]
    fn mark_active_promotes_from_provisioning() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("prod")).unwrap();
        let active = svc
            .mark_active(&t.id, Some("https://prod.example:50051".into()))
            .unwrap();
        assert_eq!(active.status, TenantStatus::Active);
        assert_eq!(
            active.instance_endpoint.as_deref(),
            Some("https://prod.example:50051")
        );
    }

    #[test]
    fn suspend_and_reactivate_roundtrip() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("prod")).unwrap();
        svc.mark_active(&t.id, None).unwrap();
        let suspended = svc.suspend(&t.id).unwrap();
        assert_eq!(suspended.status, TenantStatus::Suspended);
        let active = svc.mark_active(&t.id, None).unwrap();
        assert_eq!(active.status, TenantStatus::Active);
    }

    #[test]
    fn deprovision_then_terminate() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("old")).unwrap();
        svc.mark_active(&t.id, None).unwrap();
        let deprov = svc.deprovision(&t.id).unwrap();
        assert_eq!(deprov.status, TenantStatus::Deprovisioning);
        let term = svc.terminate(&t.id).unwrap();
        assert_eq!(term.status, TenantStatus::Terminated);
    }

    #[test]
    fn cannot_deprovision_twice() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("x")).unwrap();
        svc.mark_active(&t.id, None).unwrap();
        svc.deprovision(&t.id).unwrap();
        let err = svc.deprovision(&t.id).unwrap_err();
        assert!(matches!(err, ControlPlaneError::InvalidState { .. }));
    }

    #[test]
    fn terminate_requires_deprovisioning_state() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("x")).unwrap();
        let err = svc.terminate(&t.id).unwrap_err();
        assert!(matches!(err, ControlPlaneError::InvalidState { .. }));
    }

    #[test]
    fn byoc_registration_activates_tenant() {
        let svc = service();
        let t = svc.provision_tenant(byoc_spec("customer-a")).unwrap();
        let registered = svc
            .register_byoc_instance(&t.id, "https://axon.customer-a.example".into())
            .unwrap();
        assert_eq!(registered.status, TenantStatus::Active);
        assert_eq!(
            registered.instance_endpoint.as_deref(),
            Some("https://axon.customer-a.example")
        );
    }

    #[test]
    fn byoc_registration_rejects_hosted_tenant() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("prod")).unwrap();
        let err = svc
            .register_byoc_instance(&t.id, "https://rogue".into())
            .unwrap_err();
        assert!(matches!(err, ControlPlaneError::InvalidArgument(_)));
    }

    #[test]
    fn byoc_registration_rejects_terminated_tenant() {
        let svc = service();
        let t = svc.provision_tenant(byoc_spec("c1")).unwrap();
        svc.register_byoc_instance(&t.id, "https://first".into())
            .unwrap();
        svc.deprovision(&t.id).unwrap();
        svc.terminate(&t.id).unwrap();
        let err = svc
            .register_byoc_instance(&t.id, "https://zombie".into())
            .unwrap_err();
        assert!(matches!(err, ControlPlaneError::InvalidState { .. }));
    }

    #[test]
    fn health_reports_stored_on_active_tenant() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("prod")).unwrap();
        svc.mark_active(&t.id, None).unwrap();
        let updated = svc
            .record_health(
                &t.id,
                HealthReport {
                    reported_at_ms: 1_700_000_000_000,
                    status: HealthStatus::Healthy,
                    instance_version: Some("0.1.0".into()),
                    storage_bytes: Some(4096),
                    open_connections: Some(5),
                    p99_latency_ms: Some(20),
                    error_rate: Some(0.001),
                },
            )
            .unwrap();
        let stored = updated.last_health.unwrap();
        assert_eq!(stored.status, HealthStatus::Healthy);
        assert_eq!(stored.storage_bytes, Some(4096));
    }

    #[test]
    fn health_reports_rejected_on_terminated_tenant() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("old")).unwrap();
        svc.mark_active(&t.id, None).unwrap();
        svc.deprovision(&t.id).unwrap();
        svc.terminate(&t.id).unwrap();
        let err = svc
            .record_health(
                &t.id,
                HealthReport {
                    reported_at_ms: 1,
                    status: HealthStatus::Healthy,
                    instance_version: None,
                    storage_bytes: None,
                    open_connections: None,
                    p99_latency_ms: None,
                    error_rate: None,
                },
            )
            .unwrap_err();
        assert!(matches!(err, ControlPlaneError::InvalidState { .. }));
    }

    #[test]
    fn issue_credential_ttl_within_24h() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("t")).unwrap();
        let cred = svc
            .issue_observation_credential(
                &t.id,
                crate::model::ObservationScope::HealthOnly,
                3_600_000,
            )
            .unwrap();
        assert_eq!(cred.tenant_id, t.id);
        assert!(cred.ttl_ms() <= 24 * 3_600_000);
        assert_eq!(cred.ttl_ms(), 3_600_000);
    }

    #[test]
    fn issue_credential_rejects_ttl_exceeding_24h() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("t")).unwrap();
        let too_long = 24 * 3_600_000 + 1;
        let err = svc
            .issue_observation_credential(
                &t.id,
                crate::model::ObservationScope::HealthOnly,
                too_long,
            )
            .unwrap_err();
        assert!(matches!(err, ControlPlaneError::InvalidArgument(_)));
    }

    #[test]
    fn issue_credential_against_unknown_tenant_returns_not_found() {
        let svc = service();
        let err = svc
            .issue_observation_credential(
                &TenantId::new("does-not-exist"),
                crate::model::ObservationScope::HealthOnly,
                3_600_000,
            )
            .unwrap_err();
        assert!(matches!(err, ControlPlaneError::TenantNotFound(_)));
    }

    #[test]
    fn get_credential_expired_returns_credential_expired() {
        // Issue at t=0 with ttl=1ms, verify at t=2 → expired.
        let store: Arc<dyn ControlPlaneStore> = Arc::new(InMemoryControlPlaneStore::new());

        #[derive(Debug)]
        struct FixedClock(u64);
        impl Clock for FixedClock {
            fn now_ms(&self) -> u64 {
                self.0
            }
        }

        let svc_past = ControlPlaneService::with_clock(
            Arc::clone(&store),
            Arc::new(FixedClock(0)),
        );
        let svc_present = ControlPlaneService::with_clock(
            Arc::clone(&store),
            Arc::new(FixedClock(2)),
        );

        let t = svc_past.provision_tenant(hosted_spec("t")).unwrap();
        let cred = svc_past
            .issue_observation_credential(
                &t.id,
                crate::model::ObservationScope::HealthOnly,
                1,
            )
            .unwrap();
        assert_eq!(cred.expires_at_ms, 1);

        let err = svc_present
            .get_observation_credential(&t.id, &cred.id)
            .unwrap_err();
        assert!(
            matches!(err, ControlPlaneError::CredentialExpired(_)),
            "expected CredentialExpired, got {err:?}"
        );
    }

    #[test]
    fn revoke_credential_removes_it_from_store() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("t")).unwrap();
        let cred = svc
            .issue_observation_credential(
                &t.id,
                crate::model::ObservationScope::MetricsRead,
                3_600_000,
            )
            .unwrap();

        svc.revoke_observation_credential(&t.id, &cred.id).unwrap();

        let err = svc
            .get_observation_credential(&t.id, &cred.id)
            .unwrap_err();
        assert!(matches!(err, ControlPlaneError::CredentialNotFound(_)));
    }

    #[test]
    fn revoke_unknown_credential_returns_not_found() {
        let svc = service();
        let t = svc.provision_tenant(hosted_spec("t")).unwrap();
        let err = svc
            .revoke_observation_credential(&t.id, "no-such-cred")
            .unwrap_err();
        assert!(matches!(err, ControlPlaneError::CredentialNotFound(_)));
    }

    #[test]
    fn list_returns_provisioned_tenants_in_order() {
        let svc = service();
        let a = svc.provision_tenant(hosted_spec("a")).unwrap();
        let b = svc.provision_tenant(hosted_spec("b")).unwrap();
        let c = svc.provision_tenant(hosted_spec("c")).unwrap();
        let listed = svc.list_tenants().unwrap();
        assert_eq!(listed.len(), 3);
        let ids: Vec<_> = listed.iter().map(|t| t.id.clone()).collect();
        assert_eq!(ids, vec![a.id, b.id, c.id]);
    }
}
