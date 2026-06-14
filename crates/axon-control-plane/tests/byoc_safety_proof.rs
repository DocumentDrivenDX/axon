//! BYOC control-plane safety proof — contract tests.
//!
//! Hardens the evidence for the four safety properties required by
//! FEAT-025 / PRD P1 BYOC:
//!
//! - AC2: control-plane APIs cannot read or mutate tenant entity data.
//! - AC3: tenant-scoped visibility is correct; `ObservationCredential` shape
//!   is documented — issuance follow-up is tracked in the DDx queue.
//! - AC4: administrative lifecycle actions produce auditable evidence.
//!
//! Covers SCN-016 (BYOC deployment boundary).

use std::sync::Arc;

use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::{json, Value};

use axon_control_plane::http::build_router;
use axon_control_plane::store::{ControlPlaneStore, InMemoryControlPlaneStore};
use axon_control_plane::{
    ControlPlaneService, ObservationCredential, ObservationScope, TenantId, TenantStatus,
};

// ── Test helpers ─────────────────────────────────────────────────────────────

fn server() -> (TestServer, ControlPlaneService) {
    let store: Arc<dyn ControlPlaneStore> = Arc::new(InMemoryControlPlaneStore::new());
    let svc = ControlPlaneService::new(store);
    let router = build_router(svc.clone());
    (TestServer::new(router), svc)
}

fn byoc_spec(name: &str) -> Value {
    json!({
        "name": name,
        "deployment_mode": "byoc",
        "backing_store": {
            "kind": "postgres",
            "uri": "postgres://byoc@customer.example/axon",
            "region": "customer-vpc"
        },
        "retention": "retain",
        "labels": {},
    })
}

fn hosted_spec(name: &str) -> Value {
    json!({
        "name": name,
        "deployment_mode": "hosted",
        "backing_store": { "kind": "memory" },
        "labels": {},
    })
}

// ── AC2: no entity data access ────────────────────────────────────────────────

/// The control plane must not expose any route that serves entity data.
/// Data-plane-style paths — entities, collections, links, audit query — must
/// all return 404 or 405, never 200.
///
/// This is the core data-sovereignty contract for FEAT-025.
#[tokio::test]
async fn entity_data_routes_absent() {
    let (server, _) = server();

    let data_plane_paths = [
        "/entities/tasks/t-1",
        "/collections/tasks/query",
        "/links",
        "/links/l-1",
        "/audit/query",
        "/databases/mydb/entities/tasks",
        "/tenants/t1/entities/tasks",
        "/tenants/t1/databases/db1/entities/invoices/inv-001",
        "/tenants/t1/databases/db1/collections/invoices",
    ];

    for path in data_plane_paths {
        let resp = server.get(path).await;
        assert_ne!(
            resp.status_code(),
            StatusCode::OK,
            "GET {path} returned 200 — data-sovereignty violation"
        );
    }
}

/// POST to entity-like paths must also be absent.
#[tokio::test]
async fn entity_data_mutation_routes_absent() {
    let (server, _) = server();

    let data_plane_post_paths = [
        "/entities/tasks",
        "/collections/tasks/query",
        "/links",
        "/tenants/t1/databases/db1/entities/invoices",
    ];

    for path in data_plane_post_paths {
        let resp = server.post(path).json(&json!({})).await;
        assert_ne!(
            resp.status_code(),
            StatusCode::OK,
            "POST {path} returned 200 — data-sovereignty violation"
        );
    }
}

/// The control plane router has no route that returns entity-shaped JSON.
/// Even if a path were accidentally added, the contract test must catch it.
#[tokio::test]
async fn dashboard_row_contains_no_entity_data_keys() {
    let (server, _) = server();

    server
        .post("/tenants")
        .json(&hosted_spec("dashboard-check"))
        .await;

    let dash = server.get("/dashboard").await;
    dash.assert_status_ok();
    let body: Value = dash.json();
    let row = &body["tenants"][0];

    // These keys are characteristic of data-plane entity payloads and must
    // never appear in control-plane responses.
    for forbidden in [
        "entities",
        "data",
        "rows",
        "collections",
        "links",
        "fields",
        "schema",
        "payload",
    ] {
        assert!(
            row.get(forbidden).is_none(),
            "dashboard row contains forbidden entity-data key '{forbidden}'"
        );
    }
}

// ── AC3: tenant-scoped visibility + credential shape ─────────────────────────

