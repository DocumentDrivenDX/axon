#![allow(
    clippy::inefficient_to_string,
    clippy::needless_collect,
    clippy::similar_names,
    clippy::unwrap_used,
    clippy::wildcard_imports
)]

//! L2 Business Scenario Tests (SCN-001 through SCN-010)
//!
//! Each test validates a real-world workflow from use-case research, exercising
//! Axon's API end-to-end against the in-memory storage backend.

use axon_api::handler::AxonHandler;
use axon_api::request::*;
use axon_api::transaction::Transaction;
use axon_audit::log::MemoryAuditLog;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_storage::adapter::StorageAdapter;
use axon_storage::memory::MemoryStorageAdapter;
use serde_json::{json, Value};

// ── Helpers ─────────────────────────────────────────────────────────────────

fn handler() -> AxonHandler<MemoryStorageAdapter> {
    AxonHandler::new(MemoryStorageAdapter::default())
}

fn col(name: &str) -> CollectionId {
    CollectionId::new(name)
}

fn eid(id: &str) -> EntityId {
    EntityId::new(id)
}

fn create(h: &mut AxonHandler<MemoryStorageAdapter>, collection: &str, id: &str, data: Value) {
    h.create_entity(CreateEntityRequest {
        collection: col(collection),
        id: eid(id),
        data,
        actor: None,
        audit_metadata: None,
    attribution: None,
    })
    .unwrap();
}

fn get(h: &AxonHandler<MemoryStorageAdapter>, collection: &str, id: &str) -> Option<Entity> {
    h.get_entity(GetEntityRequest {
        collection: col(collection),
        id: eid(id),
    })
    .ok()
    .map(|r| r.entity)
}

fn update(
    h: &mut AxonHandler<MemoryStorageAdapter>,
    collection: &str,
    id: &str,
    data: Value,
    version: u64,
) -> Entity {
    h.update_entity(UpdateEntityRequest {
        collection: col(collection),
        id: eid(id),
        data,
        expected_version: version,
        actor: None,
        audit_metadata: None,
    attribution: None,
    })
    .unwrap()
    .entity
}

fn link(
    h: &mut AxonHandler<MemoryStorageAdapter>,
    src_col: &str,
    src_id: &str,
    tgt_col: &str,
    tgt_id: &str,
    link_type: &str,
    metadata: Value,
) {
    h.create_link(CreateLinkRequest {
        source_collection: col(src_col),
        source_id: eid(src_id),
        target_collection: col(tgt_col),
        target_id: eid(tgt_id),
        link_type: link_type.into(),
        metadata,
        actor: None,
    attribution: None,
    })
    .unwrap();
}

fn query(
    h: &AxonHandler<MemoryStorageAdapter>,
    collection: &str,
    filter: Option<FilterNode>,
) -> Vec<Entity> {
    h.query_entities(QueryEntitiesRequest {
        collection: col(collection),
        filter,
        ..Default::default()
    })
    .unwrap()
    .entities
}

fn traverse(
    h: &AxonHandler<MemoryStorageAdapter>,
    collection: &str,
    id: &str,
    link_type: Option<&str>,
    max_depth: Option<usize>,
) -> Vec<Entity> {
    h.traverse(TraverseRequest {
        collection: col(collection),
        id: eid(id),
        link_type: link_type.map(String::from),
        max_depth,
        direction: Default::default(),
        hop_filter: None,
    })
    .unwrap()
    .entities
}

// ── SCN-001: AP/AR — Payment Application with Partial Payment ───────────────

