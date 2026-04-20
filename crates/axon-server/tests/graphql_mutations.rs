//! GraphQL mutation integration tests (FEAT-015, salvage of eitri-apr13).
//!
//! Exercises the `/graphql` endpoint end-to-end through a real axum test
//! server so that the wiring between HTTP → `resolve_caller_identity`
//! middleware → dynamic GraphQL schema builder → `AxonHandler` is validated
//! as a single pipeline. Unit tests on the handler or the schema builder
//! alone do not cover this integration.
//!
//! Tests here intentionally use `/graphql` rather than the REST
//! `/entities/*` routes so the coverage is specific to the GraphQL transport
//! and the `_with_caller` wrappers invoked by the mutation resolvers.

#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_api::request::CreateCollectionRequest;
use axon_core::id::CollectionId;
use axon_schema::schema::{CollectionSchema, LifecycleDef};
use axon_server::gateway::build_router;
use axon_server::tenant_router::TenantRouter;
use axon_storage::adapter::StorageAdapter;
use axon_storage::SqliteStorageAdapter;
use serde_json::{json, Value};
use tokio::sync::Mutex;

// ── Fixtures ─────────────────────────────────────────────────────────────────

/// Build a test server with a `tasks` collection whose schema has no lifecycle
/// and whose entity schema allows free-form fields. The shared handler is
/// returned so tests that need to pre-seed state may do so before the server
/// accepts requests.
fn test_server() -> axum_test::TestServer {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler));
    let app = build_router(tenant_router, "memory", None);
    axum_test::TestServer::new(app)
}

/// Build a test server with a `tasks` collection that has a `status`
/// lifecycle: `draft -> submitted -> approved`.
///
/// The collection is installed directly on the handler before the test server
/// is constructed because the HTTP create-collection route does not yet
/// expose the `lifecycles` field.
async fn lifecycle_server() -> axum_test::TestServer {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));

    let mut transitions = HashMap::new();
    transitions.insert("draft".to_string(), vec!["submitted".to_string()]);
    transitions.insert("submitted".to_string(), vec!["approved".to_string()]);
    transitions.insert("approved".to_string(), vec![]);

    let lifecycle = LifecycleDef {
        field: "status".to_string(),
        initial: "draft".to_string(),
        transitions,
    };

    let mut lifecycles = HashMap::new();
    lifecycles.insert("status".to_string(), lifecycle);

    let mut schema = CollectionSchema::new(CollectionId::new("tasks"));
    schema.entity_schema = Some(json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "status": { "type": "string" }
        }
    }));
    schema.lifecycles = lifecycles;

    handler
        .lock()
        .await
        .create_collection(CreateCollectionRequest {
            name: CollectionId::new("tasks"),
            schema,
            actor: Some("test-setup".into()),
        })
        .expect("create_collection should succeed");

    let tenant_router = Arc::new(TenantRouter::single(handler));
    let app = build_router(tenant_router, "memory", None);
    axum_test::TestServer::new(app)
}

/// Register a plain `tasks` collection via the REST create-collection route.
async fn seed_tasks_collection(server: &axum_test::TestServer) {
    server
        .post("/tenants/default/databases/default/collections/tasks")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "label": { "type": "string" }
                    }
                }
            },
            "actor": "test"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
}

/// POST a GraphQL document and return the parsed JSON response body.
async fn gql(server: &axum_test::TestServer, query: &str) -> Value {
    server
        .post("/tenants/default/databases/default/graphql")
        .json(&json!({ "query": query }))
        .await
        .json::<Value>()
}

/// Same as [`gql`] but attaches an `x-axon-actor` header so the gateway's
/// identity middleware records the request as coming from the given actor.
async fn gql_as(server: &axum_test::TestServer, actor: &str, query: &str) -> Value {
    server
        .post("/tenants/default/databases/default/graphql")
        .add_header("x-axon-actor", actor)
        .json(&json!({ "query": query }))
        .await
        .json::<Value>()
}

