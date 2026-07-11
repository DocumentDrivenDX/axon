#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicU64, Ordering};

use axon_api::{
    bead::{self, Bead, CreateBeadParams},
    handler::AxonHandler,
};
use axon_audit::entry::MutationType;
use axon_core::{error::AxonError, id::EntityId};
use axon_storage::{
    deprovision_postgres_database, provision_postgres_database, tenant_dsn, MemoryStorageAdapter,
    PostgresStorageAdapter, SqliteStorageAdapter, StorageAdapter,
};
use serde_json::{json, Value};

static POSTGRES_DB_COUNTER: AtomicU64 = AtomicU64::new(0);

struct PostgresFixture {
    superadmin_dsn: String,
    database_name: String,
}

impl PostgresFixture {
    fn new(vector_name: &str) -> Self {
        let superadmin_dsn = std::env::var("AXON_TEST_POSTGRES").expect(
            "AXON_TEST_POSTGRES must be set for governed_system_backend_parity; no PostgreSQL \
             backend case may be skipped",
        );
        let database_name = format!(
            "gsp_{}_{}_{}",
            vector_name,
            std::process::id(),
            POSTGRES_DB_COUNTER.fetch_add(1, Ordering::SeqCst)
        );

        let _ = deprovision_postgres_database(&superadmin_dsn, &database_name);
        provision_postgres_database(&superadmin_dsn, &database_name)
            .unwrap_or_else(|err| panic!("provision PostgreSQL fixture {database_name}: {err}"));

        Self {
            superadmin_dsn,
            database_name,
        }
    }

    fn handler(&self) -> AxonHandler<PostgresStorageAdapter> {
        let dsn = tenant_dsn(&self.superadmin_dsn, &self.database_name);
        let storage = PostgresStorageAdapter::connect(&dsn).unwrap_or_else(|err| {
            panic!("connect PostgreSQL fixture {}: {err}", self.database_name)
        });
        AxonHandler::new(storage)
    }
}

impl Drop for PostgresFixture {
    fn drop(&mut self) {
        let _ = deprovision_postgres_database(&self.superadmin_dsn, &self.database_name);
    }
}

fn run_on_all_backends(
    vector_name: &str,
    memory: fn(&mut AxonHandler<MemoryStorageAdapter>),
    sqlite: fn(&mut AxonHandler<SqliteStorageAdapter>),
    postgres: fn(&mut AxonHandler<PostgresStorageAdapter>),
) {
    let mut memory_handler = AxonHandler::new(MemoryStorageAdapter::default());
    memory(&mut memory_handler);

    let sqlite_storage = SqliteStorageAdapter::open(":memory:").expect("open SQLite fixture");
    let mut sqlite_handler = AxonHandler::new(sqlite_storage);
    sqlite(&mut sqlite_handler);

    let postgres_fixture = PostgresFixture::new(vector_name);
    let mut postgres_handler = postgres_fixture.handler();
    postgres(&mut postgres_handler);
}

fn make_bead<S: StorageAdapter>(handler: &mut AxonHandler<S>, id: &str, title: &str) -> Bead {
    bead::create_bead(
        handler,
        CreateBeadParams {
            id,
            bead_type: "task",
            title,
            description: Some("backend parity fixture"),
            priority: 1,
            assignee: None,
            tags: &[],
            acceptance: Some("same behavior on every backend"),
        },
    )
    .unwrap()
}

fn bead_ids(beads: &[Bead]) -> Vec<String> {
    let mut ids = beads.iter().map(|bead| bead.id.clone()).collect::<Vec<_>>();
    ids.sort();
    ids
}

fn entity_and_link_audit_len<S: StorageAdapter>(handler: &AxonHandler<S>) -> usize {
    handler
        .audit_log()
        .entries()
        .iter()
        .filter(|entry| {
            matches!(
                entry.mutation,
                MutationType::EntityCreate | MutationType::EntityUpdate | MutationType::LinkCreate
            )
        })
        .count()
}

fn exported_bead<'a>(exported: &'a Value, id: &str) -> &'a Value {
    exported
        .as_array()
        .expect("exported beads should be an array")
        .iter()
        .find(|item| item["id"] == id)
        .unwrap_or_else(|| panic!("missing exported bead {id}"))
}

fn bootstrap_vector<S: StorageAdapter>(handler: &mut AxonHandler<S>) {
    bead::init_beads(handler).unwrap();
    bead::init_beads(handler).unwrap();

    assert!(bead::list_beads(handler, None).unwrap().is_empty());
}

fn schema_vector<S: StorageAdapter>(handler: &mut AxonHandler<S>) {
    bead::init_beads(handler).unwrap();
    let audit_len_before = entity_and_link_audit_len(handler);

    let err = bead::import_beads(
        handler,
        &json!([{
            "id": "missing-title",
            "issue_type": "task",
            "status": "open"
        }]),
    )
    .unwrap_err();

    assert!(
        matches!(err, AxonError::SchemaValidation(_)),
        "got: {err:?}"
    );
    assert!(bead::list_beads(handler, None).unwrap().is_empty());
    assert_eq!(
        entity_and_link_audit_len(handler),
        audit_len_before,
        "invalid schema import must not append entity/link audit"
    );
}

fn entity_vector<S: StorageAdapter>(handler: &mut AxonHandler<S>) {
    make_bead(handler, "b-1", "First");
    make_bead(handler, "b-2", "Second");

    let open = bead::list_beads(handler, Some("open")).unwrap();
    assert_eq!(bead_ids(&open), vec!["b-1", "b-2"]);
    assert!(open.iter().all(|bead| bead.status == "open"));
}