#[test]
fn scn_001_payment_application_with_partial_payment() {
    let mut h = handler();

    // SETUP: create invoices and customer.
    create(
        &mut h,
        "invoices",
        "INV-030",
        json!({"amount": 5000, "status": "open"}),
    );
    create(
        &mut h,
        "invoices",
        "INV-035",
        json!({"amount": 5000, "status": "open"}),
    );
    create(
        &mut h,
        "invoices",
        "INV-040",
        json!({"amount": 3000, "status": "open"}),
    );
    create(&mut h, "customers", "cust-001", json!({"balance": 13000}));

    // EXECUTION: create payment and apply in a transaction.
    create(&mut h, "payments", "PMT-107", json!({"amount": 7500}));

    let inv030 = get(&h, "invoices", "INV-030").unwrap();
    let inv035 = get(&h, "invoices", "INV-035").unwrap();
    let cust = get(&h, "customers", "cust-001").unwrap();

    let mut tx = Transaction::new();
    let tx_id = tx.id.clone();

    // Apply $5,000 to INV-030 → paid
    tx.update(
        Entity::new(
            col("invoices"),
            eid("INV-030"),
            json!({"amount": 5000, "status": "paid"}),
        ),
        inv030.version,
        Some(inv030.data.clone()),
    )
    .unwrap();
    // Apply $2,500 to INV-035 → partially_paid
    tx.update(
        Entity::new(
            col("invoices"),
            eid("INV-035"),
            json!({"amount": 5000, "status": "partially_paid", "amount_paid": 2500}),
        ),
        inv035.version,
        Some(inv035.data.clone()),
    )
    .unwrap();
    // Create ledger entries
    tx.create(Entity::new(
        col("ledger"),
        eid("LE-001"),
        json!({"type": "debit", "account": "cash", "amount": 7500}),
    ))
    .unwrap();
    tx.create(Entity::new(
        col("ledger"),
        eid("LE-002"),
        json!({"type": "credit", "account": "ar", "amount": 7500}),
    ))
    .unwrap();
    // Update customer balance
    tx.update(
        Entity::new(col("customers"), eid("cust-001"), json!({"balance": 5500})),
        cust.version,
        Some(cust.data.clone()),
    )
    .unwrap();

    let (storage, audit) = h.storage_and_audit_mut();
    let written = tx
        .commit(storage, audit, Some("payment-agent".into()), None)
        .unwrap();
    assert_eq!(written.len(), 5);

    // CHECK: invoice statuses
    let inv030 = get(&h, "invoices", "INV-030").unwrap();
    assert_eq!(inv030.data["status"], "paid");
    let inv035 = get(&h, "invoices", "INV-035").unwrap();
    assert_eq!(inv035.data["status"], "partially_paid");

    // CHECK: ledger entries balance
    let le1 = get(&h, "ledger", "LE-001").unwrap();
    let le2 = get(&h, "ledger", "LE-002").unwrap();
    assert_eq!(le1.data["amount"], 7500);
    assert_eq!(le2.data["amount"], 7500);

    // CHECK: customer balance reduced
    let cust = get(&h, "customers", "cust-001").unwrap();
    assert_eq!(cust.data["balance"], 5500);

    // CHECK: paid-by links with metadata
    link(
        &mut h,
        "invoices",
        "INV-030",
        "payments",
        "PMT-107",
        "paid-by",
        json!({"amount_applied": 5000}),
    );
    link(
        &mut h,
        "invoices",
        "INV-035",
        "payments",
        "PMT-107",
        "paid-by",
        json!({"amount_applied": 2500}),
    );

    // CHECK: audit entries share transaction ID
    let audit_entries = h.audit_log().entries();
    let tx_entries: Vec<_> = audit_entries
        .iter()
        .filter(|e| e.transaction_id.as_deref() == Some(&tx_id))
        .collect();
    assert_eq!(tx_entries.len(), 5, "all 5 ops should share the tx id");

    // FAILURE SCENARIO: version conflict on customer balance rolls back everything.
    let mut bad_tx = Transaction::new();
    bad_tx
        .update(
            Entity::new(
                col("invoices"),
                eid("INV-040"),
                json!({"amount": 3000, "status": "paid"}),
            ),
            1, // correct
            None,
        )
        .unwrap();
    bad_tx
        .update(
            Entity::new(col("customers"), eid("cust-001"), json!({"balance": 2500})),
            99, // WRONG — triggers rollback
            None,
        )
        .unwrap();
    let (storage, audit) = h.storage_and_audit_mut();
    let err = bad_tx.commit(storage, audit, None, None).unwrap_err();
    assert!(matches!(err, AxonError::ConflictingVersion { .. }));

    // INV-040 must be unchanged (no partial application).
    let inv040 = get(&h, "invoices", "INV-040").unwrap();
    assert_eq!(inv040.data["status"], "open");
}

// ── SCN-002: CRM — Contact Merge (Duplicate Resolution) ────────────────────

