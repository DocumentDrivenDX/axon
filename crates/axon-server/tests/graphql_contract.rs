//! GraphQL API integration tests.
//!
//! Exercise every GraphQL endpoint — queries, mutations, error paths,
//! introspection, and the playground — against an in-process server backed by
//! SQLite in-memory storage. No mocks, no Playwright: pure HTTP round-trips
//! through the same router used in production.
//!
//! Collection used throughout: "item" → GraphQL type "Item"
//!   Queries:   item(id: ID!), items(limit: Int, afterId: ID)
//!   Mutations: createItem, updateItem, patchItem, deleteItem
//!
//! Each test creates its own server instance for full isolation.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::Mutex;

use axon_api::handler::AxonHandler;
use axon_server::gateway::build_router;
use axon_server::tenant_router::TenantRouter;
use axon_storage::adapter::StorageAdapter;
use axon_storage::SqliteStorageAdapter;

// ── Server helpers ─────────────────────────────────────────────────────────────

fn test_server() -> axum_test::TestServer {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler));
    let app = build_router(tenant_router, "memory", None);
    axum_test::TestServer::new(app)
}

/// Register a collection with a simple label field via the REST API so it
/// shows up in the per-request GraphQL schema.
async fn seed_collection(server: &axum_test::TestServer, name: &str) {
    server
        .post(&format!(
            "/tenants/default/databases/default/collections/{name}"
        ))
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" },
                        "status": { "type": "string" },
                        "week": { "type": ["string", "null"] },
                        "hours": { "type": "number" },
                        "billable": { "type": "boolean" }
                    }
                },
                "indexes": [
                    { "field": "status", "type": "string" }
                ]
            },
            "actor": "test"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
}

/// Register a collection whose schema enforces a minLength:3 constraint on
/// `title` — used by schema validation error tests.
async fn seed_constrained_collection(server: &axum_test::TestServer, name: &str) {
    server
        .post(&format!(
            "/tenants/default/databases/default/collections/{name}"
        ))
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "required": ["title"],
                    "properties": {
                        "title": { "type": "string", "minLength": 3 }
                    }
                }
            },
            "actor": "test"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
}

/// Register two collections with declared link types for relationship-field
/// GraphQL contract tests.
async fn seed_relationship_collections(server: &axum_test::TestServer) {
    server
        .post("/tenants/default/databases/default/collections/user")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "status": { "type": "string" }
                    }
                },
                "link_types": {
                    "assigned-to": {
                        "target_collection": "task",
                        "cardinality": "many-to-many",
                        "metadata_schema": {
                            "type": "object",
                            "properties": {
                                "role": { "type": "string" },
                                "weight": { "type": "number" }
                            }
                        }
                    },
                    "mentors": {
                        "target_collection": "user",
                        "cardinality": "many-to-many"
                    }
                }
            },
            "actor": "test"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    server
        .post("/tenants/default/databases/default/collections/task")
        .json(&json!({
            "schema": {
                "version": 1,
                "entity_schema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "status": { "type": "string" }
                    }
                }
            },
            "actor": "test"
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
}

/// POST a GraphQL document and return the parsed JSON response body.
/// GraphQL always returns HTTP 200; errors live in the `errors` field of the body.
async fn gql(server: &axum_test::TestServer, query: &str) -> Value {
    server
        .post("/tenants/default/databases/default/graphql")
        .json(&json!({"query": query}))
        .await
        .json::<Value>()
}

fn graphql_input_literal(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let fields = map
                .iter()
                .map(|(key, value)| format!("{key}: {}", graphql_input_literal(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {fields} }}")
        }
        Value::Array(items) => {
            let values = items
                .iter()
                .map(graphql_input_literal)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{values}]")
        }
        scalar => serde_json::to_string(scalar).expect("scalar JSON should serialize"),
    }
}

async fn create_item(server: &axum_test::TestServer, id: &str, data: Value) -> Value {
    let input = graphql_input_literal(&data);
    gql(
        server,
        &format!(r#"mutation {{ createItem(id: "{id}", input: {input}) {{ id version }} }}"#),
    )
    .await
}

// ── Playground ────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_playground_returns_html() {
    let server = test_server();
    let resp = server.get("/graphql/playground").await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(
        body.contains("GraphQL"),
        "playground response should contain 'GraphQL'"
    );
}