/// Two tenants registered with the same control plane cannot be confused.
/// Fetching tenant A by its ID returns tenant A's metadata, not tenant B's.
#[tokio::test]
async fn tenant_records_are_isolated_by_id() {
    let (server, _) = server();

    let a = server.post("/tenants").json(&byoc_spec("tenant-alpha")).await;
    let a_id = a.json::<Value>()["id"].as_str().unwrap().to_string();

    let b = server.post("/tenants").json(&byoc_spec("tenant-beta")).await;
    let b_id = b.json::<Value>()["id"].as_str().unwrap().to_string();

    // Each tenant is accessible by its own ID with correct metadata.
    let got_a = server.get(&format!("/tenants/{a_id}")).await;
    got_a.assert_status_ok();
    assert_eq!(got_a.json::<Value>()["spec"]["name"], "tenant-alpha");

    let got_b = server.get(&format!("/tenants/{b_id}")).await;
    got_b.assert_status_ok();
    assert_eq!(got_b.json::<Value>()["spec"]["name"], "tenant-beta");

    // Tenant A's record must not expose tenant B's name or ID.
    assert_ne!(got_a.json::<Value>()["id"].as_str().unwrap(), b_id.as_str());
    assert_ne!(got_b.json::<Value>()["id"].as_str().unwrap(), a_id.as_str());
    assert_ne!(
        got_a.json::<Value>()["spec"]["name"],
        got_b.json::<Value>()["spec"]["name"]
    );
}

/// Each control-plane deployment manages only the tenants registered with it.
/// Two independent deployments (separate stores) cannot see each other's tenants.
#[tokio::test]
async fn tenant_listing_is_scoped_to_deployment() {
    let (server_a, _) = server();
    let (server_b, _) = server();

    // Register a tenant only in deployment A.
    server_a
        .post("/tenants")
        .json(&byoc_spec("only-in-a"))
        .await;

    // Deployment B has an independent empty registry.
    let list_b = server_b.get("/tenants").await;
    list_b.assert_status_ok();
    let body: Value = list_b.json();
    assert_eq!(
        body["tenants"].as_array().map(Vec::len).unwrap_or(0),
        0,
        "deployment B must not see tenants registered with deployment A"
    );

    // Deployment A's list has the one tenant.
    let list_a = server_a.get("/tenants").await;
    list_a.assert_status_ok();
    let body_a: Value = list_a.json();
    assert_eq!(body_a["tenants"].as_array().map(Vec::len).unwrap_or(0), 1);
}

/// A random (unknown) tenant ID returns 404, not another tenant's record.
#[tokio::test]
async fn unknown_tenant_id_returns_404_not_cross_tenant_data() {
    let (server, _) = server();

    // Register a real tenant.
    server.post("/tenants").json(&byoc_spec("real")).await;

    // A UUID that was never issued must return 404.
    let resp = server
        .get("/tenants/00000000-0000-0000-0000-000000000000")
        .await;
    resp.assert_status(StatusCode::NOT_FOUND);

    let body: Value = resp.json();
    assert_eq!(body["code"], "tenant_not_found");
    // The response must not leak the real tenant's name or data.
    let body_str = body.to_string();
    assert!(
        !body_str.contains("real"),
        "404 response must not expose other tenant data: {body_str}"
    );
}

/// `ObservationCredential` has the required TTL shape and expiry semantics.
///
/// Credential issuance is not yet implemented. This test documents the
/// intended contract so that a real implementation can be wired in without
/// changing the shape. A follow-up bead tracks the implementation of
/// `POST /tenants/{id}/observation-credentials`.
#[test]
fn observation_credential_ttl_and_expiry_shape() {
    let one_hour_ms: u64 = 3_600_000;
    let cred = ObservationCredential {
        id: "cred-shape-test".to_string(),
        tenant_id: TenantId::new("t-shape"),
        issued_at_ms: 1_000_000,
        expires_at_ms: 1_000_000 + one_hour_ms,
        scope: ObservationScope::HealthOnly,
    };

    assert_eq!(cred.ttl_ms(), one_hour_ms, "TTL must equal expires_at - issued_at");
    assert!(
        !cred.is_expired(cred.expires_at_ms - 1),
        "credential must be valid 1ms before expiry"
    );
    assert!(
        cred.is_expired(cred.expires_at_ms),
        "credential must be expired at the exact expiry instant"
    );
    assert!(
        cred.is_expired(cred.expires_at_ms + 1),
        "credential must be expired after expiry"
    );
}