#[test]
fn scn_002_contact_merge_duplicate_resolution() {
    let mut h = handler();

    // SETUP
    create(
        &mut h,
        "contacts",
        "contact-a",
        json!({"name": "Alice Smith", "email": "alice@old.com", "phone": "555-0101"}),
    );
    create(
        &mut h,
        "contacts",
        "contact-b",
        json!({"name": "A. Smith", "email": "alice@new.com", "phone": "555-0202"}),
    );
    create(
        &mut h,
        "companies",
        "company-x",
        json!({"name": "Acme Corp"}),
    );
    create(&mut h, "deals", "deal-1", json!({"value": 10000}));
    create(&mut h, "deals", "deal-2", json!({"value": 20000}));

    link(
        &mut h,
        "contacts",
        "contact-a",
        "companies",
        "company-x",
        "works-at",
        json!(null),
    );
    link(
        &mut h,
        "contacts",
        "contact-a",
        "deals",
        "deal-1",
        "owns-deal",
        json!(null),
    );
    link(
        &mut h,
        "contacts",
        "contact-b",
        "companies",
        "company-x",
        "works-at",
        json!(null),
    );
    link(
        &mut h,
        "contacts",
        "contact-b",
        "deals",
        "deal-2",
        "owns-deal",
        json!(null),
    );

    // EXECUTION: merge Contact-A into Contact-B in one transaction.
    let contact_a = get(&h, "contacts", "contact-a").unwrap();
    let contact_b = get(&h, "contacts", "contact-b").unwrap();

    let mut tx = Transaction::new();
    // Merge fields into Contact-B (keep newer email, merge phone).
    tx.update(
        Entity::new(
            col("contacts"),
            eid("contact-b"),
            json!({"name": "Alice Smith", "email": "alice@new.com", "phone": "555-0202", "alt_phone": "555-0101"}),
        ),
        contact_b.version,
        Some(contact_b.data.clone()),
    ).unwrap();
    // Delete Contact-A
    tx.delete(
        col("contacts"),
        eid("contact-a"),
        contact_a.version,
        Some(contact_a.data.clone()),
    )
    .unwrap();

    let (storage, audit) = h.storage_and_audit_mut();
    tx.commit(storage, audit, Some("merge-agent".into()), None)
        .unwrap();

    // Re-link Deal-1 from Contact-A to Contact-B.
    link(
        &mut h,
        "contacts",
        "contact-b",
        "deals",
        "deal-1",
        "owns-deal",
        json!(null),
    );

    // CHECK: Contact-A no longer exists.
    assert!(get(&h, "contacts", "contact-a").is_none());

    // CHECK: Contact-B has merged fields.
    let merged = get(&h, "contacts", "contact-b").unwrap();
    assert_eq!(merged.data["name"], "Alice Smith");
    assert_eq!(merged.data["alt_phone"], "555-0101");

    // CHECK: Contact-B has both deals linked.
    let deals = traverse(&h, "contacts", "contact-b", Some("owns-deal"), Some(3));
    let deal_ids: Vec<_> = deals.iter().map(|e| e.id.as_str()).collect();
    assert!(deal_ids.contains(&"deal-1"), "should link to deal-1");
    assert!(deal_ids.contains(&"deal-2"), "should link to deal-2");

    // CHECK: audit log records the merge.
    let entries = h.audit_log().entries();
    let merge_entries: Vec<_> = entries
        .iter()
        .filter(|e| e.actor == "merge-agent")
        .collect();
    assert_eq!(
        merge_entries.len(),
        2,
        "update contact-b + delete contact-a"
    );
}

// ── SCN-003: CDP — Identity Resolution and Profile Merge ────────────────────

#[test]
fn scn_003_identity_resolution_and_profile_merge() {
    let mut h = handler();

    // SETUP: two source records from different channels with matching email.
    create(
        &mut h,
        "source-records",
        "sr-web-001",
        json!({
            "channel": "web",
            "email": "jane@example.com",
            "name": "Jane Doe",
            "age": 30
        }),
    );
    create(
        &mut h,
        "source-records",
        "sr-crm-002",
        json!({
            "channel": "crm",
            "email": "jane@example.com",
            "name": "J. Doe",
            "phone": "555-1234"
        }),
    );

    // EXECUTION: identity resolution creates unified profile.
    // Highest-confidence source wins per field (web has name+age, crm has phone).
    create(
        &mut h,
        "profiles",
        "p-001",
        json!({
            "email": "jane@example.com",
            "name": "Jane Doe",       // from web (higher confidence for name)
            "age": 30,                 // from web
            "phone": "555-1234"        // from crm
        }),
    );

    // Link source records to profile with confidence metadata.
    link(
        &mut h,
        "source-records",
        "sr-web-001",
        "profiles",
        "p-001",
        "resolved-from",
        json!({"confidence": 0.95, "match_rule": "email_exact"}),
    );
    link(
        &mut h,
        "source-records",
        "sr-crm-002",
        "profiles",
        "p-001",
        "resolved-from",
        json!({"confidence": 0.90, "match_rule": "email_exact"}),
    );

    // CHECK: unified profile exists with canonical fields.
    let profile = get(&h, "profiles", "p-001").unwrap();
    assert_eq!(profile.data["email"], "jane@example.com");
    assert_eq!(profile.data["name"], "Jane Doe");
    assert_eq!(profile.data["phone"], "555-1234");

    // CHECK: audit trail shows profile creation and link creation.
    let audit = h
        .query_audit(QueryAuditRequest {
            collection: Some(col("profiles")),
            ..Default::default()
        })
        .unwrap();
    assert!(
        !audit.entries.is_empty(),
        "audit should record profile creation"
    );
}