fn link_vector<S: StorageAdapter>(handler: &mut AxonHandler<S>) {
    make_bead(handler, "base", "Base");
    make_bead(handler, "child", "Child");
    bead::add_dependency(handler, "child", "base").unwrap();

    assert_eq!(
        bead_ids(&bead::dependency_tree(handler, "child").unwrap()),
        vec!["base"]
    );
    assert_eq!(bead_ids(&bead::ready_queue(handler).unwrap()), vec!["base"]);

    let exported = bead::export_beads(handler).unwrap();
    assert_eq!(
        exported_bead(&exported, "child")["dependencies"],
        json!([{
            "issue_id": "child",
            "depends_on_id": "base",
            "type": "blocks"
        }])
    );
}

fn lifecycle_vector<S: StorageAdapter>(handler: &mut AxonHandler<S>) {
    make_bead(handler, "reopen", "Reopen");
    bead::transition_bead(handler, "reopen", "in_progress").unwrap();
    bead::transition_bead(handler, "reopen", "closed").unwrap();

    let ordinary_reopen = bead::transition_bead(handler, "reopen", "open").unwrap_err();
    assert!(matches!(
        ordinary_reopen,
        AxonError::InvalidTransition {
            current_state,
            target_state,
            valid_transitions,
            ..
        } if current_state == "closed" && target_state == "open" && valid_transitions.is_empty()
    ));

    let reopened = bead::reopen_bead(handler, "reopen").unwrap();
    assert_eq!(reopened.status, "open");

    make_bead(handler, "cancelled", "Cancelled");
    bead::transition_bead(handler, "cancelled", "cancelled").unwrap();
    let cancelled_reopen = bead::reopen_bead(handler, "cancelled").unwrap_err();
    assert!(matches!(
        cancelled_reopen,
        AxonError::InvalidTransition {
            current_state,
            target_state,
            ..
        } if current_state == "cancelled" && target_state == "open"
    ));
}

fn occ_vector<S: StorageAdapter>(handler: &mut AxonHandler<S>) {
    make_bead(handler, "occ", "OCC");

    let updated =
        bead::transition_bead_with_expected_version(handler, "occ", "in_progress", 1).unwrap();
    assert_eq!(updated.status, "in_progress");

    let stale =
        bead::transition_bead_with_expected_version(handler, "occ", "closed", 1).unwrap_err();
    assert!(
        matches!(stale, AxonError::ConflictingVersion { .. }),
        "got: {stale:?}"
    );
}

fn audit_vector<S: StorageAdapter>(handler: &mut AxonHandler<S>) {
    make_bead(handler, "base", "Base");
    make_bead(handler, "child", "Child");
    bead::transition_bead(handler, "base", "in_progress").unwrap();
    bead::add_dependency(handler, "child", "base").unwrap();

    let base_audit = handler
        .audit_log()
        .query_by_entity(&bead::bead_collection(), &EntityId::new("base"))
        .unwrap();
    assert!(base_audit
        .iter()
        .any(|entry| entry.mutation == MutationType::EntityCreate));
    assert!(base_audit
        .iter()
        .any(|entry| entry.mutation == MutationType::EntityUpdate));

    assert!(handler
        .audit_log()
        .entries()
        .iter()
        .any(|entry| entry.mutation == MutationType::LinkCreate));
}

#[test]
fn bootstrap_vector_runs_on_memory_sqlite_and_postgres() {
    run_on_all_backends(
        "bootstrap",
        bootstrap_vector::<MemoryStorageAdapter>,
        bootstrap_vector::<SqliteStorageAdapter>,
        bootstrap_vector::<PostgresStorageAdapter>,
    );
}

#[test]
fn schema_vector_runs_on_memory_sqlite_and_postgres() {
    run_on_all_backends(
        "schema",
        schema_vector::<MemoryStorageAdapter>,
        schema_vector::<SqliteStorageAdapter>,
        schema_vector::<PostgresStorageAdapter>,
    );
}

#[test]
fn entity_vector_runs_on_memory_sqlite_and_postgres() {
    run_on_all_backends(
        "entity",
        entity_vector::<MemoryStorageAdapter>,
        entity_vector::<SqliteStorageAdapter>,
        entity_vector::<PostgresStorageAdapter>,
    );
}

#[test]
fn link_vector_runs_on_memory_sqlite_and_postgres() {
    run_on_all_backends(
        "link",
        link_vector::<MemoryStorageAdapter>,
        link_vector::<SqliteStorageAdapter>,
        link_vector::<PostgresStorageAdapter>,
    );
}

#[test]
fn lifecycle_vector_runs_on_memory_sqlite_and_postgres() {
    run_on_all_backends(
        "lifecycle",
        lifecycle_vector::<MemoryStorageAdapter>,
        lifecycle_vector::<SqliteStorageAdapter>,
        lifecycle_vector::<PostgresStorageAdapter>,
    );
}

#[test]
fn occ_vector_runs_on_memory_sqlite_and_postgres() {
    run_on_all_backends(
        "occ",
        occ_vector::<MemoryStorageAdapter>,
        occ_vector::<SqliteStorageAdapter>,
        occ_vector::<PostgresStorageAdapter>,
    );
}

#[test]
fn audit_vector_runs_on_memory_sqlite_and_postgres() {
    run_on_all_backends(
        "audit",
        audit_vector::<MemoryStorageAdapter>,
        audit_vector::<SqliteStorageAdapter>,
        audit_vector::<PostgresStorageAdapter>,
    );
}