// ── Happy-path mutations ─────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_collection_admin_create_list_refresh_and_drop() {
    let server = test_server();

    let create = gql(
        &server,
        r#"mutation {
            createCollection(input: {
                name: "projects"
                schema: {
                    description: "Project records for browser schema help text"
                    version: 1
                    entitySchema: {
                        type: "object"
                        description: "A project entity payload"
                        required: ["title"]
                        properties: {
                            title: {
                                type: "string"
                                description: "Human-readable project title"
                            }
                        }
                    }
                }
            }) {
                name
                entityCount
                schemaVersion
                schema
            }
        }"#,
    )
    .await;
    assert!(create["errors"].is_null(), "unexpected errors: {create}");
    assert_eq!(create["data"]["createCollection"]["name"], "projects");
    assert_eq!(create["data"]["createCollection"]["schemaVersion"], 1);

    let list = gql(&server, r#"{ collections { name schemaVersion schema } }"#).await;
    assert!(list["errors"].is_null(), "unexpected errors: {list}");
    let collections = list["data"]["collections"].as_array().unwrap();
    assert!(
        collections.iter().any(|collection| {
            collection["name"] == "projects"
                && collection["schema"]["version"] == 1
                && collection["schema"]["description"]
                    == "Project records for browser schema help text"
                && collection["schema"]["entity_schema"]["description"]
                    == "A project entity payload"
                && collection["schema"]["entity_schema"]["properties"]["title"]["description"]
                    == "Human-readable project title"
        }),
        "created collection should be listed with full schema descriptions: {list}"
    );

    let create_entity = gql(
        &server,
        r#"mutation { createProjects(id: "p1", input: { title: "alpha" }) { id title version } }"#,
    )
    .await;
    assert!(
        create_entity["errors"].is_null(),
        "next request should see regenerated schema: {create_entity}"
    );
    assert_eq!(create_entity["data"]["createProjects"]["title"], "alpha");

    let no_confirm = gql(
        &server,
        r#"mutation { dropCollection(input: { name: "projects", confirm: false }) { name } }"#,
    )
    .await;
    let errors = no_confirm["errors"].as_array().expect("errors array");
    assert_eq!(errors[0]["extensions"]["code"], "INVALID_ARGUMENT");

    let drop_body = gql(
        &server,
        r#"mutation { dropCollection(input: { name: "projects", confirm: true }) { name entitiesRemoved } }"#,
    )
    .await;
    assert!(
        drop_body["errors"].is_null(),
        "unexpected errors: {drop_body}"
    );
    assert_eq!(drop_body["data"]["dropCollection"]["name"], "projects");
    assert_eq!(drop_body["data"]["dropCollection"]["entitiesRemoved"], 1);

    let missing = gql(&server, r#"{ collection(name: "projects") { name } }"#).await;
    assert!(missing["errors"].is_null(), "unexpected errors: {missing}");
    assert!(missing["data"]["collection"].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_collection_template_round_trips_and_renders_markdown() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    let create_entity = gql(
        &server,
        r#"mutation {
            createTasks(id: "t1", input: { title: "GraphQL template", label: "canary" }) {
                id
                version
            }
        }"#,
    )
    .await;
    assert!(
        create_entity["errors"].is_null(),
        "unexpected errors: {create_entity}"
    );

    let missing = gql(
        &server,
        r#"{ collectionTemplate(collection: "tasks") { template } }"#,
    )
    .await;
    assert!(missing["errors"].is_null(), "unexpected errors: {missing}");
    assert!(missing["data"]["collectionTemplate"].is_null());

    let put = gql_as(
        &server,
        "template-admin",
        r##"mutation {
            putCollectionTemplate(input: {
                collection: "tasks"
                template: "# {{title}}\n\nLabel: {{label}}"
            }) {
                collection
                template
                version
                updatedAtNs
                updatedBy
                warnings
            }
        }"##,
    )
    .await;
    assert!(put["errors"].is_null(), "unexpected errors: {put}");
    assert_eq!(
        put["data"]["putCollectionTemplate"]["template"],
        "# {{title}}\n\nLabel: {{label}}"
    );
    assert_eq!(
        put["data"]["putCollectionTemplate"]["updatedBy"],
        "template-admin"
    );

    let fetched = gql(
        &server,
        r#"{ collectionTemplate(collection: "tasks") { collection template version updatedBy warnings } }"#,
    )
    .await;
    assert!(fetched["errors"].is_null(), "unexpected errors: {fetched}");
    assert_eq!(fetched["data"]["collectionTemplate"]["collection"], "tasks");
    assert_eq!(fetched["data"]["collectionTemplate"]["version"], 1);

    let rendered = gql(
        &server,
        r#"{ renderedEntity(collection: "tasks", id: "t1") { markdown entity { id data } } }"#,
    )
    .await;
    assert!(
        rendered["errors"].is_null(),
        "unexpected errors: {rendered}"
    );
    assert_eq!(
        rendered["data"]["renderedEntity"]["markdown"],
        "# GraphQL template\n\nLabel: canary"
    );
    assert_eq!(rendered["data"]["renderedEntity"]["entity"]["id"], "t1");

    let deleted = gql_as(
        &server,
        "template-admin",
        r#"mutation { deleteCollectionTemplate(collection: "tasks") { collection deleted } }"#,
    )
    .await;
    assert!(deleted["errors"].is_null(), "unexpected errors: {deleted}");
    assert_eq!(
        deleted["data"]["deleteCollectionTemplate"],
        json!({ "collection": "tasks", "deleted": true })
    );

    let missing_after_delete = gql(
        &server,
        r#"{ collectionTemplate(collection: "tasks") { template } }"#,
    )
    .await;
    assert!(
        missing_after_delete["errors"].is_null(),
        "unexpected errors: {missing_after_delete}"
    );
    assert!(missing_after_delete["data"]["collectionTemplate"].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_put_schema_compatible_and_breaking_changes() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    let compatible = gql(
        &server,
        r#"mutation {
            putSchema(input: {
                collection: "tasks"
                schema: {
                    version: 2
                    entitySchema: {
                        type: "object"
                        properties: {
                            title: { type: "string" }
                            label: { type: "string" }
                            priority: { type: "integer" }
                        }
                    }
                }
            }) {
                schema
                compatibility
                diff
                dryRun
            }
        }"#,
    )
    .await;
    assert!(
        compatible["errors"].is_null(),
        "unexpected errors: {compatible}"
    );
    assert_eq!(compatible["data"]["putSchema"]["schema"]["version"], 2);
    assert_eq!(compatible["data"]["putSchema"]["dryRun"], false);

    let breaking = gql(
        &server,
        r#"mutation {
            putSchema(input: {
                collection: "tasks"
                schema: {
                    version: 3
                    entitySchema: {
                        type: "object"
                        properties: {
                            title: { type: "integer" }
                        }
                    }
                }
            }) { schema }
        }"#,
    )
    .await;
    let errors = breaking["errors"].as_array().expect("errors array");
    assert_eq!(errors[0]["extensions"]["code"], "INVALID_OPERATION");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_create_entity_mutation_happy_path() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    let create_body = gql(
        &server,
        r#"mutation { createTasks(id: "t1", input: { title: "ship it" }) { id version title } }"#,
    )
    .await;
    assert!(
        create_body["errors"].is_null(),
        "unexpected errors: {create_body}"
    );
    assert_eq!(create_body["data"]["createTasks"]["id"], "t1");
    assert_eq!(create_body["data"]["createTasks"]["version"], 1);
    assert_eq!(create_body["data"]["createTasks"]["title"], "ship it");

    // Entity must be visible through a subsequent get query.
    let get_body = gql(&server, r#"{ tasks(id: "t1") { id version title } }"#).await;
    assert!(
        get_body["errors"].is_null(),
        "unexpected errors: {get_body}"
    );
    assert_eq!(get_body["data"]["tasks"]["id"], "t1");
    assert_eq!(get_body["data"]["tasks"]["version"], 1);
    assert_eq!(get_body["data"]["tasks"]["title"], "ship it");
}