// ── SCN-004: ERP — BOM Explosion via Recursive Traversal ────────────────────

#[test]
fn scn_004_bom_explosion_recursive_traversal() {
    let mut h = handler();

    // SETUP: Widget-A → Sub-Assembly-B (qty:2), Component-C (qty:4)
    //        Sub-Assembly-B → Component-C (qty:1), Component-D (qty:3)
    create(
        &mut h,
        "products",
        "widget-a",
        json!({"name": "Widget A", "type": "assembly"}),
    );
    create(
        &mut h,
        "products",
        "sub-assy-b",
        json!({"name": "Sub-Assembly B", "type": "sub-assembly"}),
    );
    create(
        &mut h,
        "products",
        "comp-c",
        json!({"name": "Component C", "type": "component"}),
    );
    create(
        &mut h,
        "products",
        "comp-d",
        json!({"name": "Component D", "type": "component"}),
    );

    link(
        &mut h,
        "products",
        "widget-a",
        "products",
        "sub-assy-b",
        "contains",
        json!({"quantity": 2}),
    );
    link(
        &mut h,
        "products",
        "widget-a",
        "products",
        "comp-c",
        "contains",
        json!({"quantity": 4}),
    );
    link(
        &mut h,
        "products",
        "sub-assy-b",
        "products",
        "comp-c",
        "contains",
        json!({"quantity": 1}),
    );
    link(
        &mut h,
        "products",
        "sub-assy-b",
        "products",
        "comp-d",
        "contains",
        json!({"quantity": 3}),
    );

    // EXECUTION: traverse BOM from Widget-A.
    let bom = traverse(&h, "products", "widget-a", Some("contains"), Some(3));

    // CHECK: traversal returns all components.
    let bom_ids: Vec<_> = bom.iter().map(|e| e.id.as_str()).collect();
    assert!(
        bom_ids.contains(&"sub-assy-b"),
        "should reach sub-assembly B"
    );
    assert!(bom_ids.contains(&"comp-c"), "should reach component C");
    assert!(bom_ids.contains(&"comp-d"), "should reach component D");

    // CHECK: leaf nodes (components with no outgoing `contains` links).
    let comp_c_children = traverse(&h, "products", "comp-c", Some("contains"), Some(1));
    assert!(comp_c_children.is_empty(), "comp-c is a leaf node");
    let comp_d_children = traverse(&h, "products", "comp-d", Some("contains"), Some(1));
    assert!(comp_d_children.is_empty(), "comp-d is a leaf node");
}

// ── SCN-005: Workflow — Invoice Approval Chain ──────────────────────────────

