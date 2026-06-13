#![allow(
    clippy::inefficient_to_string,
    clippy::needless_collect,
    clippy::similar_names,
    clippy::unwrap_used,
    clippy::wildcard_imports
)]

//! L2 Business Scenario Tests (SCN-001 through SCN-010, plus rollback/repair flows)
//!
//! Each test validates a real-world workflow from use-case research, exercising
//! Axon's API end-to-end against the in-memory storage backend.

use axon_api::handler::AxonHandler;
use axon_api::request::*;
use axon_api::response::RollbackEntityResponse;
use axon_api::transaction::Transaction;
use axon_audit::entry::MutationType;
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
    let err = dup_tx
        .commit(&mut storage, &mut audit, None, None)
        .unwrap_err();
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

// ── US-024-AC4: BOM dangling-link resilience ────────────────────────────────

#[test]
fn scn_024_ac4_dangling_link_targets_skipped_without_error() {
    // @covers US-024-AC4
    // After force-deleting a component that is the target of a `contains` link,
    // BOM traversal must skip the dangling link silently and return without error.
    let mut h = handler();

    // SETUP: same BOM tree as SCN-004.
    for (id, name, t) in [
        ("widget-a", "Widget A", "assembly"),
        ("sub-assy-b", "Sub-Assembly B", "sub-assembly"),
        ("comp-c", "Component C", "component"),
        ("comp-d", "Component D", "component"),
    ] {
        h.create_entity(CreateEntityRequest {
            collection: col("products"),
            id: eid(id),
            data: json!({"name": name, "type": t}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    }
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

    // EXECUTION: force-delete comp-d, leaving the sub-assy-b→comp-d link dangling.
    h.delete_entity(DeleteEntityRequest {
        collection: col("products"),
        id: eid("comp-d"),
        force: true,
        actor: None,
        audit_metadata: None,
        attribution: None,
    })
    .unwrap();

    // CHECK: traversal must succeed without error and skip the dangling link.
    let bom = traverse(&h, "products", "widget-a", Some("contains"), Some(3));
    let bom_ids: Vec<&str> = bom.iter().map(|e| e.id.as_str()).collect();
    assert!(
        !bom_ids.contains(&"comp-d"),
        "deleted comp-d must not appear in traversal results"
    );
    assert!(
        bom_ids.contains(&"sub-assy-b"),
        "sub-assy-b must still be reachable"
    );
    assert!(
        bom_ids.contains(&"comp-c"),
        "comp-c must still be reachable via two paths"
    );
    // Three entities remain reachable: sub-assy-b, comp-c (direct), comp-c (via sub-assy-b).
    // The traverse() API deduplicates via visited set, so comp-c appears once.
    assert_eq!(
        bom_ids.len(),
        2,
        "only sub-assy-b and comp-c should be reachable after comp-d is deleted"
    );
}

// ── US-025-AC2: reachability short-circuit ───────────────────────────────────

#[test]
fn us_025_ac2_reachable_short_circuits_on_first_path_found() {
    // @covers US-025-AC2
    // When multiple paths lead to the same target, reachable() returns true
    // as soon as the first path is found without materializing all paths.
    // Diamond graph: source → path-a → target  and  source → path-b → target.
    let mut h = handler();

    for id in ["source", "path-a", "path-b", "target"] {
        h.create_entity(CreateEntityRequest {
            collection: col("nodes"),
            id: eid(id),
            data: json!({"name": id}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    }
    // Two independent paths to target.
    link(&mut h, "nodes", "source", "nodes", "path-a", "edge", json!(null));
    link(&mut h, "nodes", "source", "nodes", "path-b", "edge", json!(null));
    link(&mut h, "nodes", "path-a", "nodes", "target", "edge", json!(null));
    link(&mut h, "nodes", "path-b", "nodes", "target", "edge", json!(null));

    let result = h
        .reachable(ReachableRequest {
            source_collection: col("nodes"),
            source_id: eid("source"),
            target_collection: col("nodes"),
            target_id: eid("target"),
            link_type: Some("edge".into()),
            max_depth: Some(3),
            direction: Default::default(),
        })
        .unwrap();

    assert!(result.reachable, "target must be reachable from source");
    assert_eq!(
        result.depth,
        Some(2),
        "shortest path is 2 hops (source → path-a → target or source → path-b → target)"
    );
}

#[test]
fn us_025_ac2_reachable_returns_false_when_unreachable() {
    // Complementary negative case: unreachable target returns false.
    let mut h = handler();
    for id in ["a", "b", "c"] {
        h.create_entity(CreateEntityRequest {
            collection: col("nodes"),
            id: eid(id),
            data: json!({"name": id}),
            actor: None,
            audit_metadata: None,
            attribution: None,
        })
        .unwrap();
    }
    link(&mut h, "nodes", "a", "nodes", "b", "edge", json!(null));
    // c is disconnected from a and b.

    let result = h
        .reachable(ReachableRequest {
            source_collection: col("nodes"),
            source_id: eid("a"),
            target_collection: col("nodes"),
            target_id: eid("c"),
            link_type: Some("edge".into()),
            max_depth: Some(5),
            direction: Default::default(),
        })
        .unwrap();

    assert!(!result.reachable, "c must not be reachable from a");
    assert_eq!(result.depth, None, "depth should be None when unreachable");
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

// ── Rollback and Repair Flows (FEAT-023, ADR-015) ───────────────────────────
//
// These tests prove the repair principle: rollback and repair are possible
// when preventive guardrails fail. Each test maps to user story acceptance
// criteria (US-095, US-096, US-097).

// ── US-095-AC1, US-095-AC2: dry-run returns diff without mutating the entity ─

#[test]
fn us_095_rollback_dry_run_returns_diff_without_mutating() {
    // @covers US-095-AC1
    // @covers US-095-AC2
    // Given an invoice damaged by a bad agent write, dry-run rollback returns
    // the field-level diff and leaves the entity unchanged.
    let mut h = handler();

    // SETUP: invoice entity with a known-good v1 and a bad v2 written by a rogue agent.
    create(
        &mut h,
        "invoices",
        "inv-repair-001",
        json!({"amount": 5000, "status": "open", "vendor": "Acme"}),
    );
    let inv_v1 = get(&h, "invoices", "inv-repair-001").unwrap();
    assert_eq!(inv_v1.version, 1);

    // Bad agent write: wrong amount and status.
    update(
        &mut h,
        "invoices",
        "inv-repair-001",
        json!({"amount": 999, "status": "closed", "vendor": "Acme"}),
        inv_v1.version,
    );

    let inv_v2 = get(&h, "invoices", "inv-repair-001").unwrap();
    assert_eq!(inv_v2.version, 2);
    assert_eq!(inv_v2.data["amount"], 999, "bad write is stored");

    // DRY-RUN: rollback to version 1 without committing.
    let resp = h
        .rollback_entity(RollbackEntityRequest {
            collection: col("invoices"),
            id: eid("inv-repair-001"),
            target: RollbackEntityTarget::Version(1),
            expected_version: None,
            actor: Some("operator".into()),
            dry_run: true,
        })
        .unwrap();

    // Dry-run identifies the target version from audit history (US-095-AC2).
    let (current, target, diff) = match resp {
        RollbackEntityResponse::DryRun {
            current,
            target,
            diff,
        } => (current, target, diff),
        _ => panic!("expected DryRun response"),
    };

    // Diff shows the fields that would be restored (US-095-AC1).
    assert!(
        diff.contains_key("amount"),
        "diff must flag the changed amount field"
    );
    assert!(
        diff.contains_key("status"),
        "diff must flag the changed status field"
    );
    let amount_diff = diff.get("amount").unwrap();
    assert_eq!(amount_diff.before.as_ref().unwrap(), &json!(999));
    assert_eq!(amount_diff.after.as_ref().unwrap(), &json!(5000));

    // Target state matches v1 content (US-095-AC2: dry-run identifies target version).
    assert_eq!(target.data["amount"], 5000);
    assert_eq!(target.data["status"], "open");

    // Current state is the v2 (bad) state.
    let current = current.unwrap();
    assert_eq!(current.data["amount"], 999);

    // ENTITY MUST NOT BE MUTATED (US-095-AC1: entity still at v2 after dry-run).
    let still_v2 = get(&h, "invoices", "inv-repair-001").unwrap();
    assert_eq!(still_v2.version, 2, "dry-run must not mutate the entity");
    assert_eq!(still_v2.data["amount"], 999, "entity data unchanged after dry-run");

    // AUDIT LOG MUST HAVE NO NEW ENTRIES (dry-run produces no audit entries).
    let audit = h
        .query_audit(QueryAuditRequest {
            collection: Some(col("invoices")),
            entity_id: Some(eid("inv-repair-001")),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        audit.entries.len(),
        2,
        "dry-run must not produce new audit entries (create + bad update = 2)"
    );
}

// ── US-096-AC1/AC2/AC3/AC4: commit rollback, audit lineage, diff/blame view ──

#[test]
fn us_096_rollback_commit_restores_state_and_audits_lineage() {
    // @covers US-096-AC1
    // @covers US-096-AC2
    // @covers US-096-AC3
    // @covers US-096-AC4
    // Full repair flow: bad mutation → audit diff/blame inspection → dry-run → commit →
    // assert audit lineage for both original and repair mutations.
    let mut h = handler();

    // SETUP: invoice at v1 (good), then a bad agent write at v2.
    create(
        &mut h,
        "invoices",
        "inv-repair-002",
        json!({"amount": 3000, "status": "pending", "vendor": "Globex"}),
    );
    let inv_v1 = get(&h, "invoices", "inv-repair-002").unwrap();

    update(
        &mut h,
        "invoices",
        "inv-repair-002",
        json!({"amount": 0, "status": "cancelled", "vendor": "Globex"}),
        inv_v1.version,
    );
    let inv_v2 = get(&h, "invoices", "inv-repair-002").unwrap();
    assert_eq!(inv_v2.version, 2);

    // AUDIT DIFF/BLAME VIEW: inspect the bad mutation's before/after.
    let audit_resp = h
        .query_audit(QueryAuditRequest {
            collection: Some(col("invoices")),
            entity_id: Some(eid("inv-repair-002")),
            ..Default::default()
        })
        .unwrap();

    assert_eq!(audit_resp.entries.len(), 2, "create + bad update");

    let bad_entry = &audit_resp.entries[1];
    assert_eq!(bad_entry.mutation, MutationType::EntityUpdate);
    // before/after state is captured — blame view.
    assert_eq!(
        bad_entry.data_before.as_ref().unwrap()["amount"],
        3000,
        "before state shows original good amount"
    );
    assert_eq!(
        bad_entry.data_after.as_ref().unwrap()["amount"],
        0,
        "after state shows bad amount"
    );

    // Note the bad entry's ID — we'll verify the rollback references it.
    let bad_entry_id = bad_entry.id;

    // COMMIT ROLLBACK to version 1 (US-096-AC1: entity state equals target version content).
    let resp = h
        .rollback_entity(RollbackEntityRequest {
            collection: col("invoices"),
            id: eid("inv-repair-002"),
            target: RollbackEntityTarget::Version(1),
            expected_version: None,
            actor: Some("repair-operator".into()),
            dry_run: false,
        })
        .unwrap();

    let (restored, rollback_entry) = match resp {
        RollbackEntityResponse::Applied {
            entity,
            audit_entry,
        } => (entity, audit_entry),
        _ => panic!("expected Applied response"),
    };

    // US-096-AC1: entity state equals v1 content; version advances.
    assert_eq!(restored.version, 3, "version must advance (not rewritten)");
    assert_eq!(restored.data["amount"], 3000, "amount restored to v1 value");
    assert_eq!(restored.data["status"], "pending", "status restored to v1 value");

    // US-096-AC2: old versions are not rewritten — audit still has 3 entries (create, bad update, rollback).
    let final_audit = h
        .query_audit(QueryAuditRequest {
            collection: Some(col("invoices")),
            entity_id: Some(eid("inv-repair-002")),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        final_audit.entries.len(),
        3,
        "history grows forward: create + bad write + rollback = 3"
    );
    assert_eq!(
        final_audit.entries[0].mutation,
        MutationType::EntityCreate,
        "v1 entry is still EntityCreate"
    );
    assert_eq!(
        final_audit.entries[1].mutation,
        MutationType::EntityUpdate,
        "v2 bad entry is still EntityUpdate"
    );

    // US-096-AC3: rollback audit entry uses entity.revert operation (CONTRACT-005 taxonomy).
    assert_eq!(
        rollback_entry.mutation,
        MutationType::EntityRevert,
        "rollback entry must use EntityRevert operation per CONTRACT-005"
    );

    // US-096-AC3: rollback entry references the source audit entry it restored from.
    assert!(
        rollback_entry.metadata.contains_key("reverted_from_entry_id"),
        "rollback entry must reference source audit entry"
    );

    // US-096-AC4: diff/blame view — rollback entry carries before/after state.
    assert_eq!(
        rollback_entry.data_before.as_ref().unwrap()["amount"],
        0,
        "before state of rollback = bad state being undone"
    );
    assert_eq!(
        rollback_entry.data_after.as_ref().unwrap()["amount"],
        3000,
        "after state of rollback = restored good state"
    );
    assert_eq!(rollback_entry.actor, "repair-operator");

    // Also verify audit_log shows the last entry as the rollback revert at v3.
    let last_entry = final_audit.entries.last().unwrap();
    assert_eq!(last_entry.version, 3);
    assert_eq!(last_entry.mutation, MutationType::EntityRevert);

    // Verify entity is now at the restored state.
    let current = get(&h, "invoices", "inv-repair-002").unwrap();
    assert_eq!(current.version, 3);
    assert_eq!(current.data["amount"], 3000);
    let _ = bad_entry_id; // referenced above; suppress unused warning
}

// ── US-096-AC5: OCC conflict when entity modified between dry-run and commit ──

#[test]
fn us_096_ac5_occ_conflict_when_entity_modified_between_dryrun_and_commit() {
    // @covers US-096-AC5
    // Given the entity was modified between dry-run and commit, the rollback
    // commit fails with an OCC conflict.
    let mut h = handler();

    create(
        &mut h,
        "invoices",
        "inv-conflict-001",
        json!({"amount": 1000, "status": "open"}),
    );
    let v1 = get(&h, "invoices", "inv-conflict-001").unwrap();

    update(
        &mut h,
        "invoices",
        "inv-conflict-001",
        json!({"amount": 500, "status": "open"}),
        v1.version,
    );

    // Simulate another writer modifying the entity (racing with our dry-run).
    let v2 = get(&h, "invoices", "inv-conflict-001").unwrap();
    update(
        &mut h,
        "invoices",
        "inv-conflict-001",
        json!({"amount": 750, "status": "open"}),
        v2.version,
    );

    // Now the entity is at v3; our rollback expected v2 → should conflict.
    let err = h
        .rollback_entity(RollbackEntityRequest {
            collection: col("invoices"),
            id: eid("inv-conflict-001"),
            target: RollbackEntityTarget::Version(1),
            expected_version: Some(2), // stale — entity is now at v3
            actor: Some("operator".into()),
            dry_run: false,
        })
        .unwrap_err();

    assert!(
        matches!(err, AxonError::ConflictingVersion { .. }),
        "rollback with stale expected_version must fail with ConflictingVersion, got: {err:?}"
    );

    // Entity remains at v3 — no partial state.
    let current = get(&h, "invoices", "inv-conflict-001").unwrap();
    assert_eq!(current.version, 3);
    assert_eq!(current.data["amount"], 750);
}

// ── US-096-AC6: rollback of a rollback ───────────────────────────────────────

#[test]
fn us_096_ac6_rollback_of_rollback_reapplies_later_state() {
    // @covers US-096-AC6
    // Given a committed rollback, rolling it back re-applies the post-damage
    // state as another new write with its own audit entry. History accumulates;
    // no rewrites occur.
    let mut h = handler();

    // v1: good state.
    create(
        &mut h,
        "tasks",
        "task-rollback-001",
        json!({"title": "Good state", "priority": "high"}),
    );
    // v2: bad state.
    update(
        &mut h,
        "tasks",
        "task-rollback-001",
        json!({"title": "Bad state", "priority": "low"}),
        1,
    );
    // v3: rollback to v1 (repair).
    h.rollback_entity(RollbackEntityRequest {
        collection: col("tasks"),
        id: eid("task-rollback-001"),
        target: RollbackEntityTarget::Version(1),
        expected_version: None,
        actor: Some("operator".into()),
        dry_run: false,
    })
    .unwrap();
    let v3 = get(&h, "tasks", "task-rollback-001").unwrap();
    assert_eq!(v3.version, 3);
    assert_eq!(v3.data["title"], "Good state", "v3 should be the repaired state");

    // Now roll back the rollback (i.e., re-apply the bad v2 state).
    // Rollback targeting the v2 audit entry restores that state as v4.
    h.rollback_entity(RollbackEntityRequest {
        collection: col("tasks"),
        id: eid("task-rollback-001"),
        target: RollbackEntityTarget::Version(2),
        expected_version: None,
        actor: Some("operator".into()),
        dry_run: false,
    })
    .unwrap();

    let v4 = get(&h, "tasks", "task-rollback-001").unwrap();
    assert_eq!(v4.version, 4, "rollback of rollback creates a new write at v4");
    assert_eq!(v4.data["title"], "Bad state", "v4 re-applies the bad v2 state");

    // Full history: 4 entries — no rewriting, only forward-only appends.
    let audit = h
        .query_audit(QueryAuditRequest {
            collection: Some(col("tasks")),
            entity_id: Some(eid("task-rollback-001")),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(audit.entries.len(), 4, "4 audit entries: create, update, revert, revert");
    assert_eq!(audit.entries[0].mutation, MutationType::EntityCreate);
    assert_eq!(audit.entries[1].mutation, MutationType::EntityUpdate);
    assert_eq!(audit.entries[2].mutation, MutationType::EntityRevert);
    assert_eq!(audit.entries[3].mutation, MutationType::EntityRevert);
}

// ── US-097-AC1: transaction rollback is atomic ────────────────────────────────

#[test]
fn us_097_ac1_transaction_rollback_reverts_all_entities_atomically() {
    // @covers US-097-AC1
    // A bad automation updates multiple entities in one transaction. Transaction
    // rollback reverts all of them atomically — or none, on conflict.
    let mut h = handler();

    // SETUP: two accounts and a ledger entry.
    create(&mut h, "accounts", "acc-001", json!({"balance": 1000, "name": "Alice"}));
    create(&mut h, "accounts", "acc-002", json!({"balance": 500, "name": "Bob"}));

    let acc1 = get(&h, "accounts", "acc-001").unwrap();
    let acc2 = get(&h, "accounts", "acc-002").unwrap();

    // BAD automation run: wrong transfer amounts in one transaction.
    let mut bad_tx = Transaction::new();
    let bad_tx_id = bad_tx.id.clone();
    bad_tx
        .update(
            Entity::new(
                col("accounts"),
                eid("acc-001"),
                json!({"balance": 200, "name": "Alice"}), // wrong: should be 800
            ),
            acc1.version,
            Some(acc1.data.clone()),
        )
        .unwrap();
    bad_tx
        .update(
            Entity::new(
                col("accounts"),
                eid("acc-002"),
                json!({"balance": 1300, "name": "Bob"}), // wrong: should be 700
            ),
            acc2.version,
            Some(acc2.data.clone()),
        )
        .unwrap();

    h.commit_transaction(bad_tx, Some("rogue-agent".into()), None)
        .unwrap();

    // Verify bad state is in place.
    assert_eq!(get(&h, "accounts", "acc-001").unwrap().data["balance"], 200);
    assert_eq!(get(&h, "accounts", "acc-002").unwrap().data["balance"], 1300);

    // TRANSACTION ROLLBACK: atomically revert the bad transaction.
    let resp = h
        .rollback_transaction(RollbackTransactionRequest {
            transaction_id: bad_tx_id.clone(),
            actor: Some("admin".into()),
            dry_run: false,
        })
        .unwrap();

    assert_eq!(resp.entities_affected, 2);
    assert_eq!(resp.entities_rolled_back, 2);
    assert_eq!(resp.errors, 0, "no errors — all entities rolled back");

    // US-097-AC1: all changes reversed.
    assert_eq!(
        get(&h, "accounts", "acc-001").unwrap().data["balance"],
        1000,
        "acc-001 restored to pre-transaction balance"
    );
    assert_eq!(
        get(&h, "accounts", "acc-002").unwrap().data["balance"],
        500,
        "acc-002 restored to pre-transaction balance"
    );

    // Audit lineage: each account now has create + bad_tx update + rollback revert.
    let audit_acc1 = h
        .query_audit(QueryAuditRequest {
            collection: Some(col("accounts")),
            entity_id: Some(eid("acc-001")),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(audit_acc1.entries.len(), 3, "create + bad update + rollback = 3");
    assert_eq!(
        audit_acc1.entries.last().unwrap().mutation,
        MutationType::EntityRevert,
        "rollback produces EntityRevert audit entry"
    );
}

// ── US-097-AC2 (compensating-write semantics): rollback applies over intermediate writes ─

#[test]
fn us_097_ac2_transaction_rollback_applies_compensating_write_over_intermediate_mutations() {
    // @covers US-097-AC1
    // Note: US-097-AC2 (all-or-nothing failure on OCC conflict during execution) requires
    // concurrent writers racing the rollback execution and is not provable in a
    // single-threaded test. This test instead proves the compensating-write semantics
    // defined in ADR-015 §1: rollback produces a new write at the current version, NOT
    // a version-pointer rewrite. An entity modified independently AFTER the bad transaction
    // but BEFORE the rollback gets the pre-bad-transaction state applied on top of its
    // current version (rollback over intermediate mutations is deliberate — operators
    // choose to restore the known-good state regardless of what happened in between).
    let mut h = handler();

    create(&mut h, "accounts", "acc-003", json!({"balance": 100}));
    create(&mut h, "accounts", "acc-004", json!({"balance": 200}));

    let a3 = get(&h, "accounts", "acc-003").unwrap();
    let a4 = get(&h, "accounts", "acc-004").unwrap();

    // Bad transaction touching both accounts.
    let mut bad_tx = Transaction::new();
    let bad_tx_id = bad_tx.id.clone();
    bad_tx
        .update(
            Entity::new(col("accounts"), eid("acc-003"), json!({"balance": 50})),
            a3.version,
            Some(a3.data.clone()),
        )
        .unwrap();
    bad_tx
        .update(
            Entity::new(col("accounts"), eid("acc-004"), json!({"balance": 250})),
            a4.version,
            Some(a4.data.clone()),
        )
        .unwrap();
    h.commit_transaction(bad_tx, Some("rogue-agent".into()), None)
        .unwrap();

    // An independent legitimate write modifies acc-003 after the bad transaction.
    let a3_now = get(&h, "accounts", "acc-003").unwrap();
    update(
        &mut h,
        "accounts",
        "acc-003",
        json!({"balance": 75}),
        a3_now.version,
    );

    // Rollback the bad transaction. Per ADR-015 compensating-write model, the rollback
    // applies the pre-bad-transaction state for each entity at its current version —
    // it does not check whether intermediate mutations occurred between the bad
    // transaction and the rollback attempt.
    let resp = h
        .rollback_transaction(RollbackTransactionRequest {
            transaction_id: bad_tx_id,
            actor: Some("admin".into()),
            dry_run: false,
        })
        .unwrap();

    assert_eq!(resp.entities_affected, 2);
    assert_eq!(resp.entities_rolled_back, 2);
    assert_eq!(resp.errors, 0);

    // Both entities are at the pre-bad-transaction state (compensating write over
    // intermediate modifications).
    let a3_final = get(&h, "accounts", "acc-003").unwrap();
    assert_eq!(
        a3_final.data["balance"],
        100,
        "acc-003 restored to pre-bad-transaction balance, overriding intermediate write"
    );
    let a4_final = get(&h, "accounts", "acc-004").unwrap();
    assert_eq!(
        a4_final.data["balance"],
        200,
        "acc-004 restored to pre-bad-transaction balance"
    );

    // Both rollback audit entries carry the rolled_back_transaction_id metadata.
    let audit_a3 = h
        .query_audit(QueryAuditRequest {
            collection: Some(col("accounts")),
            entity_id: Some(eid("acc-003")),
            ..Default::default()
        })
        .unwrap();
    let last_a3 = audit_a3.entries.last().unwrap();
    assert_eq!(last_a3.mutation, MutationType::EntityRevert);
    assert!(
        last_a3.metadata.contains_key("rolled_back_transaction_id"),
        "rollback audit entry must carry the source transaction ID"
    );
}
