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
                }
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

/// POST a GraphQL document and return the parsed JSON response body.
/// GraphQL always returns HTTP 200; errors live in the `errors` field of the body.
async fn gql(server: &axum_test::TestServer, query: &str) -> Value {
    server
        .post("/tenants/default/databases/default/graphql")
        .json(&json!({"query": query}))
        .await
        .json::<Value>()
}

async fn create_item(server: &axum_test::TestServer, id: &str, data: Value) -> Value {
    gql(
        server,
        &format!(
            r#"mutation {{ createItem(id: "{id}", input: "{}") {{ id version }} }}"#,
            data.to_string().replace('"', "\\\"")
        ),
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
        r#"mutation { createItem(id: "e1", input: "{\"label\":\"hello\"}") { id version } }"#,
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
        r#"mutation { createItem(id: "e2", input: "{\"label\":\"world\"}") { id } }"#,
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
                r#"mutation {{ createItem(id: "li-{i:02}", input: "{{\"label\":\"L{i}\"}}") {{ id }} }}"#
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
                r#"mutation {{ createItem(id: "pg-{i:02}", input: "{{\"label\":\"P{i}\"}}") {{ id }} }}"#
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
                r#"mutation {{ createItem(id: "ap-{i:02}", input: "{{\"label\":\"A{i}\"}}") {{ id }} }}"#
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
        let body = gql(
            &server,
            &format!(
                r#"mutation {{ createItem(id: "{id}", input: "{}") {{ id }} }}"#,
                data.to_string().replace('"', "\\\"")
            ),
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
        let body = gql(
            &server,
            &format!(
                r#"mutation {{ createItem(id: "{id}", input: "{}") {{ id }} }}"#,
                data.to_string().replace('"', "\\\"")
            ),
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
        r#"mutation { createItem(id: "upd-1", input: "{\"label\":\"v1\"}") { id } }"#,
    )
    .await;

    let body = gql(
        &server,
        r#"mutation { updateItem(id: "upd-1", version: 1, input: "{\"label\":\"v2\"}") { id version label } }"#,
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
        r#"mutation { createItem(id: "occ-1", input: "{\"label\":\"v1\"}") { id } }"#,
    )
    .await;

    // Submit with wrong expected version (99 instead of 1).
    let body = gql(
        &server,
        r#"mutation { updateItem(id: "occ-1", version: 99, input: "{\"label\":\"v2\"}") { id } }"#,
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
        r#"mutation { createItem(id: "pat-1", input: "{\"label\":\"original\"}") { id } }"#,
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
        r#"mutation { createItem(id: "poc-1", input: "{\"label\":\"v1\"}") { id } }"#,
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
        r#"mutation { createItem(id: "del-1", input: "{\"label\":\"bye\"}") { id } }"#,
    )
    .await;

    let body = gql(&server, r#"mutation { deleteItem(id: "del-1") }"#).await;

    assert!(body["errors"].is_null(), "unexpected errors: {body}");
    assert_eq!(
        body["data"]["deleteItem"], true,
        "delete should return true: {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_get_after_delete_returns_null() {
    let server = test_server();
    seed_collection(&server, "item").await;

    gql(
        &server,
        r#"mutation { createItem(id: "del-2", input: "{\"label\":\"gone\"}") { id } }"#,
    )
    .await;
    gql(&server, r#"mutation { deleteItem(id: "del-2") }"#).await;

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
        r#"mutation { createItem(id: "bad-1", input: "{\"title\":\"ab\"}") { id } }"#,
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
        r#"mutation { createItem(id: "bad-json", input: "not{{json") { id } }"#,
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