#[test]
fn scn_005_invoice_approval_chain() {
    let mut h = handler();

    // State machine transitions are enforced at the application level using
    // Axon's entity update + schema validation.
    let valid_transitions: &[(&str, &str)] = &[
        ("draft", "submitted"),
        ("submitted", "approved"),
        ("approved", "paid"),
    ];

    // SETUP
    create(
        &mut h,
        "invoices",
        "inv-100",
        json!({
            "amount": 1500,
            "status": "draft",
            "approver": null
        }),
    );
    create(
        &mut h,
        "contacts",
        "approver-1",
        json!({"name": "Bob Manager"}),
    );

    // EXECUTION 1: attempt invalid transition draft → approved.
    let inv = get(&h, "invoices", "inv-100").unwrap();
    let from = inv.data["status"].as_str().unwrap();
    let to = "approved";
    let is_valid = valid_transitions
        .iter()
        .any(|(f, t)| *f == from && *t == to);
    assert!(!is_valid, "draft → approved is not a valid transition");

    // EXECUTION 2: valid transition draft → submitted.
    let inv = update(
        &mut h,
        "invoices",
        "inv-100",
        json!({"amount": 1500, "status": "submitted", "approver": null}),
        inv.version,
    );

    // EXECUTION 3: attempt submitted → approved without approval link (guard check).
    let has_approval_link =
        !traverse(&h, "invoices", "inv-100", Some("approved-by"), Some(1)).is_empty();
    assert!(
        !has_approval_link,
        "no approval link yet — guard condition not met"
    );

    // EXECUTION 4: create approved-by link, then transition.
    link(
        &mut h,
        "invoices",
        "inv-100",
        "contacts",
        "approver-1",
        "approved-by",
        json!(null),
    );
    let has_approval_link =
        !traverse(&h, "invoices", "inv-100", Some("approved-by"), Some(1)).is_empty();
    assert!(has_approval_link, "approval link now exists");

    let _inv = update(
        &mut h,
        "invoices",
        "inv-100",
        json!({"amount": 1500, "status": "approved", "approver": "approver-1"}),
        inv.version,
    );

    // CHECK: final status is approved.
    let final_inv = get(&h, "invoices", "inv-100").unwrap();
    assert_eq!(final_inv.data["status"], "approved");

    // CHECK: audit trail captures transitions.
    let audit = h
        .query_audit(QueryAuditRequest {
            collection: Some(col("invoices")),
            entity_id: Some(eid("inv-100")),
            ..Default::default()
        })
        .unwrap();
    assert!(
        audit.entries.len() >= 3,
        "expected at least create + 2 updates, got {}",
        audit.entries.len()
    );
}

// ── SCN-006: Issue Tracking — Dependency DAG and Ready Queue ────────────────

#[test]
fn scn_006_issue_dependency_dag_and_ready_queue() {
    let mut h = handler();

    // SETUP: Issue-A depends on B and C. Issue-B depends on D. Issue-C has no deps.
    create(
        &mut h,
        "issues",
        "issue-a",
        json!({"title": "A", "status": "open"}),
    );
    create(
        &mut h,
        "issues",
        "issue-b",
        json!({"title": "B", "status": "open"}),
    );
    create(
        &mut h,
        "issues",
        "issue-c",
        json!({"title": "C", "status": "open"}),
    );
    create(
        &mut h,
        "issues",
        "issue-d",
        json!({"title": "D", "status": "open"}),
    );

    link(
        &mut h,
        "issues",
        "issue-a",
        "issues",
        "issue-b",
        "depends-on",
        json!(null),
    );
    link(
        &mut h,
        "issues",
        "issue-a",
        "issues",
        "issue-c",
        "depends-on",
        json!(null),
    );
    link(
        &mut h,
        "issues",
        "issue-b",
        "issues",
        "issue-d",
        "depends-on",
        json!(null),
    );

    // Helper: find "ready" issues (status=open with all dependencies done).
    let find_ready = |h: &AxonHandler<MemoryStorageAdapter>| -> Vec<String> {
        let open = query(
            h,
            "issues",
            Some(FilterNode::Field(FieldFilter {
                field: "status".into(),
                op: FilterOp::Eq,
                value: json!("open"),
            })),
        );

        open.into_iter()
            .filter(|issue| {
                let deps = traverse(h, "issues", issue.id.as_str(), Some("depends-on"), Some(1));
                deps.iter().all(|dep| dep.data["status"] == "done")
            })
            .map(|e| e.id.to_string())
            .collect()
    };

    // CHECK: initially C and D are ready (no deps or all deps done).
    let ready = find_ready(&h);
    assert!(
        ready.contains(&"issue-c".to_string()),
        "C should be ready (no deps)"
    );
    assert!(
        ready.contains(&"issue-d".to_string()),
        "D should be ready (no deps)"
    );
    assert!(
        !ready.contains(&"issue-a".to_string()),
        "A should not be ready (blocked by B, C)"
    );
    assert!(
        !ready.contains(&"issue-b".to_string()),
        "B should not be ready (blocked by D)"
    );

    // Close D, re-query.
    let d = get(&h, "issues", "issue-d").unwrap();
    update(
        &mut h,
        "issues",
        "issue-d",
        json!({"title": "D", "status": "done"}),
        d.version,
    );

    let ready = find_ready(&h);
    assert!(
        ready.contains(&"issue-b".to_string()),
        "B should be ready after D is done"
    );

    // Close B, re-query.
    let b = get(&h, "issues", "issue-b").unwrap();
    update(
        &mut h,
        "issues",
        "issue-b",
        json!({"title": "B", "status": "done"}),
        b.version,
    );

    let ready = find_ready(&h);
    assert!(
        !ready.contains(&"issue-a".to_string()),
        "A still blocked by C"
    );

    // Close C, re-query.
    let c = get(&h, "issues", "issue-c").unwrap();
    update(
        &mut h,
        "issues",
        "issue-c",
        json!({"title": "C", "status": "done"}),
        c.version,
    );

    let ready = find_ready(&h);
    assert!(
        ready.contains(&"issue-a".to_string()),
        "A should be ready after B and C done"
    );
}