// ── Error contracts ──────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_update_entity_version_conflict_returns_structured_error() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    gql(
        &server,
        r#"mutation { createTasks(id: "t1", input: { title: "v1" }) { id } }"#,
    )
    .await;

    // Stale expected_version (99) — must produce a VERSION_CONFLICT error
    // with the structured currentEntity extension.
    let body = gql(
        &server,
        r#"mutation { updateTasks(id: "t1", version: 99, input: { title: "stale" }) { id } }"#,
    )
    .await;

    let errors = body["errors"]
        .as_array()
        .expect("errors array for stale version");
    assert!(!errors.is_empty(), "expected errors: {body}");
    let ext = &errors[0]["extensions"];
    assert_eq!(
        ext["code"].as_str().unwrap(),
        "VERSION_CONFLICT",
        "error code should be VERSION_CONFLICT: {body}"
    );
    assert_eq!(ext["expected"], 99);
    assert_eq!(ext["actual"], 1);
    assert_eq!(
        ext["currentEntity"]["version"], 1,
        "currentEntity extension should expose the live version: {body}"
    );
    assert_eq!(ext["currentEntity"]["id"], "t1");
    assert_eq!(ext["currentEntity"]["data"]["title"], "v1");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_rollback_entity_preview_and_apply() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    gql(
        &server,
        r#"mutation { createTasks(id: "rollback-1", input: { title: "v1" }) { id } }"#,
    )
    .await;
    gql(
        &server,
        r#"mutation { updateTasks(id: "rollback-1", version: 1, input: { title: "v2" }) { id version title } }"#,
    )
    .await;

    let preview = gql(
        &server,
        r#"mutation {
            rollbackEntity(input: {
                collection: "tasks"
                id: "rollback-1"
                toVersion: 1
                dryRun: true
            }) {
                dryRun
                current { id version data }
                target { id version data }
                diff
            }
        }"#,
    )
    .await;
    assert!(preview["errors"].is_null(), "unexpected errors: {preview}");
    assert_eq!(preview["data"]["rollbackEntity"]["dryRun"], true);
    assert_eq!(
        preview["data"]["rollbackEntity"]["current"]["data"]["title"],
        "v2"
    );
    assert_eq!(
        preview["data"]["rollbackEntity"]["target"]["data"]["title"],
        "v1"
    );

    let applied = gql_as(
        &server,
        "rollback-tester",
        r#"mutation {
            rollbackEntity(input: {
                collection: "tasks"
                id: "rollback-1"
                toVersion: 1
                expectedVersion: 2
                dryRun: false
            }) {
                dryRun
                entity { id version data }
                auditEntry { actor mutation entityId }
            }
        }"#,
    )
    .await;
    assert!(applied["errors"].is_null(), "unexpected errors: {applied}");
    assert_eq!(applied["data"]["rollbackEntity"]["dryRun"], false);
    assert_eq!(
        applied["data"]["rollbackEntity"]["entity"]["data"]["title"],
        "v1"
    );
    assert_eq!(applied["data"]["rollbackEntity"]["entity"]["version"], 3);
    assert_eq!(
        applied["data"]["rollbackEntity"]["auditEntry"]["actor"],
        "rollback-tester"
    );
    assert_eq!(
        applied["data"]["rollbackEntity"]["auditEntry"]["entityId"],
        "rollback-1"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_revert_audit_entry_restores_before_state() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    gql(
        &server,
        r#"mutation { createTasks(id: "revert-1", input: { title: "v1" }) { id } }"#,
    )
    .await;
    gql(
        &server,
        r#"mutation { updateTasks(id: "revert-1", version: 1, input: { title: "v2" }) { id version title } }"#,
    )
    .await;

    let audit = gql(
        &server,
        r#"{ auditLog(collection: "tasks", entityId: "revert-1") {
            edges {
                node { id mutation version dataBefore dataAfter }
            }
        } }"#,
    )
    .await;
    assert!(audit["errors"].is_null(), "unexpected errors: {audit}");
    let update_entry_id = audit["data"]["auditLog"]["edges"]
        .as_array()
        .expect("audit edges")
        .iter()
        .find(|edge| edge["node"]["mutation"] == "entity.update")
        .and_then(|edge| edge["node"]["id"].as_str())
        .expect("update audit entry id");

    let reverted = gql_as(
        &server,
        "revert-tester",
        &format!(
            r#"mutation {{
                revertAuditEntry(auditEntryId: "{update_entry_id}") {{
                    entity {{ id version data }}
                    auditEntry {{ id actor mutation entityId metadata }}
                }}
            }}"#
        ),
    )
    .await;
    assert!(
        reverted["errors"].is_null(),
        "unexpected errors: {reverted}"
    );
    assert_eq!(
        reverted["data"]["revertAuditEntry"]["entity"]["data"]["title"],
        "v1"
    );
    assert_eq!(
        reverted["data"]["revertAuditEntry"]["auditEntry"]["actor"],
        "revert-tester"
    );
    assert_eq!(
        reverted["data"]["revertAuditEntry"]["auditEntry"]["mutation"],
        "entity.revert"
    );
    assert_eq!(
        reverted["data"]["revertAuditEntry"]["auditEntry"]["metadata"]["reverted_from_entry_id"],
        update_entry_id
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_delete_entity_mutation() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    gql(
        &server,
        r#"mutation { createTasks(id: "del-1", input: { title: "bye" }) { id } }"#,
    )
    .await;

    let del_body = gql(
        &server,
        r#"mutation { deleteTasks(id: "del-1") { deleted } }"#,
    )
    .await;
    assert!(
        del_body["errors"].is_null(),
        "unexpected errors: {del_body}"
    );
    assert_eq!(del_body["data"]["deleteTasks"]["deleted"], true);

    let get_body = gql(&server, r#"{ tasks(id: "del-1") { id } }"#).await;
    assert!(
        get_body["errors"].is_null(),
        "unexpected errors: {get_body}"
    );
    assert!(
        get_body["data"]["tasks"].is_null(),
        "deleted entity should be null: {get_body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_commit_transaction_create_and_idempotent_replay() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    let body = gql(
        &server,
        r#"mutation {
            commitTransaction(input: {
                idempotencyKey: "gql-retry-1"
                operations: [
                    { createEntity: {
                        collection: "tasks"
                        id: "tx-create-1"
                        data: { title: "batched" }
                    }}
                ]
            }) {
                transactionId
                replayHit
                results {
                    index
                    success
                    collection
                    id
                    entity { id collection version data }
                }
            }
        }"#,
    )
    .await;
    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let tx_id = body["data"]["commitTransaction"]["transactionId"]
        .as_str()
        .expect("transaction id");
    assert!(!tx_id.is_empty());
    assert_eq!(body["data"]["commitTransaction"]["replayHit"], false);
    assert_eq!(
        body["data"]["commitTransaction"]["results"][0]["entity"]["data"]["title"],
        "batched"
    );

    let replay = gql(
        &server,
        r#"mutation {
            commitTransaction(input: {
                idempotencyKey: "gql-retry-1"
                operations: [
                    { createEntity: {
                        collection: "tasks"
                        id: "tx-create-1"
                        data: { title: "different-payload" }
                    }}
                ]
            }) {
                transactionId
                replayHit
                results { entity { id version data } }
            }
        }"#,
    )
    .await;
    assert!(replay["errors"].is_null(), "unexpected errors: {replay}");
    assert_eq!(
        replay["data"]["commitTransaction"]["transactionId"], tx_id,
        "same key should replay the cached transaction"
    );
    assert_eq!(replay["data"]["commitTransaction"]["replayHit"], true);
    assert_eq!(
        replay["data"]["commitTransaction"]["results"][0]["entity"]["data"]["title"], "batched",
        "same-key/different-payload replay returns the original cached response"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_commit_transaction_rolls_back_on_conflict() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    gql(
        &server,
        r#"mutation { createTasks(id: "tx-a", input: { title: "A" }) { id } }"#,
    )
    .await;
    gql(
        &server,
        r#"mutation { createTasks(id: "tx-b", input: { title: "B" }) { id } }"#,
    )
    .await;

    let body = gql(
        &server,
        r#"mutation {
            commitTransaction(input: {
                operations: [
                    { updateEntity: {
                        collection: "tasks"
                        id: "tx-a"
                        expectedVersion: 1
                        data: { title: "A-updated" }
                    }}
                    { updateEntity: {
                        collection: "tasks"
                        id: "tx-b"
                        expectedVersion: 99
                        data: { title: "B-stale" }
                    }}
                ]
            }) {
                transactionId
            }
        }"#,
    )
    .await;

    let errors = body["errors"].as_array().expect("errors array");
    assert!(!errors.is_empty(), "expected conflict: {body}");
    assert_eq!(errors[0]["extensions"]["code"], "VERSION_CONFLICT");

    let get_body = gql(&server, r#"{ tasks(id: "tx-a") { id version title } }"#).await;
    assert!(
        get_body["errors"].is_null(),
        "unexpected errors: {get_body}"
    );
    assert_eq!(
        get_body["data"]["tasks"]["title"], "A",
        "first update must be rolled back when a later op conflicts"
    );
    assert_eq!(get_body["data"]["tasks"]["version"], 1);
}