// ── Introspection ─────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_introspection_root_types() {
    let server = test_server();
    let body = gql(
        &server,
        r"{ __schema { queryType { name } mutationType { name } subscriptionType { name } } }",
    )
    .await;

    assert_eq!(body["data"]["__schema"]["queryType"]["name"], "Query");
    assert_eq!(body["data"]["__schema"]["mutationType"]["name"], "Mutation");
    // No broker passed to build_router → no Subscription type.
    assert!(
        body["data"]["__schema"]["subscriptionType"].is_null(),
        "subscriptionType should be null without a broker: {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_root_contract_types_and_fields_are_introspectable() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(
        &server,
        r#"{
            __schema { types { name } }
            __type(name: "Query") { fields { name } }
        }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let type_names: Vec<&str> = body["data"]["__schema"]["types"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for expected in [
        "Entity",
        "EntityConnection",
        "EntityEdge",
        "PageInfo",
        "CollectionMeta",
        "AuditEntry",
        "AuditConnection",
    ] {
        assert!(
            type_names.contains(&expected),
            "missing root GraphQL type {expected}: {body}"
        );
    }

    let query_fields: Vec<&str> = body["data"]["__type"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["name"].as_str())
        .collect();
    for expected in [
        "entity",
        "entities",
        "collections",
        "collection",
        "auditLog",
    ] {
        assert!(
            query_fields.contains(&expected),
            "missing root GraphQL field {expected}: {body}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_empty_database_exposes_root_schema_and_empty_collections() {
    let server = test_server();

    let body = gql(&server, r"{ collections { name entityCount } }").await;

    assert!(
        body["errors"].is_null(),
        "empty DB should still have a valid root schema: {body}"
    );
    assert_eq!(
        body["data"]["collections"].as_array().unwrap().len(),
        0,
        "empty DB should return no collection metadata"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_collection_type_visible_after_creation() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(&server, r"{ __schema { queryType { fields { name } } } }").await;

    let fields = body["data"]["__schema"]["queryType"]["fields"]
        .as_array()
        .unwrap();
    let names: Vec<&str> = fields.iter().filter_map(|f| f["name"].as_str()).collect();

    assert!(names.contains(&"item"), "should have singular query 'item'");
    assert!(names.contains(&"items"), "should have plural query 'items'");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_list_field_exposes_filter_and_sort_arguments() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(
        &server,
        r#"{
            __type(name: "Query") {
                fields {
                    name
                    args { name type { kind name ofType { kind name } } }
                }
            }
        }"#,
    )
    .await;

    let fields = body["data"]["__type"]["fields"].as_array().unwrap();
    let items = fields
        .iter()
        .find(|field| field["name"] == "items")
        .expect("items query field exists");
    let arg_names: Vec<&str> = items["args"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect();

    assert!(arg_names.contains(&"filter"), "items should accept filter");
    assert!(arg_names.contains(&"sort"), "items should accept sort");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_typed_list_connection_alias_is_registered() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(
        &server,
        r#"{
            __type(name: "Query") {
                fields { name type { kind name ofType { kind name } } }
            }
        }"#,
    )
    .await;

    let fields = body["data"]["__type"]["fields"].as_array().unwrap();
    assert!(
        fields.iter().any(|field| field["name"] == "itemsConnection"),
        "itemsConnection should expose Relay-style list access while items remains compatible: {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_generated_typed_inputs_payloads_filters_and_sorts() {
    let server = test_server();
    seed_constrained_collection(&server, "task").await;

    let introspection = gql(
        &server,
        r#"{
            __schema { types { name } }
            createInput: __type(name: "CreateTaskInput") {
                inputFields { name type { kind name ofType { kind name } } }
            }
            taskFilter: __type(name: "TaskFilter") { inputFields { name } }
            taskSortField: __type(name: "TaskSortField") { enumValues { name } }
            createPayload: __type(name: "CreateTaskPayload") { fields { name } }
            updatePayload: __type(name: "UpdateTaskPayload") { fields { name } }
            patchPayload: __type(name: "PatchTaskPayload") { fields { name } }
            deleteInput: __type(name: "DeleteTaskInput") { inputFields { name } }
            deletePayload: __type(name: "DeleteTaskPayload") { fields { name } }
        }"#,
    )
    .await;
    assert!(
        introspection["errors"].is_null(),
        "unexpected introspection errors: {introspection}"
    );
    let type_names: Vec<&str> = introspection["data"]["__schema"]["types"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for expected in [
        "TaskFilter",
        "TaskSortField",
        "TaskSort",
        "CreateTaskInput",
        "UpdateTaskInput",
        "PatchTaskInput",
        "DeleteTaskInput",
        "CreateTaskPayload",
        "UpdateTaskPayload",
        "PatchTaskPayload",
        "DeleteTaskPayload",
    ] {
        assert!(
            type_names.contains(&expected),
            "missing generated GraphQL type {expected}: {introspection}"
        );
    }
    let create_fields = introspection["data"]["createInput"]["inputFields"]
        .as_array()
        .unwrap();
    let title = create_fields
        .iter()
        .find(|field| field["name"] == "title")
        .expect("title should be present in CreateTaskInput");
    assert_eq!(
        title["type"]["kind"], "NON_NULL",
        "required schema fields should be non-null in create inputs"
    );
    assert!(
        introspection["data"]["taskFilter"]["inputFields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["name"] == "title"),
        "typed filters should expose schema fields"
    );
    assert!(
        introspection["data"]["taskSortField"]["enumValues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["name"] == "title"),
        "typed sort enum should expose schema fields"
    );

    let created = gql(
        &server,
        r#"mutation {
            createTask(id: "typed-1", input: { title: "draft" }) {
                id
                title
                entity { id title }
            }
        }"#,
    )
    .await;
    assert!(
        created["errors"].is_null(),
        "unexpected create errors: {created}"
    );
    assert_eq!(created["data"]["createTask"]["title"], "draft");
    assert_eq!(created["data"]["createTask"]["entity"]["id"], "typed-1");

    let updated = gql(
        &server,
        r#"mutation {
            updateTask(id: "typed-1", version: 1, input: { title: "review" }) {
                id
                version
                title
                entity { title }
            }
        }"#,
    )
    .await;
    assert!(
        updated["errors"].is_null(),
        "unexpected update errors: {updated}"
    );
    assert_eq!(updated["data"]["updateTask"]["title"], "review");
    assert_eq!(updated["data"]["updateTask"]["entity"]["title"], "review");

    let patched = gql(
        &server,
        r#"mutation {
            patchTask(id: "typed-1", version: 2, typedInput: { patch: { title: "approved" } }) {
                id
                version
                title
            }
        }"#,
    )
    .await;
    assert!(
        patched["errors"].is_null(),
        "unexpected patch errors: {patched}"
    );
    assert_eq!(patched["data"]["patchTask"]["title"], "approved");

    let listed = gql(
        &server,
        r#"{
            tasks(
                filter: { title: { contains: "approv" } }
                sort: [{ field: title, direction: "desc" }]
            ) { id title }
        }"#,
    )
    .await;
    assert!(
        listed["errors"].is_null(),
        "unexpected list errors: {listed}"
    );
    assert_eq!(listed["data"]["tasks"][0]["id"], "typed-1");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_relationship_fields_follow_declared_links() {
    let server = test_server();
    seed_relationship_collections(&server).await;

    let introspection = gql(
        &server,
        r#"{
            userType: __type(name: "User") {
                fields {
                    name
                    args { name }
                    type { kind name ofType { kind name } }
                }
            }
            taskType: __type(name: "Task") { fields { name args { name } } }
            edgeType: __type(name: "UserAssignedToRelationshipEdge") {
                fields { name }
            }
        }"#,
    )
    .await;
    assert!(
        introspection["errors"].is_null(),
        "unexpected introspection errors: {introspection}"
    );
    let user_fields = introspection["data"]["userType"]["fields"]
        .as_array()
        .unwrap();
    let assigned_to = user_fields
        .iter()
        .find(|field| field["name"] == "assignedTo")
        .expect("User should expose assignedTo relationship field");
    let assigned_to_args: Vec<&str> = assigned_to["args"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect();
    for expected in ["limit", "after", "afterId", "filter"] {
        assert!(
            assigned_to_args.contains(&expected),
            "relationship field should expose {expected} arg: {introspection}"
        );
    }
    let task_fields: Vec<&str> = introspection["data"]["taskType"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect();
    assert!(
        task_fields.contains(&"assignedToInbound"),
        "Task should expose reverse assignedToInbound relationship: {introspection}"
    );
    let edge_fields: Vec<&str> = introspection["data"]["edgeType"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect();
    for expected in [
        "cursor",
        "node",
        "linkType",
        "metadata",
        "sourceCollection",
        "sourceId",
        "targetCollection",
        "targetId",
    ] {
        assert!(
            edge_fields.contains(&expected),
            "relationship edge should expose {expected}: {introspection}"
        );
    }

    for mutation in [
        r#"mutation { createUser(id: "u1", input: { name: "Ada", status: "active" }) { id } }"#,
        r#"mutation { createUser(id: "u2", input: { name: "Bea", status: "active" }) { id } }"#,
        r#"mutation { createTask(id: "task-a", input: { title: "Open A", status: "open" }) { id } }"#,
        r#"mutation { createTask(id: "task-b", input: { title: "Closed B", status: "closed" }) { id } }"#,
        r#"mutation { createTask(id: "task-z", input: { title: "Archived Z", status: "archived" }) { id } }"#,
        r#"mutation { createLink(sourceCollection: "user", sourceId: "u1", targetCollection: "task", targetId: "task-a", linkType: "assigned-to", metadata: "{\"role\":\"owner\",\"weight\":2}") }"#,
        r#"mutation { createLink(sourceCollection: "user", sourceId: "u1", targetCollection: "task", targetId: "task-b", linkType: "assigned-to", metadata: "{\"role\":\"reviewer\"}") }"#,
        r#"mutation { createLink(sourceCollection: "user", sourceId: "u1", targetCollection: "task", targetId: "task-z", linkType: "assigned-to", metadata: "{\"role\":\"stale\"}") }"#,
        r#"mutation { createLink(sourceCollection: "user", sourceId: "u1", targetCollection: "user", targetId: "u2", linkType: "mentors") }"#,
        r#"mutation { createLink(sourceCollection: "user", sourceId: "u2", targetCollection: "user", targetId: "u1", linkType: "mentors") }"#,
    ] {
        let body = gql(&server, mutation).await;
        assert!(
            body["errors"].is_null(),
            "unexpected mutation errors: {body}"
        );
    }

    let related = gql(
        &server,
        r#"{
            user(id: "u1") {
                id
                assignedTo(limit: 1) {
                    totalCount
                    edges {
                        cursor
                        linkType
                        metadata
                        sourceCollection
                        sourceId
                        targetCollection
                        targetId
                        node { id title status }
                    }
                    pageInfo { hasNextPage hasPreviousPage startCursor endCursor }
                }
                openTasks: assignedTo(filter: { status: { eq: "open" } }) {
                    totalCount
                    edges { node { id status } }
                }
                mentors {
                    edges {
                        node {
                            id
                            mentors { edges { node { id } } }
                        }
                    }
                }
            }
            task(id: "task-a") {
                assignedToInbound {
                    totalCount
                    edges {
                        metadata
                        linkType
                        node { id name status }
                    }
                }
            }
        }"#,
    )
    .await;
    assert!(
        related["errors"].is_null(),
        "unexpected relationship query errors: {related}"
    );
    let assigned = &related["data"]["user"]["assignedTo"];
    assert_eq!(
        assigned["totalCount"], 3,
        "relationship totalCount should include all matched one-hop targets before pagination"
    );
    assert_eq!(assigned["edges"].as_array().unwrap().len(), 1);
    assert_eq!(assigned["edges"][0]["linkType"], "assigned-to");
    assert_eq!(assigned["edges"][0]["sourceCollection"], "user");
    assert_eq!(assigned["edges"][0]["sourceId"], "u1");
    assert_eq!(assigned["edges"][0]["targetCollection"], "task");
    assert_eq!(assigned["edges"][0]["metadata"]["role"], "owner");
    assert_eq!(assigned["edges"][0]["node"]["id"], "task-a");
    assert_eq!(assigned["pageInfo"]["hasNextPage"], true);
    assert_eq!(assigned["pageInfo"]["hasPreviousPage"], false);

    assert_eq!(related["data"]["user"]["openTasks"]["totalCount"], 1);
    assert_eq!(
        related["data"]["user"]["openTasks"]["edges"][0]["node"]["status"],
        "open"
    );
    assert_eq!(
        related["data"]["task"]["assignedToInbound"]["edges"][0]["node"]["id"],
        "u1"
    );
    assert_eq!(
        related["data"]["task"]["assignedToInbound"]["edges"][0]["metadata"]["role"],
        "owner"
    );
    assert_eq!(
        related["data"]["user"]["mentors"]["edges"][0]["node"]["mentors"]["edges"][0]["node"]["id"],
        "u1",
        "cyclic relationships should resolve only as deeply as requested"
    );

    let after = assigned["pageInfo"]["endCursor"]
        .as_str()
        .expect("relationship page should expose an end cursor");
    let paged = gql(
        &server,
        &format!(
            r#"{{
                user(id: "u1") {{
                    assignedTo(after: "{after}", limit: 1) {{
                        totalCount
                        edges {{ node {{ id }} }}
                        pageInfo {{ hasNextPage hasPreviousPage }}
                    }}
                }}
            }}"#
        ),
    )
    .await;
    assert!(
        paged["errors"].is_null(),
        "unexpected relationship pagination errors: {paged}"
    );
    assert_eq!(paged["data"]["user"]["assignedTo"]["totalCount"], 3);
    assert_eq!(
        paged["data"]["user"]["assignedTo"]["edges"][0]["node"]["id"],
        "task-b"
    );
    assert_eq!(
        paged["data"]["user"]["assignedTo"]["pageInfo"]["hasPreviousPage"],
        true
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_link_discovery_and_neighbors_are_exposed() {
    let server = test_server();
    seed_relationship_collections(&server).await;

    let introspection = gql(
        &server,
        r#"{
            query: __type(name: "Query") { fields { name args { name } } }
            candidates: __type(name: "LinkCandidatesPayload") { fields { name } }
            neighborEdge: __type(name: "NeighborEdge") { fields { name } }
        }"#,
    )
    .await;
    assert!(
        introspection["errors"].is_null(),
        "unexpected introspection errors: {introspection}"
    );
    let query_fields: Vec<&str> = introspection["data"]["query"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect();
    assert!(query_fields.contains(&"linkCandidates"));
    assert!(query_fields.contains(&"neighbors"));
    let candidate_fields: Vec<&str> = introspection["data"]["candidates"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect();
    for expected in [
        "targetCollection",
        "linkType",
        "cardinality",
        "existingLinkCount",
        "candidates",
    ] {
        assert!(
            candidate_fields.contains(&expected),
            "candidate payload should expose {expected}: {introspection}"
        );
    }
    let neighbor_edge_fields: Vec<&str> = introspection["data"]["neighborEdge"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect();
    for expected in ["cursor", "node", "linkType", "direction", "metadata"] {
        assert!(
            neighbor_edge_fields.contains(&expected),
            "neighbor edge should expose {expected}: {introspection}"
        );
    }

    for mutation in [
        r#"mutation { createUser(id: "u1", input: { name: "Ada", status: "active" }) { id } }"#,
        r#"mutation { createTask(id: "task-a", input: { title: "Alpha open", status: "open" }) { id } }"#,
        r#"mutation { createTask(id: "task-b", input: { title: "Beta closed", status: "closed" }) { id } }"#,
        r#"mutation { createTask(id: "task-c", input: { title: "Gamma open", status: "open" }) { id } }"#,
        r#"mutation { createLink(sourceCollection: "user", sourceId: "u1", targetCollection: "task", targetId: "task-a", linkType: "assigned-to", metadata: "{\"role\":\"owner\"}") }"#,
        r#"mutation { createLink(sourceCollection: "task", sourceId: "task-a", targetCollection: "task", targetId: "task-b", linkType: "depends-on", metadata: "{\"kind\":\"hard\"}") }"#,
    ] {
        let body = gql(&server, mutation).await;
        assert!(
            body["errors"].is_null(),
            "unexpected mutation errors: {body}"
        );
    }

    let candidates = gql(
        &server,
        r#"{
            linked: linkCandidates(
                sourceCollection: "user"
                sourceId: "u1"
                linkType: "assigned-to"
                filter: { field: "title", op: "contains", value: "Alpha" }
                limit: 5
            ) {
                targetCollection
                linkType
                cardinality
                existingLinkCount
                candidates {
                    alreadyLinked
                    entity { id collection data }
                }
            }
            searched: linkCandidates(
                sourceCollection: "user"
                sourceId: "u1"
                linkType: "assigned-to"
                search: "Beta"
                limit: 1
            ) {
                candidates {
                    alreadyLinked
                    entity { id data }
                }
            }
        }"#,
    )
    .await;
    assert!(
        candidates["errors"].is_null(),
        "unexpected linkCandidates errors: {candidates}"
    );
    assert_eq!(candidates["data"]["linked"]["targetCollection"], "task");
    assert_eq!(candidates["data"]["linked"]["linkType"], "assigned-to");
    assert_eq!(candidates["data"]["linked"]["cardinality"], "many-to-many");
    assert_eq!(candidates["data"]["linked"]["existingLinkCount"], 1);
    assert_eq!(
        candidates["data"]["linked"]["candidates"][0]["entity"]["id"],
        "task-a"
    );
    assert_eq!(
        candidates["data"]["linked"]["candidates"][0]["alreadyLinked"],
        true
    );
    assert_eq!(
        candidates["data"]["searched"]["candidates"][0]["entity"]["id"],
        "task-b"
    );
    assert_eq!(
        candidates["data"]["searched"]["candidates"][0]["alreadyLinked"],
        false
    );

    let neighbors = gql(
        &server,
        r#"{
            neighbors(collection: "task", id: "task-a", limit: 10) {
                totalCount
                groups {
                    linkType
                    direction
                    totalCount
                    edges {
                        cursor
                        metadata
                        sourceCollection
                        sourceId
                        targetCollection
                        targetId
                        node { id collection data }
                    }
                }
                pageInfo { hasNextPage hasPreviousPage endCursor }
            }
            outbound: neighbors(collection: "task", id: "task-a", direction: "outbound", linkType: "depends-on") {
                totalCount
                groups { linkType direction edges { node { id } metadata } }
            }
        }"#,
    )
    .await;
    assert!(
        neighbors["errors"].is_null(),
        "unexpected neighbors errors: {neighbors}"
    );
    assert_eq!(neighbors["data"]["neighbors"]["totalCount"], 2);
    let groups = neighbors["data"]["neighbors"]["groups"].as_array().unwrap();
    let inbound = groups
        .iter()
        .find(|group| group["direction"] == "inbound")
        .expect("task-a should have inbound assigned-to neighbor");
    assert_eq!(inbound["linkType"], "assigned-to");
    assert_eq!(inbound["edges"][0]["metadata"]["role"], "owner");
    assert_eq!(inbound["edges"][0]["node"]["id"], "u1");

    let outbound_group = groups
        .iter()
        .find(|group| group["direction"] == "outbound")
        .expect("task-a should have outbound depends-on neighbor");
    assert_eq!(outbound_group["linkType"], "depends-on");
    assert_eq!(outbound_group["edges"][0]["metadata"]["kind"], "hard");
    assert_eq!(outbound_group["edges"][0]["node"]["id"], "task-b");
    assert_eq!(neighbors["data"]["outbound"]["totalCount"], 1);
    assert_eq!(
        neighbors["data"]["outbound"]["groups"][0]["edges"][0]["node"]["id"],
        "task-b"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_per_collection_aggregate_queries() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let introspection = gql(
        &server,
        r#"{
            aggregate: __type(name: "ItemAggregate") { fields { name } }
            group: __type(name: "ItemAggregateGroup") { fields { name } }
            input: __type(name: "ItemAggregation") { inputFields { name } }
            functions: __type(name: "AxonAggregateFunction") { enumValues { name } }
        }"#,
    )
    .await;
    assert!(
        introspection["errors"].is_null(),
        "unexpected introspection errors: {introspection}"
    );
    assert!(
        introspection["data"]["functions"]["enumValues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["name"] == "SUM"),
        "aggregate function enum should include SUM"
    );

    for (id, data) in [
        (
            "agg-1",
            json!({"label": "Alpha", "status": "approved", "week": "2026-W16", "hours": 4.0, "billable": true}),
        ),
        (
            "agg-2",
            json!({"label": "Beta", "status": "approved", "week": "2026-W16", "billable": false}),
        ),
        (
            "agg-3",
            json!({"label": "Gamma", "status": "approved", "week": "2026-W17", "hours": 6.0, "billable": true}),
        ),
        (
            "agg-4",
            json!({"label": "Draft", "status": "draft", "week": "2026-W16", "hours": 8.0, "billable": false}),
        ),
    ] {
        let body = create_item(&server, id, data).await;
        assert!(body["errors"].is_null(), "unexpected create error: {body}");
    }

    let body = gql(
        &server,
        r#"{
            itemAggregate(
                filter: { status: { eq: "approved" } }
                groupBy: [status, week]
                aggregations: [
                    { function: COUNT }
                    { function: SUM, field: hours }
                    { function: AVG, field: hours }
                    { function: MIN, field: hours }
                    { function: MAX, field: hours }
                ]
            ) {
                totalCount
                groups {
                    key
                    keyFields
                    count
                    values { function field value count }
                }
            }
            items(limit: 1) { id }
        }"#,
    )
    .await;
    assert!(
        body["errors"].is_null(),
        "unexpected aggregate errors: {body}"
    );
    let aggregate = &body["data"]["itemAggregate"];
    assert_eq!(aggregate["totalCount"], 3);
    assert_eq!(aggregate["groups"].as_array().unwrap().len(), 2);
    assert_eq!(body["data"]["items"][0]["id"], "agg-1");

    let week_16 = aggregate["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| group["keyFields"]["week"] == "2026-W16")
        .expect("week 16 group should be present");
    assert_eq!(week_16["count"], 2);
    let sum = week_16["values"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["function"] == "SUM")
        .expect("SUM value should be present");
    assert_eq!(sum["value"].as_f64().unwrap(), 4.0);
    assert_eq!(sum["count"], 1);

    let invalid = gql(
        &server,
        r#"{
            itemAggregate(aggregations: [{ function: SUM, field: label }]) {
                totalCount
            }
        }"#,
    )
    .await;
    assert!(
        invalid["errors"].is_array(),
        "SUM over a string field should fail: {invalid}"
    );
    assert_eq!(
        invalid["errors"][0]["extensions"]["code"],
        "INVALID_ARGUMENT"
    );
    assert_eq!(
        invalid["errors"][0]["extensions"]["category"],
        "AGGREGATION"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_aggregate_empty_collection_returns_zero_count() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(
        &server,
        r#"{
            itemAggregate(aggregations: [{ function: COUNT }]) {
                totalCount
                groups { count }
            }
        }"#,
    )
    .await;
    assert!(
        body["errors"].is_null(),
        "unexpected aggregate errors: {body}"
    );
    assert_eq!(body["data"]["itemAggregate"]["totalCount"], 0);
    assert_eq!(
        body["data"]["itemAggregate"]["groups"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_mutation_fields_registered_per_collection() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(
        &server,
        r"{ __schema { mutationType { fields { name } } } }",
    )
    .await;

    let fields = body["data"]["__schema"]["mutationType"]["fields"]
        .as_array()
        .unwrap();
    let names: Vec<&str> = fields.iter().filter_map(|f| f["name"].as_str()).collect();

    assert!(names.contains(&"createItem"));
    assert!(names.contains(&"updateItem"));
    assert!(names.contains(&"patchItem"));
    assert!(names.contains(&"deleteItem"));
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_multiple_collections_in_schema() {
    let server = test_server();
    seed_collection(&server, "item").await;
    seed_collection(&server, "note").await;

    let body = gql(&server, r"{ __schema { queryType { fields { name } } } }").await;

    let fields = body["data"]["__schema"]["queryType"]["fields"]
        .as_array()
        .unwrap();
    let names: Vec<&str> = fields.iter().filter_map(|f| f["name"].as_str()).collect();

    assert!(names.contains(&"item"));
    assert!(names.contains(&"items"));
    assert!(names.contains(&"note"));
    assert!(names.contains(&"notes"));
}

// ── Queries ───────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_get_missing_entity_returns_null() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(&server, r#"{ item(id: "ghost") { id } }"#).await;

    // No error — missing entity resolves to null.
    assert!(
        body["errors"].is_null(),
        "no errors expected for missing entity: {body}"
    );
    assert!(
        body["data"]["item"].is_null(),
        "missing entity should resolve to null: {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_list_empty_before_any_entities() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(&server, r"{ items { id } }").await;

    let list = body["data"]["items"].as_array().unwrap();
    assert!(
        list.is_empty(),
        "no entities created yet — list should be empty"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_root_collection_metadata_and_missing_collection() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(
        &server,
        r#"{
            collections { name entityCount schemaVersion schema }
            collection(name: "item") { name entityCount schemaVersion schema }
            missing: collection(name: "missing") { name }
        }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let collections = body["data"]["collections"].as_array().unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0]["name"], "item");
    assert_eq!(collections[0]["entityCount"], 0);
    assert_eq!(collections[0]["schemaVersion"], 1);
    assert_eq!(body["data"]["collection"]["name"], "item");
    assert_eq!(body["data"]["collection"]["schema"]["collection"], "item");
    assert!(
        body["data"]["missing"].is_null(),
        "missing collection should resolve to null: {body}"
    );
}

// ── Mutations: create ─────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_create_entity_returns_system_fields() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let body = gql(
        &server,
        r#"mutation { createItem(id: "e1", input: { label: "hello" }) { id version } }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let entity = &body["data"]["createItem"];
    assert_eq!(entity["id"], "e1");
    assert_eq!(entity["version"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_get_entity_after_create() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "e2", input: { label: "world" }) { id } }"#,
    )
    .await;

    let body = gql(&server, r#"{ item(id: "e2") { id version label } }"#).await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let entity = &body["data"]["item"];
    assert_eq!(entity["id"], "e2");
    assert_eq!(entity["version"], 1);
    assert_eq!(entity["label"], "world");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_root_entity_after_create() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let create = create_item(&server, "root-e1", json!({"label": "root"})).await;
    assert!(
        create["errors"].is_null(),
        "unexpected create error: {create}"
    );

    let body = gql(
        &server,
        r#"{
            entity(collection: "item", id: "root-e1") {
                id
                collection
                version
                data
            }
        }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let entity = &body["data"]["entity"];
    assert_eq!(entity["id"], "root-e1");
    assert_eq!(entity["collection"], "item");
    assert_eq!(entity["version"], 1);
    assert_eq!(entity["data"]["label"], "root");
}

// ── Queries: list + pagination ────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_list_entities_after_creates() {
    let server = test_server();
    seed_collection(&server, "item").await;

    for i in 1..=3_u32 {
        gql(
            &server,
            &format!(
                r#"mutation {{ createItem(id: "li-{i:02}", input: {{ label: "L{i}" }}) {{ id }} }}"#
            ),
        )
        .await;
    }

    let body = gql(&server, r"{ items { id } }").await;
    let list = body["data"]["items"].as_array().unwrap();
    assert_eq!(list.len(), 3, "should list all 3 created entities");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_root_entities_connection_one_page() {
    let server = test_server();
    seed_collection(&server, "item").await;

    for i in 1..=3_u32 {
        let body = create_item(
            &server,
            &format!("rc-{i:02}"),
            json!({"label": format!("R{i}")}),
        )
        .await;
        assert!(body["errors"].is_null(), "unexpected create error: {body}");
    }

    let body = gql(
        &server,
        r#"{
            entities(collection: "item", limit: 10) {
                totalCount
                edges { cursor node { id collection data } }
                pageInfo { hasNextPage hasPreviousPage startCursor endCursor }
            }
        }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let connection = &body["data"]["entities"];
    assert_eq!(connection["totalCount"], 3);
    assert_eq!(connection["edges"].as_array().unwrap().len(), 3);
    assert_eq!(connection["pageInfo"]["hasNextPage"], false);
    assert_eq!(connection["pageInfo"]["hasPreviousPage"], false);
    assert_eq!(connection["edges"][0]["node"]["collection"], "item");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_list_with_limit() {
    let server = test_server();
    seed_collection(&server, "item").await;

    for i in 1..=5_u32 {
        gql(
            &server,
            &format!(
                r#"mutation {{ createItem(id: "pg-{i:02}", input: {{ label: "P{i}" }}) {{ id }} }}"#
            ),
        )
        .await;
    }

    let body = gql(&server, r"{ items(limit: 2) { id } }").await;
    let list = body["data"]["items"].as_array().unwrap();
    assert_eq!(list.len(), 2, "limit: 2 should return exactly 2 items");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_list_pagination_via_after_id() {
    let server = test_server();
    seed_collection(&server, "item").await;

    for i in 1..=4_u32 {
        gql(
            &server,
            &format!(
                r#"mutation {{ createItem(id: "ap-{i:02}", input: {{ label: "A{i}" }}) {{ id }} }}"#
            ),
        )
        .await;
    }

    // First page: 2 items.
    let page1 = gql(&server, r"{ items(limit: 2) { id } }").await;
    let page1_items = page1["data"]["items"].as_array().unwrap();
    assert_eq!(page1_items.len(), 2, "first page should return 2 items");

    let last_id = page1_items.last().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Second page: remaining 2 items.
    let page2 = gql(
        &server,
        &format!(r#"{{ items(limit: 2, afterId: "{last_id}") {{ id }} }}"#),
    )
    .await;
    let page2_items = page2["data"]["items"].as_array().unwrap();
    assert!(!page2_items.is_empty(), "second page should have items");

    // No ID should appear in both pages.
    for item in page2_items {
        let id = item["id"].as_str().unwrap();
        assert!(
            !page1_items.iter().any(|p| p["id"] == id),
            "item {id} appeared in both pages"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_root_entities_connection_multi_page_and_invalid_cursor() {
    let server = test_server();
    seed_collection(&server, "item").await;

    for i in 1..=4_u32 {
        let body = create_item(
            &server,
            &format!("gc-{i:02}"),
            json!({"label": format!("G{i}")}),
        )
        .await;
        assert!(body["errors"].is_null(), "unexpected create error: {body}");
    }

    let page1 = gql(
        &server,
        r#"{
            entities(collection: "item", limit: 2) {
                edges { cursor node { id } }
                pageInfo { hasNextPage endCursor }
                totalCount
            }
        }"#,
    )
    .await;
    assert!(
        page1["errors"].is_null(),
        "unexpected page1 errors: {page1}"
    );
    assert_eq!(page1["data"]["entities"]["totalCount"], 4);
    assert_eq!(
        page1["data"]["entities"]["edges"].as_array().unwrap().len(),
        2
    );
    assert_eq!(page1["data"]["entities"]["pageInfo"]["hasNextPage"], true);
    let cursor = page1["data"]["entities"]["pageInfo"]["endCursor"]
        .as_str()
        .unwrap();

    let page2 = gql(
        &server,
        &format!(
            r#"{{
                entities(collection: "item", limit: 2, after: "{cursor}") {{
                    edges {{ node {{ id }} }}
                    pageInfo {{ hasNextPage hasPreviousPage }}
                }}
            }}"#
        ),
    )
    .await;
    assert!(
        page2["errors"].is_null(),
        "unexpected page2 errors: {page2}"
    );
    assert_eq!(
        page2["data"]["entities"]["edges"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        page2["data"]["entities"]["pageInfo"]["hasPreviousPage"],
        true
    );

    let invalid = gql(
        &server,
        r#"{ entities(collection: "item", after: "does-not-exist") { totalCount } }"#,
    )
    .await;
    let errors = invalid["errors"].as_array().unwrap();
    assert!(
        errors[0]["message"].as_str().unwrap().contains("cursor"),
        "invalid cursor should return a structured GraphQL error: {invalid}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_list_filters_and_sorts_entities() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let cases = [
        (
            "te-1",
            json!({"label": "Alpha", "status": "approved", "week": "2026-W16", "hours": 3.0, "billable": true}),
        ),
        (
            "te-2",
            json!({"label": "Beta", "status": "approved", "week": "2026-W16", "hours": 6.0, "billable": true}),
        ),
        (
            "te-3",
            json!({"label": "Gamma", "status": "draft", "week": "2026-W16", "hours": 8.0, "billable": true}),
        ),
        (
            "te-4",
            json!({"label": "Delta", "status": "approved", "week": "2026-W17", "hours": 9.0, "billable": true}),
        ),
    ];

    for (id, data) in cases {
        let input = graphql_input_literal(&data);
        let body = gql(
            &server,
            &format!(r#"mutation {{ createItem(id: "{id}", input: {input}) {{ id }} }}"#),
        )
        .await;
        assert!(body["errors"].is_null(), "unexpected create error: {body}");
    }

    let body = gql(
        &server,
        r#"{
            items(
                filter: {
                    and: [
                        { field: "status", op: "eq", value: "approved" },
                        { field: "week", op: "eq", value: "2026-W16" },
                        { field: "hours", op: "gte", value: 4.0 }
                    ]
                },
                sort: [{ field: "hours", direction: "desc" }]
            ) {
                id
                label
                hours
            }
        }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let list = body["data"]["items"].as_array().unwrap();
    assert_eq!(
        list.len(),
        1,
        "only approved W16 items with hours >= 4 match"
    );
    assert_eq!(list[0]["id"], "te-2");
    assert_eq!(list[0]["label"], "Beta");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_root_entities_filters_and_sorts_connection() {
    let server = test_server();
    seed_collection(&server, "item").await;

    for (id, data) in [
        (
            "grf-1",
            json!({"label": "Alpha", "status": "approved", "hours": 3.0}),
        ),
        (
            "grf-2",
            json!({"label": "Beta", "status": "approved", "hours": 7.0}),
        ),
        (
            "grf-3",
            json!({"label": "Gamma", "status": "draft", "hours": 9.0}),
        ),
    ] {
        let body = create_item(&server, id, data).await;
        assert!(body["errors"].is_null(), "unexpected create error: {body}");
    }

    let body = gql(
        &server,
        r#"{
            entities(
                collection: "item"
                filter: { field: "status", op: "eq", value: "approved" }
                sort: [{ field: "hours", direction: "desc" }]
            ) {
                totalCount
                edges { node { id data } }
            }
        }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let edges = body["data"]["entities"]["edges"].as_array().unwrap();
    assert_eq!(body["data"]["entities"]["totalCount"], 2);
    assert_eq!(edges[0]["node"]["id"], "grf-2");
    assert_eq!(edges[1]["node"]["id"], "grf-1");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_typed_connection_returns_typed_nodes() {
    let server = test_server();
    seed_collection(&server, "item").await;
    let create = create_item(&server, "tc-1", json!({"label": "typed"})).await;
    assert!(
        create["errors"].is_null(),
        "unexpected create error: {create}"
    );

    let body = gql(
        &server,
        r#"{
            itemsConnection {
                totalCount
                edges { cursor node { id version label } }
                pageInfo { hasNextPage }
            }
        }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    assert_eq!(body["data"]["itemsConnection"]["totalCount"], 1);
    assert_eq!(
        body["data"]["itemsConnection"]["edges"][0]["node"]["label"],
        "typed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_list_supports_in_or_and_null_filters() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let cases = [
        (
            "nf-1",
            json!({"label": "Alpha", "status": "approved", "week": Value::Null}),
        ),
        (
            "nf-2",
            json!({"label": "Beta", "status": "submitted", "week": "2026-W16"}),
        ),
        (
            "nf-3",
            json!({"label": "Gamma", "status": "draft", "week": "2026-W16"}),
        ),
    ];

    for (id, data) in cases {
        let input = graphql_input_literal(&data);
        let body = gql(
            &server,
            &format!(r#"mutation {{ createItem(id: "{id}", input: {input}) {{ id }} }}"#),
        )
        .await;
        assert!(body["errors"].is_null(), "unexpected create error: {body}");
    }

    let body = gql(
        &server,
        r#"{
            items(
                filter: {
                    or: [
                        { field: "status", op: "in", value: ["approved", "submitted"] },
                        { field: "week", op: "is_null" }
                    ]
                },
                sort: [{ field: "label" }]
            ) {
                id
                label
            }
        }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let ids: Vec<&str> = body["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["nf-1", "nf-2"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_audit_log_connection_filters_entity_creates() {
    let server = test_server();
    seed_collection(&server, "item").await;
    let create1 = create_item(&server, "aud-1", json!({"label": "audit"})).await;
    assert!(
        create1["errors"].is_null(),
        "unexpected create error: {create1}"
    );
    let create2 = create_item(&server, "aud-2", json!({"label": "other"})).await;
    assert!(
        create2["errors"].is_null(),
        "unexpected create error: {create2}"
    );

    let body = gql(
        &server,
        r#"{
            auditLog(collection: "item", entityId: "aud-1", operation: "entity.create") {
                totalCount
                edges {
                    cursor
                    node { id collection entityId mutation actor dataAfter }
                }
                pageInfo { hasNextPage hasPreviousPage }
            }
        }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let edges = body["data"]["auditLog"]["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(body["data"]["auditLog"]["totalCount"], 1);
    assert_eq!(edges[0]["node"]["collection"], "item");
    assert_eq!(edges[0]["node"]["entityId"], "aud-1");
    assert_eq!(edges[0]["node"]["mutation"], "entity.create");
    assert_eq!(edges[0]["node"]["dataAfter"]["label"], "audit");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_rejects_excessive_depth_and_complexity() {
    let server = test_server();
    seed_collection(&server, "item").await;

    let mut deep_query = String::from("{ __schema { types { fields { type ");
    for _ in 0..24 {
        deep_query.push_str("{ ofType ");
    }
    deep_query.push_str("{ name }");
    for _ in 0..24 {
        deep_query.push_str(" }");
    }
    deep_query.push_str(" } } } }");
    let deep = gql(&server, &deep_query).await;
    assert!(
        deep["errors"].is_array(),
        "depth-limited query should be rejected: {deep}"
    );

    let mut complex_query = String::from("{");
    for i in 0..300 {
        complex_query.push_str(&format!(" c{i}: collections {{ name }}"));
    }
    complex_query.push('}');
    let complex = gql(&server, &complex_query).await;
    assert!(
        complex["errors"].is_array(),
        "complexity-limited query should be rejected: {complex}"
    );
}

// ── Mutations: update ─────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_update_entity_success() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "upd-1", input: { label: "v1" }) { id } }"#,
    )
    .await;

    let body = gql(
        &server,
        r#"mutation { updateItem(id: "upd-1", version: 1, input: { label: "v2" }) { id version label } }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let entity = &body["data"]["updateItem"];
    assert_eq!(entity["id"], "upd-1");
    assert_eq!(entity["version"], 2);
    assert_eq!(entity["label"], "v2");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_update_version_conflict_error_code() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "occ-1", input: { label: "v1" }) { id } }"#,
    )
    .await;

    // Submit with wrong expected version (99 instead of 1).
    let body = gql(
        &server,
        r#"mutation { updateItem(id: "occ-1", version: 99, input: { label: "v2" }) { id } }"#,
    )
    .await;

    let errors = body["errors"].as_array().unwrap();
    assert!(!errors.is_empty(), "expected VERSION_CONFLICT error");
    assert_eq!(
        errors[0]["extensions"]["code"].as_str().unwrap(),
        "VERSION_CONFLICT",
        "error code should be VERSION_CONFLICT: {body}"
    );
}

// ── Mutations: patch ──────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_patch_entity_success() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "pat-1", input: { label: "original" }) { id } }"#,
    )
    .await;

    let body = gql(
        &server,
        r#"mutation { patchItem(id: "pat-1", version: 1, patch: "{\"label\":\"patched\"}") { id version label } }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    let entity = &body["data"]["patchItem"];
    assert_eq!(entity["id"], "pat-1");
    assert_eq!(entity["version"], 2);
    assert_eq!(entity["label"], "patched");
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_patch_version_conflict_error_code() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "poc-1", input: { label: "v1" }) { id } }"#,
    )
    .await;

    let body = gql(
        &server,
        r#"mutation { patchItem(id: "poc-1", version: 99, patch: "{\"label\":\"x\"}") { id } }"#,
    )
    .await;

    let errors = body["errors"].as_array().unwrap();
    assert!(!errors.is_empty(), "expected VERSION_CONFLICT error");
    assert_eq!(
        errors[0]["extensions"]["code"].as_str().unwrap(),
        "VERSION_CONFLICT",
        "error code should be VERSION_CONFLICT: {body}"
    );
}

// ── Mutations: delete ─────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_delete_entity_returns_true() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "del-1", input: { label: "bye" }) { id } }"#,
    )
    .await;

    let body = gql(
        &server,
        r#"mutation { deleteItem(id: "del-1") { deleted } }"#,
    )
    .await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    assert_eq!(
        body["data"]["deleteItem"]["deleted"], true,
        "delete should return true: {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_get_after_delete_returns_null() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "del-2", input: { label: "gone" }) { id } }"#,
    )
    .await;
    gql(
        &server,
        r#"mutation { deleteItem(id: "del-2") { deleted } }"#,
    )
    .await;

    let body = gql(&server, r#"{ item(id: "del-2") { id } }"#).await;

    assert!(
        body["errors"].is_null(),
        "no error expected for missing entity: {body}"
    );
    assert!(
        body["data"]["item"].is_null(),
        "deleted entity should resolve to null: {body}"
    );
}

// ── Error paths ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn graphql_schema_validation_error() {
    let server = test_server();
    // title must be at least 3 characters.
    seed_constrained_collection(&server, "item").await;

    // Submit a title that is too short (2 chars → fails minLength: 3).
    let body = gql(
        &server,
        r#"mutation { createItem(id: "bad-1", input: { title: "ab" }) { id } }"#,
    )
    .await;

    let errors = body["errors"].as_array().unwrap();
    assert!(
        !errors.is_empty(),
        "expected SCHEMA_VALIDATION error: {body}"
    );
    assert_eq!(
        errors[0]["extensions"]["code"].as_str().unwrap(),
        "SCHEMA_VALIDATION",
        "error code should be SCHEMA_VALIDATION: {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_invalid_json_input_returns_error() {
    let server = test_server();
    seed_collection(&server, "item").await;

    // The `input` argument must be valid JSON — pass a broken string.
    let body = gql(
        &server,
        r#"mutation { createItem(id: "bad-json", legacyInput: "not{{json") { id } }"#,
    )
    .await;

    let errors = body["errors"].as_array().unwrap();
    assert!(
        !errors.is_empty(),
        "expected a parse error for invalid JSON input"
    );
    let msg = errors[0]["message"].as_str().unwrap();
    assert!(
        msg.to_lowercase().contains("json"),
        "error message should mention JSON, got: {msg}"
    );
}