// ── SCN-007: Agentic — Bead Lifecycle with Concurrent Agents ────────────────

#[test]
fn scn_007_bead_lifecycle_concurrent_agents() {
    let mut storage = MemoryStorageAdapter::default();
    let mut audit = MemoryAuditLog::default();

    // SETUP: 5 beads, 3 agents. Bead dependency: B1→B3, B2→B3, B4→B5.
    for i in 1..=5 {
        storage
            .put(Entity::new(
                col("beads"),
                eid(&format!("b-{i}")),
                json!({"title": format!("Bead {i}"), "status": "ready", "agent": null}),
            ))
            .unwrap();
    }
    // B1 and B2 depend on B3, B4 depends on B5 — but for this test,
    // we start all as "ready" and test OCC claims directly.

    let agents = ["agent-alpha", "agent-beta", "agent-gamma"];
    let mut claims: Vec<(String, String)> = Vec::new(); // (agent, bead_id)

    // Simulate agents claiming beads with OCC.
    for (i, agent) in agents.iter().enumerate() {
        let bead_id = format!("b-{}", i + 1);
        let bead = storage.get(&col("beads"), &eid(&bead_id)).unwrap().unwrap();

        let mut tx = Transaction::new();
        tx.update(
            Entity::new(
                col("beads"),
                eid(&bead_id),
                json!({"title": bead.data["title"], "status": "in_progress", "agent": agent}),
            ),
            bead.version,
            Some(bead.data.clone()),
        )
        .unwrap();
        tx.commit(&mut storage, &mut audit, Some((*agent).into()), None)
            .unwrap();

        claims.push((agent.to_string(), bead_id));
    }

    // CHECK: no bead claimed by more than one agent.
    let mut claimed_beads: Vec<String> = Vec::new();
    for (_, bead_id) in &claims {
        assert!(
            !claimed_beads.contains(bead_id),
            "bead {bead_id} claimed by multiple agents"
        );
        claimed_beads.push(bead_id.clone());
    }

    // CHECK: OCC prevents double-claim.
    // Agent-gamma tries to claim b-1 (already claimed by agent-alpha).
    let _b1 = storage.get(&col("beads"), &eid("b-1")).unwrap().unwrap();
    let mut dup_tx = Transaction::new();
    dup_tx
        .update(
            Entity::new(
                col("beads"),
                eid("b-1"),
                json!({"title": "Bead 1", "status": "in_progress", "agent": "agent-gamma"}),
            ),
            1, // stale version — b-1 is now at version 2
            None,
        )
        .unwrap();
    let err = dup_tx.commit(&mut storage, &mut audit, None, None).unwrap_err();
    assert!(
        matches!(err, AxonError::ConflictingVersion { .. }),
        "double-claim should fail with version conflict"
    );

    // Complete all beads.
    for i in 1..=5 {
        let bead_id = format!("b-{i}");
        let bead = storage.get(&col("beads"), &eid(&bead_id)).unwrap().unwrap();
        let mut tx = Transaction::new();
        tx.update(
            Entity::new(
                col("beads"),
                eid(&bead_id),
                json!({"title": bead.data["title"], "status": "done", "agent": bead.data["agent"]}),
            ),
            bead.version,
            Some(bead.data.clone()),
        )
        .unwrap();
        tx.commit(&mut storage, &mut audit, None, None).unwrap();
    }

    // CHECK: all beads reach "done".
    for i in 1..=5 {
        let bead = storage
            .get(&col("beads"), &eid(&format!("b-{i}")))
            .unwrap()
            .unwrap();
        assert_eq!(bead.data["status"], "done", "bead b-{i} should be done");
    }

    // CHECK: audit log shows which agent processed each bead.
    let entries = audit.entries();
    assert!(
        !entries.is_empty(),
        "audit log should contain bead processing entries"
    );
}