// ── Lifecycle transitions ────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_transition_lifecycle_mutation() {
    let server = lifecycle_server().await;

    // Create a task. The lifecycle field is auto-populated with "draft".
    let create_body = gql(
        &server,
        r#"mutation { createTasks(id: "t-100", input: { title: "design" }) { id version status } }"#,
    )
    .await;
    assert!(
        create_body["errors"].is_null(),
        "unexpected errors: {create_body}"
    );
    assert_eq!(create_body["data"]["createTasks"]["status"], "draft");

    let lifecycle_body = gql(
        &server,
        r#"{ tasks(id: "t-100") {
            id
            status
            lifecycles
            validTransitions(lifecycleName: "status")
        } }"#,
    )
    .await;
    assert!(
        lifecycle_body["errors"].is_null(),
        "unexpected errors: {lifecycle_body}"
    );
    assert_eq!(
        lifecycle_body["data"]["tasks"]["lifecycles"]["status"]["currentState"],
        "draft"
    );
    assert_eq!(
        lifecycle_body["data"]["tasks"]["validTransitions"],
        json!(["submitted"])
    );

    // Transition draft -> submitted.
    let transition_body = gql(
        &server,
        r#"mutation {
            transitionTasksLifecycle(
                id: "t-100",
                lifecycleName: "status",
                targetState: "submitted",
                expectedVersion: 1
            ) { id version status title }
        }"#,
    )
    .await;
    assert!(
        transition_body["errors"].is_null(),
        "unexpected errors: {transition_body}"
    );
    assert_eq!(
        transition_body["data"]["transitionTasksLifecycle"]["version"],
        2
    );
    assert_eq!(
        transition_body["data"]["transitionTasksLifecycle"]["status"],
        "submitted"
    );
    assert_eq!(
        transition_body["data"]["transitionTasksLifecycle"]["title"],
        "design"
    );

    // New state must be visible from a subsequent read.
    let get_body = gql(&server, r#"{ tasks(id: "t-100") { id version status } }"#).await;
    assert_eq!(get_body["data"]["tasks"]["version"], 2);
    assert_eq!(get_body["data"]["tasks"]["status"], "submitted");

    let generic_body = gql(
        &server,
        r#"{ entity(collection: "tasks", id: "t-100") {
            id
            lifecycles
            validTransitions(lifecycleName: "status")
        } }"#,
    )
    .await;
    assert!(
        generic_body["errors"].is_null(),
        "unexpected errors: {generic_body}"
    );
    assert_eq!(
        generic_body["data"]["entity"]["lifecycles"]["status"]["currentState"],
        "submitted"
    );
    assert_eq!(
        generic_body["data"]["entity"]["validTransitions"],
        json!(["approved"])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_invalid_transition_error_has_valid_transitions_extension() {
    let server = lifecycle_server().await;

    gql(
        &server,
        r#"mutation { createTasks(id: "t-bad", input: { title: "x" }) { id } }"#,
    )
    .await;

    // Attempt draft -> approved directly (not allowed; only draft -> submitted).
    let body = gql(
        &server,
        r#"mutation {
            transitionTasksLifecycle(
                id: "t-bad",
                lifecycleName: "status",
                targetState: "approved",
                expectedVersion: 1
            ) { id }
        }"#,
    )
    .await;

    let errors = body["errors"]
        .as_array()
        .expect("errors array for invalid transition");
    assert!(!errors.is_empty(), "expected errors: {body}");
    let ext = &errors[0]["extensions"];
    assert_eq!(
        ext["code"].as_str().unwrap(),
        "INVALID_TRANSITION",
        "error code should be INVALID_TRANSITION: {body}"
    );
    assert_eq!(ext["lifecycleName"], "status");
    assert_eq!(ext["currentState"], "draft");
    assert_eq!(ext["targetState"], "approved");
    let valid = ext["validTransitions"]
        .as_array()
        .expect("validTransitions must be a list");
    let valid_strings: Vec<&str> = valid.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(
        valid_strings,
        vec!["submitted"],
        "valid_transitions should list only `submitted`: {body}"
    );
}

// ── Caller identity propagation ──────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_mutation_respects_caller_identity() {
    let server = test_server();
    seed_tasks_collection(&server).await;

    let body = gql_as(
        &server,
        "agent-1",
        r#"mutation { createTasks(id: "aud-1", input: { title: "hello" }) { id } }"#,
    )
    .await;
    assert!(body["errors"].is_null(), "unexpected errors: {body}");

    // Verify the audit entry for this entity was attributed to `agent-1`.
    let audit = server
        .get("/tenants/default/databases/default/audit/entity/tasks/aud-1")
        .await
        .json::<Value>();
    let entries = audit["entries"].as_array().expect("audit entries array");
    assert!(
        entries.iter().any(|e| e["actor"] == "agent-1"),
        "expected an audit entry attributed to agent-1, got: {audit}"
    );
}