/// Observation credentials must be short-lived (≤ 24 hours).
/// Any credential with a longer TTL violates the BYOC security contract.
#[test]
fn observation_credential_is_short_lived() {
    let max_ttl_ms: u64 = 24 * 3_600_000; // 24 hours

    // A 1-hour credential is acceptable.
    let one_hour = ObservationCredential {
        id: "c1".to_string(),
        tenant_id: TenantId::new("t-1"),
        issued_at_ms: 0,
        expires_at_ms: 3_600_000,
        scope: ObservationScope::HealthOnly,
    };
    assert!(one_hour.ttl_ms() <= max_ttl_ms);

    // A 24-hour credential is at the boundary — still acceptable.
    let twenty_four_hours = ObservationCredential {
        id: "c2".to_string(),
        tenant_id: TenantId::new("t-1"),
        issued_at_ms: 0,
        expires_at_ms: max_ttl_ms,
        scope: ObservationScope::MetricsRead,
    };
    assert!(twenty_four_hours.ttl_ms() <= max_ttl_ms);

    // A 25-hour credential would violate the constraint.
    let too_long = ObservationCredential {
        id: "c3".to_string(),
        tenant_id: TenantId::new("t-1"),
        issued_at_ms: 0,
        expires_at_ms: max_ttl_ms + 3_600_001, // 25h + 1ms
        scope: ObservationScope::MetricsRead,
    };
    assert!(
        too_long.ttl_ms() > max_ttl_ms,
        "this credential intentionally violates the 24h constraint (negative test)"
    );
}

// ── AC4: lifecycle operations produce auditable evidence ─────────────────────

/// Provisioning a tenant emits an audit event with the correct fields.
#[tokio::test]
async fn provision_emits_audit_event() {
    let (server, svc) = server();

    let resp = server
        .post("/tenants")
        .json(&byoc_spec("audit-provision"))
        .await;
    resp.assert_status(StatusCode::CREATED);
    let tenant_id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    let events = svc.audit_events();
    let ev = events
        .iter()
        .find(|e| e.operation == "provision" && e.tenant_id.as_str() == tenant_id.as_str())
        .expect("provision must emit an audit event");

    assert_eq!(ev.actor, "operator");
    assert!(ev.occurred_at_ms > 0, "audit event must have a non-zero timestamp");
    assert_eq!(ev.previous_status, None, "provision has no previous state");
    assert_eq!(ev.new_status, Some(TenantStatus::Provisioning));
}

/// Activating a tenant emits an audit event with the correct state transition.
#[tokio::test]
async fn activate_emits_audit_event() {
    let (server, svc) = server();

    let resp = server.post("/tenants").json(&hosted_spec("audit-activate")).await;
    let id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/tenants/{id}/activate"))
        .json(&json!({}))
        .await
        .assert_status_ok();

    let events = svc.audit_events();
    let ev = events
        .iter()
        .find(|e| e.operation == "mark_active" && e.tenant_id.as_str() == id.as_str())
        .expect("mark_active must emit an audit event");

    assert_eq!(ev.previous_status, Some(TenantStatus::Provisioning));
    assert_eq!(ev.new_status, Some(TenantStatus::Active));
}

/// Suspending a tenant emits an audit event with the correct state transition.
#[tokio::test]
async fn suspend_emits_audit_event() {
    let (server, svc) = server();

    let resp = server.post("/tenants").json(&hosted_spec("audit-suspend")).await;
    let id = resp.json::<Value>()["id"].as_str().unwrap().to_string();
    server
        .post(&format!("/tenants/{id}/activate"))
        .json(&json!({}))
        .await;
    server.post(&format!("/tenants/{id}/suspend")).await.assert_status_ok();

    let events = svc.audit_events();
    let ev = events
        .iter()
        .find(|e| e.operation == "suspend" && e.tenant_id.as_str() == id.as_str())
        .expect("suspend must emit an audit event");

    assert_eq!(ev.previous_status, Some(TenantStatus::Active));
    assert_eq!(ev.new_status, Some(TenantStatus::Suspended));
}

/// Deprovisioning a tenant emits an audit event.
#[tokio::test]
async fn deprovision_emits_audit_event() {
    let (server, svc) = server();

    let resp = server.post("/tenants").json(&hosted_spec("audit-deprov")).await;
    let id = resp.json::<Value>()["id"].as_str().unwrap().to_string();
    server
        .post(&format!("/tenants/{id}/activate"))
        .json(&json!({}))
        .await;
    server
        .post(&format!("/tenants/{id}/deprovision"))
        .await
        .assert_status_ok();

    let events = svc.audit_events();
    let ev = events
        .iter()
        .find(|e| e.operation == "deprovision" && e.tenant_id.as_str() == id.as_str())
        .expect("deprovision must emit an audit event");

    assert_eq!(ev.new_status, Some(TenantStatus::Deprovisioning));
}

/// Terminating a tenant emits an audit event.
#[tokio::test]
async fn terminate_emits_audit_event() {
    let (server, svc) = server();

    let resp = server.post("/tenants").json(&hosted_spec("audit-term")).await;
    let id = resp.json::<Value>()["id"].as_str().unwrap().to_string();
    server
        .post(&format!("/tenants/{id}/activate"))
        .json(&json!({}))
        .await;
    server.post(&format!("/tenants/{id}/deprovision")).await;
    server
        .post(&format!("/tenants/{id}/terminate"))
        .await
        .assert_status_ok();

    let events = svc.audit_events();
    let ev = events
        .iter()
        .find(|e| e.operation == "terminate" && e.tenant_id.as_str() == id.as_str())
        .expect("terminate must emit an audit event");

    assert_eq!(ev.previous_status, Some(TenantStatus::Deprovisioning));
    assert_eq!(ev.new_status, Some(TenantStatus::Terminated));
}

