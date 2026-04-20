//! GraphQL subscription integration tests over the WebSocket transport.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axon_api::handler::AxonHandler;
use axon_graphql::BroadcastBroker;
use axon_server::gateway::build_router_with_broker;
use axon_server::tenant_router::TenantRouter;
use axon_storage::adapter::StorageAdapter;
use axon_storage::SqliteStorageAdapter;
use serde_json::{json, Value};
use tokio::sync::Mutex;

fn test_server_with_broker() -> (axum_test::TestServer, BroadcastBroker) {
    let storage: Box<dyn StorageAdapter + Send + Sync> =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(handler));
    let broker = BroadcastBroker::default();
    let app = build_router_with_broker(tenant_router, "memory", None, broker.clone());
    let server = axum_test::TestServer::builder().http_transport().build(app);
    (server, broker)
}

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
                        "status": { "type": "string" }
                    }
                }
            }
        }))
        .await
        .assert_status(axum::http::StatusCode::CREATED);
}

async fn wait_for_receivers(broker: &BroadcastBroker, count: usize) {
    for _ in 0..50 {
        if broker.receiver_count() >= count {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!(
        "expected at least {count} subscription receiver(s), got {}",
        broker.receiver_count()
    );
}

async fn receive_ws_json(websocket: &mut axum_test::TestWebSocket) -> Value {
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        websocket.receive_json::<Value>(),
    )
    .await
    .expect("websocket message arrives within the timeout")
}

#[tokio::test(flavor = "multi_thread")]
async fn graphql_transport_ws_subscription_receives_entity_change() {
    let (server, broker) = test_server_with_broker();
    seed_tasks_collection(&server).await;

    let mut websocket = server
        .get_websocket("/tenants/default/databases/default/graphql/ws")
        .add_header("sec-websocket-protocol", "graphql-transport-ws")
        .await
        .into_websocket()
        .await;

    websocket
        .send_json(&json!({"type": "connection_init"}))
        .await;
    let ack = receive_ws_json(&mut websocket).await;
    assert_eq!(ack["type"], "connection_ack");

    websocket
        .send_json(&json!({
            "id": "sub-1",
            "type": "subscribe",
            "payload": {
                "query": r#"
                    subscription {
                        tasksChanged {
                            auditId
                            collection
                            entityId
                            operation
                            data
                            actor
                        }
                    }
                "#
            }
        }))
        .await;

    wait_for_receivers(&broker, 1).await;
    server
        .post("/tenants/default/databases/default/entities/tasks/ws-1")
        .add_header("x-axon-actor", "ws-agent")
        .json(&json!({"data": {"title": "from websocket", "status": "open"}}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let next = receive_ws_json(&mut websocket).await;
    assert_eq!(next["type"], "next");
    assert_eq!(next["id"], "sub-1");
    let event = &next["payload"]["data"]["tasksChanged"];
    assert_eq!(event["auditId"], "2");
    assert_eq!(event["collection"], "tasks");
    assert_eq!(event["entityId"], "ws-1");
    assert_eq!(event["operation"], "create");
    assert_eq!(event["data"]["status"], "open");
    assert_eq!(event["actor"], "ws-agent");

    websocket
        .send_json(&json!({"id": "sub-1", "type": "complete"}))
        .await;
    websocket.close().await;
}