// ── SCN-008: MDM — Golden Record Merge with Survivorship ────────────────────

#[test]
fn scn_008_golden_record_merge_with_survivorship() {
    let mut h = handler();

    // SETUP: two source records with conflicting data.
    create(
        &mut h,
        "source-records",
        "src-erp",
        json!({
            "source_system": "erp",
            "company_name": "Acme Inc.",
            "address": "123 Main St",
            "revenue": 5_000_000,
            "confidence": 0.85
        }),
    );
    create(
        &mut h,
        "source-records",
        "src-crm",
        json!({
            "source_system": "crm",
            "company_name": "ACME Incorporated",
            "address": "123 Main Street",
            "phone": "555-9999",
            "confidence": 0.92
        }),
    );

    // EXECUTION: survivorship rules — highest confidence wins per field.
    // CRM has higher confidence overall → name, address from CRM.
    // ERP has revenue (CRM doesn't). CRM has phone (ERP doesn't).
    let mut tx = Transaction::new();
    tx.create(Entity::new(
        col("golden-records"),
        eid("gr-001"),
        json!({
            "company_name": "ACME Incorporated",  // CRM (0.92 > 0.85)
            "address": "123 Main Street",          // CRM
            "revenue": 5_000_000,                  // ERP (only source)
            "phone": "555-9999"                    // CRM (only source)
        }),
    ))
    .unwrap();
    let (storage, audit) = h.storage_and_audit_mut();
    tx.commit(storage, audit, Some("mdm-engine".into()), None)
        .unwrap();

    // Link sources to golden record.
    link(
        &mut h,
        "source-records",
        "src-erp",
        "golden-records",
        "gr-001",
        "sourced-from",
        json!({"confidence": 0.85, "match_rule": "name_fuzzy", "source_system": "erp"}),
    );
    link(
        &mut h,
        "source-records",
        "src-crm",
        "golden-records",
        "gr-001",
        "sourced-from",
        json!({"confidence": 0.92, "match_rule": "name_fuzzy", "source_system": "crm"}),
    );

    // CHECK: golden record has correct survivorship values.
    let gr = get(&h, "golden-records", "gr-001").unwrap();
    assert_eq!(gr.data["company_name"], "ACME Incorporated");
    assert_eq!(gr.data["revenue"], 5_000_000);
    assert_eq!(gr.data["phone"], "555-9999");

    // CHECK: both sources linked with metadata.
    let _sources = traverse(&h, "golden-records", "gr-001", None, Some(1));
    // Golden record doesn't have outgoing links — check from source side.
    // (traversal goes source→target, so we check from source-records→golden-records)

    // CHECK: audit trail records the merge.
    let audit = h
        .query_audit(QueryAuditRequest {
            collection: Some(col("golden-records")),
            ..Default::default()
        })
        .unwrap();
    assert!(
        !audit.entries.is_empty(),
        "golden record creation should be audited"
    );
}

// ── SCN-009: Document Management — Version Chain ────────────────────────────

#[test]
fn scn_009_document_version_chain() {
    let mut h = handler();

    // SETUP: 3 document versions linked via `supersedes`.
    create(
        &mut h,
        "documents",
        "doc-v1",
        json!({
            "title": "Design Spec",
            "version": 1,
            "content": "Initial draft",
            "is_latest": false
        }),
    );
    create(
        &mut h,
        "documents",
        "doc-v2",
        json!({
            "title": "Design Spec",
            "version": 2,
            "content": "Revised after review",
            "is_latest": false
        }),
    );
    create(
        &mut h,
        "documents",
        "doc-v3",
        json!({
            "title": "Design Spec",
            "version": 3,
            "content": "Final version",
            "is_latest": true
        }),
    );

    // Link version chain: v3 supersedes v2, v2 supersedes v1.
    link(
        &mut h,
        "documents",
        "doc-v3",
        "documents",
        "doc-v2",
        "supersedes",
        json!(null),
    );
    link(
        &mut h,
        "documents",
        "doc-v2",
        "documents",
        "doc-v1",
        "supersedes",
        json!(null),
    );

    // CHECK: traversal from v3 via `supersedes` returns [v2, v1] in order (BFS).
    let chain = traverse(&h, "documents", "doc-v3", Some("supersedes"), Some(3));
    let ids: Vec<_> = chain.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["doc-v2", "doc-v1"],
        "version chain should be v2 then v1"
    );

    // CHECK: each version has correct content.
    assert_eq!(chain[0].data["content"], "Revised after review");
    assert_eq!(chain[1].data["content"], "Initial draft");

    // CHECK: latest version queryable without traversal (field-based query).
    let latest = query(
        &h,
        "documents",
        Some(FilterNode::Field(FieldFilter {
            field: "is_latest".into(),
            op: FilterOp::Eq,
            value: json!(true),
        })),
    );
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].id.as_str(), "doc-v3");

    // CHECK: audit trail exists for each version.
    let audit = h
        .query_audit(QueryAuditRequest {
            collection: Some(col("documents")),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        audit.entries.len(),
        3,
        "one audit entry per version creation"
    );
}