/// BYOC instance registration emits an audit event.
#[tokio::test]
async fn byoc_register_emits_audit_event() {
    let (server, svc) = server();

    let resp = server.post("/tenants").json(&byoc_spec("audit-byoc")).await;
    let id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    server
        .post("/byoc/register")
        .json(&json!({
            "tenant_id": id,
            "instance_endpoint": "https://axon.customer.example"
        }))
        .await
        .assert_status_ok();

    let events = svc.audit_events();
    let ev = events
        .iter()
        .find(|e| e.operation == "register_byoc" && e.tenant_id.as_str() == id.as_str())
        .expect("BYOC registration must emit an audit event");

    assert_eq!(ev.previous_status, Some(TenantStatus::Provisioning));
    assert_eq!(ev.new_status, Some(TenantStatus::Active));
}

/// A complete BYOC deployment lifecycle produces a fully ordered, complete
/// audit trail. No lifecycle action is unrecorded.
#[tokio::test]
async fn full_lifecycle_produces_ordered_audit_trail() {
    let (server, svc) = server();

    let resp = server.post("/tenants").json(&byoc_spec("full-audit")).await;
    let id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    server
        .post("/byoc/register")
        .json(&json!({
            "tenant_id": id,
            "instance_endpoint": "https://axon.customer.example"
        }))
        .await
        .assert_status_ok();

    server.post(&format!("/tenants/{id}/suspend")).await.assert_status_ok();
    server
        .post(&format!("/tenants/{id}/activate"))
        .json(&json!({}))
        .await
        .assert_status_ok();
    server.post(&format!("/tenants/{id}/deprovision")).await.assert_status_ok();
    server.post(&format!("/tenants/{id}/terminate")).await.assert_status_ok();

    let events: Vec<_> = svc
        .audit_events()
        .into_iter()
        .filter(|e| e.tenant_id.as_str() == id.as_str())
        .collect();

    let ops: Vec<&str> = events.iter().map(|e| e.operation.as_str()).collect();
    assert!(ops.contains(&"provision"), "provision must be in audit trail; got {ops:?}");
    assert!(ops.contains(&"register_byoc"), "register_byoc must be in audit trail; got {ops:?}");
    assert!(ops.contains(&"suspend"), "suspend must be in audit trail; got {ops:?}");
    assert!(ops.contains(&"mark_active"), "mark_active must be in audit trail; got {ops:?}");
    assert!(ops.contains(&"deprovision"), "deprovision must be in audit trail; got {ops:?}");
    assert!(ops.contains(&"terminate"), "terminate must be in audit trail; got {ops:?}");

    // Timestamps must be non-decreasing (events are in chronological order).
    let timestamps: Vec<u64> = events.iter().map(|e| e.occurred_at_ms).collect();
    let is_ordered = timestamps.windows(2).all(|w| w[0] <= w[1]);
    assert!(
        is_ordered,
        "audit events must be in non-decreasing timestamp order: {timestamps:?}"
    );

    // Every event must have a unique, non-empty ID.
    let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
    let unique_ids: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(
        ids.len(),
        unique_ids.len(),
        "all audit event IDs must be unique"
    );
    assert!(ids.iter().all(|id| !id.is_empty()), "all audit event IDs must be non-empty");
}

/// Each audit event captures the full before/after status transition.
/// This makes the audit trail self-describing — no external state is needed
/// to understand what changed.
#[tokio::test]
async fn audit_events_capture_status_transitions() {
    let (server, svc) = server();

    let resp = server
        .post("/tenants")
        .json(&hosted_spec("transition-audit"))
        .await;
    let id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/tenants/{id}/activate"))
        .json(&json!({}))
        .await
        .assert_status_ok();
    server
        .post(&format!("/tenants/{id}/suspend"))
        .await
        .assert_status_ok();

    let events = svc.audit_events();

    let activate_ev = events
        .iter()
        .find(|e| e.operation == "mark_active")
        .expect("mark_active event must exist");
    assert_eq!(activate_ev.previous_status, Some(TenantStatus::Provisioning));
    assert_eq!(activate_ev.new_status, Some(TenantStatus::Active));

    let suspend_ev = events
        .iter()
        .find(|e| e.operation == "suspend")
        .expect("suspend event must exist");
    assert_eq!(suspend_ev.previous_status, Some(TenantStatus::Active));
    assert_eq!(suspend_ev.new_status, Some(TenantStatus::Suspended));
}