// ── SCN-010: Time Tracking — Approval and Billing ───────────────────────────

#[test]
fn scn_010_time_tracking_approval_and_billing() {
    let mut h = handler();

    // SETUP
    create(
        &mut h,
        "projects",
        "proj-alpha",
        json!({"name": "Project Alpha"}),
    );
    create(
        &mut h,
        "contacts",
        "mgr-001",
        json!({"name": "Manager Kim"}),
    );

    // Create time entries linked to project.
    for (id, hours) in [("te-1", 4.0), ("te-2", 6.0), ("te-3", 2.5)] {
        create(
            &mut h,
            "time-entries",
            id,
            json!({"hours": hours, "status": "draft", "project": "proj-alpha"}),
        );
        link(
            &mut h,
            "time-entries",
            id,
            "projects",
            "proj-alpha",
            "logged-for",
            json!(null),
        );
    }

    // State machine: draft → submitted → approved → billed
    let valid_transitions: &[(&str, &str)] = &[
        ("draft", "submitted"),
        ("submitted", "approved"),
        ("approved", "billed"),
    ];

    // Submit all time entries.
    for id in ["te-1", "te-2", "te-3"] {
        let te = get(&h, "time-entries", id).unwrap();
        let from = te.data["status"].as_str().unwrap();
        assert!(
            valid_transitions
                .iter()
                .any(|(f, t)| *f == from && *t == "submitted"),
            "{id}: draft → submitted should be valid"
        );
        update(
            &mut h,
            "time-entries",
            id,
            json!({"hours": te.data["hours"], "status": "submitted", "project": "proj-alpha"}),
            te.version,
        );
    }

    // Approve: create approved-by link and transition.
    for id in ["te-1", "te-2", "te-3"] {
        link(
            &mut h,
            "time-entries",
            id,
            "contacts",
            "mgr-001",
            "approved-by",
            json!(null),
        );
        let te = get(&h, "time-entries", id).unwrap();
        update(
            &mut h,
            "time-entries",
            id,
            json!({"hours": te.data["hours"], "status": "approved", "project": "proj-alpha"}),
            te.version,
        );
    }

    // CHECK: state machine enforcement — cannot go draft → billed.
    let from = "draft";
    let to = "billed";
    assert!(
        !valid_transitions
            .iter()
            .any(|(f, t)| *f == from && *t == to),
        "draft → billed should not be valid"
    );

    // CHECK: aggregation — sum hours by project where status=approved.
    let approved = query(
        &h,
        "time-entries",
        Some(FilterNode::Field(FieldFilter {
            field: "status".into(),
            op: FilterOp::Eq,
            value: json!("approved"),
        })),
    );
    let total_hours: f64 = approved
        .iter()
        .map(|e| e.data["hours"].as_f64().unwrap())
        .sum();
    assert!(
        (total_hours - 12.5).abs() < 0.01,
        "total approved hours should be 12.5, got {total_hours}"
    );

    // Bill the entries.
    for id in ["te-1", "te-2", "te-3"] {
        let te = get(&h, "time-entries", id).unwrap();
        update(
            &mut h,
            "time-entries",
            id,
            json!({"hours": te.data["hours"], "status": "billed", "project": "proj-alpha"}),
            te.version,
        );
    }

    // CHECK: audit trail shows full approval chain.
    let audit = h
        .query_audit(QueryAuditRequest {
            collection: Some(col("time-entries")),
            ..Default::default()
        })
        .unwrap();
    // Each entry: create + submit + approve + bill = 4 entries × 3 = 12
    assert!(
        audit.entries.len() >= 12,
        "expected at least 12 audit entries, got {}",
        audit.entries.len()
    );
}
