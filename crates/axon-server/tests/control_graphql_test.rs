//! Integration tests for the control-plane GraphQL surface.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::connect_info::MockConnectInfo;
use axum::Router;
use axum_test::TestServer;
use http::HeaderValue;
use serde_json::{json, Value};
use uuid::Uuid;

use axon_core::auth::{GrantedDatabase, Grants, JwtClaims, Op, TenantId, TenantRole, UserId};
use axon_server::auth::AuthContext;
use axon_server::auth_pipeline::JwtIssuer;
use axon_server::control_plane::ControlPlaneDb;
use axon_server::control_plane_routes::{
    control_plane_routes, optional_jwt_middleware, ControlPlaneState,
};
use axon_server::cors_config::CorsStore;
use axon_server::user_roles::UserRoleStore;
use axon_storage::MemoryStorageAdapter;

const SECRET: &[u8] = b"test-secret-for-control-graphql";
const ISSUER_ID: &str = "test-issuer-control-graphql";

#[allow(clippy::type_complexity)]
fn build_test_env() -> (
    TestServer,
    Arc<JwtIssuer>,
    String,
    String,
    Arc<Mutex<Box<dyn axon_storage::StorageAdapter + Send + Sync>>>,
) {
    let issuer = Arc::new(JwtIssuer::new(SECRET.to_vec(), ISSUER_ID.to_string()));
    let admin_user_id = Uuid::now_v7().to_string();
    let non_admin_user_id = Uuid::now_v7().to_string();
    let user_roles = UserRoleStore::default();
    user_roles.set_cached(admin_user_id.clone(), axon_server::auth::Role::Admin);

    let storage: Box<dyn axon_storage::StorageAdapter + Send + Sync> =
        Box::new(MemoryStorageAdapter::default());
    let storage = Arc::new(Mutex::new(storage));

    let cp_db = ControlPlaneDb::open_in_memory().expect("open control-plane db");
    let state = ControlPlaneState::new(
        Arc::new(tokio::sync::Mutex::new(cp_db)),
        std::env::temp_dir().join(format!("axon-control-graphql-{}", Uuid::now_v7())),
        user_roles,
        CorsStore::default(),
    )
    .with_storage(storage.clone())
    .with_jwt_issuer(issuer.clone());

    let peer: SocketAddr = "127.0.0.1:12345".parse().expect("valid peer");
    let auth = AuthContext::no_auth();
    let app = Router::new()
        .nest("/control", control_plane_routes())
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            optional_jwt_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            auth,
            axon_server::gateway::authenticate_http_request,
        ))
        .layer(MockConnectInfo(peer));

    (
        TestServer::new(app),
        issuer,
        admin_user_id,
        non_admin_user_id,
        storage,
    )
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn make_jwt(issuer: &JwtIssuer, user_id: &str, tenant_id: &str, grants: Grants) -> String {
    let now = now_secs();
    let claims = JwtClaims {
        iss: ISSUER_ID.to_string(),
        sub: user_id.to_string(),
        aud: tenant_id.to_string(),
        jti: Uuid::now_v7().to_string(),
        iat: now,
        nbf: now,
        exp: now + 3600,
        grants,
    };
    issuer.issue(&claims).expect("issue jwt")
}

fn deployment_admin_jwt(issuer: &JwtIssuer, user_id: &str) -> String {
    make_jwt(issuer, user_id, "deployment", Grants { databases: vec![] })
}

fn tenant_jwt(issuer: &JwtIssuer, user_id: &str, tenant_id: &str, ops: Vec<Op>) -> String {
    make_jwt(
        issuer,
        user_id,
        tenant_id,
        Grants {
            databases: vec![GrantedDatabase {
                name: "orders".to_string(),
                ops,
            }],
        },
    )
}

fn auth_header(jwt: &str) -> (http::HeaderName, HeaderValue) {
    (
        http::header::AUTHORIZATION,
        format!("Bearer {jwt}").parse().expect("valid auth header"),
    )
}

async fn gql(server: &TestServer, jwt: &str, query: &str, variables: Value) -> Value {
    let (name, value) = auth_header(jwt);
    server
        .post("/control/graphql")
        .add_header(name, value)
        .json(&json!({ "query": query, "variables": variables }))
        .await
        .json()
}

fn assert_no_errors(body: &Value) {
    assert!(
        body.get("errors").is_none(),
        "GraphQL response contained errors: {body}"
    );
}

fn first_error_code(body: &Value) -> &str {
    body["errors"][0]["extensions"]["code"]
        .as_str()
        .expect("GraphQL error should include extensions.code")
}

fn insert_member(
    storage: &Arc<Mutex<Box<dyn axon_storage::StorageAdapter + Send + Sync>>>,
    tenant_id: &str,
    user_id: &str,
    role: TenantRole,
) {
    let storage = storage.lock().expect("storage lock");
    storage
        .upsert_tenant_member(TenantId::new(tenant_id), UserId::new(user_id), role)
        .expect("insert member");
}

#[tokio::test(flavor = "multi_thread")]
async fn control_graphql_admin_lifecycle_and_rest_parity() {
    let (server, issuer, admin_uid, _, _) = build_test_env();
    let jwt = deployment_admin_jwt(&issuer, &admin_uid);

    let create_tenant = gql(
        &server,
        &jwt,
        r#"
        mutation($name: String!) {
          createTenant(name: $name) { id name dbName dbPath createdAt }
        }
        "#,
        json!({ "name": "Acme GraphQL" }),
    )
    .await;
    assert_no_errors(&create_tenant);
    let tenant_id = create_tenant["data"]["createTenant"]["id"]
        .as_str()
        .expect("tenant id")
        .to_string();

    let user = gql(
        &server,
        &jwt,
        r#"
        mutation {
          provisionUser(displayName: "Ada", email: "ada@example.com") {
            id displayName email createdAtMs suspendedAtMs
          }
        }
        "#,
        json!({}),
    )
    .await;
    assert_no_errors(&user);
    let user_id = user["data"]["provisionUser"]["id"]
        .as_str()
        .expect("user id")
        .to_string();

    let member = gql(
        &server,
        &jwt,
        r#"
        mutation($tenantId: String!, $userId: String!) {
          upsertTenantMember(tenantId: $tenantId, userId: $userId, role: "write") {
            tenantId userId role
          }
        }
        "#,
        json!({ "tenantId": tenant_id, "userId": user_id }),
    )
    .await;
    assert_no_errors(&member);
    assert_eq!(member["data"]["upsertTenantMember"]["role"], "write");

    let database = gql(
        &server,
        &jwt,
        r#"
        mutation($tenantId: String!) {
          createTenantDatabase(tenantId: $tenantId, name: "orders") {
            tenantId name createdAtMs
          }
        }
        "#,
        json!({ "tenantId": tenant_id }),
    )
    .await;
    assert_no_errors(&database);

    let (name, value) = auth_header(&jwt);
    let rest_list: Value = server
        .get(&format!("/control/tenants/{tenant_id}/databases"))
        .add_header(name, value)
        .await
        .json();
    assert_eq!(rest_list["databases"][0]["name"], "orders");

    let second_tenant = gql(
        &server,
        &jwt,
        r#"mutation { createTenant(name: "Beta GraphQL") { id } }"#,
        json!({}),
    )
    .await;
    assert_no_errors(&second_tenant);
    let second_tenant_id = second_tenant["data"]["createTenant"]["id"]
        .as_str()
        .expect("second tenant id");
    let same_db_name = gql(
        &server,
        &jwt,
        r#"
        mutation($tenantId: String!) {
          createTenantDatabase(tenantId: $tenantId, name: "orders") { tenantId name }
        }
        "#,
        json!({ "tenantId": second_tenant_id }),
    )
    .await;
    assert_no_errors(&same_db_name);
    assert_eq!(
        same_db_name["data"]["createTenantDatabase"]["tenantId"],
        second_tenant_id
    );

    let suspend = gql(
        &server,
        &jwt,
        r#"mutation($userId: String!) { suspendUser(userId: $userId) { userId suspended } }"#,
        json!({ "userId": user_id }),
    )
    .await;
    assert_no_errors(&suspend);
    assert_eq!(suspend["data"]["suspendUser"]["suspended"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn control_graphql_tenant_admin_permissions_and_denial_codes() {
    let (server, issuer, _, non_admin_uid, _) = build_test_env();
    let tenant_id = "tenant-gql-authz";

    let tenant_admin_jwt = tenant_jwt(&issuer, &non_admin_uid, tenant_id, vec![Op::Admin]);
    let create_db = gql(
        &server,
        &tenant_admin_jwt,
        r#"
        mutation($tenantId: String!) {
          createTenantDatabase(tenantId: $tenantId, name: "orders") { name }
        }
        "#,
        json!({ "tenantId": tenant_id }),
    )
    .await;
    assert_no_errors(&create_db);

    let read_jwt = tenant_jwt(&issuer, &non_admin_uid, tenant_id, vec![Op::Read]);
    let denied_write = gql(
        &server,
        &read_jwt,
        r#"
        mutation($tenantId: String!) {
          createTenantDatabase(tenantId: $tenantId, name: "invoices") { name }
        }
        "#,
        json!({ "tenantId": tenant_id }),
    )
    .await;
    assert_eq!(first_error_code(&denied_write), "forbidden");

    let denied_admin = gql(
        &server,
        &read_jwt,
        r#"
        mutation($tenantId: String!) {
          upsertTenantMember(tenantId: $tenantId, userId: "target", role: "read") { userId }
        }
        "#,
        json!({ "tenantId": tenant_id }),
    )
    .await;
    assert_eq!(first_error_code(&denied_admin), "forbidden");

    let invalid = gql(
        &server,
        &tenant_admin_jwt,
        r#"
        mutation($tenantId: String!) {
          createTenantDatabase(tenantId: $tenantId, name: "1invalid") { name }
        }
        "#,
        json!({ "tenantId": tenant_id }),
    )
    .await;
    assert_eq!(first_error_code(&invalid), "invalid_identifier");
}

#[tokio::test(flavor = "multi_thread")]
async fn control_graphql_credentials_issue_list_and_revoke() {
    let (server, issuer, admin_uid, _, storage) = build_test_env();
    let tenant_id = "tenant-gql-credentials";
    let target_user = Uuid::now_v7().to_string();
    insert_member(&storage, tenant_id, &target_user, TenantRole::Write);
    let admin_jwt = deployment_admin_jwt(&issuer, &admin_uid);

    let issued = gql(
        &server,
        &admin_jwt,
        r#"
        mutation($tenantId: String!, $targetUser: String!) {
          issueCredential(
            tenantId: $tenantId
            targetUser: $targetUser
            grants: { databases: [{ name: "orders", ops: ["read", "write"] }] }
            ttlSeconds: 3600
          ) {
            jwt
            jti
            expiresAt
          }
        }
        "#,
        json!({ "tenantId": tenant_id, "targetUser": target_user }),
    )
    .await;
    assert_no_errors(&issued);
    assert!(issued["data"]["issueCredential"]["jwt"].is_string());
    let jti = issued["data"]["issueCredential"]["jti"]
        .as_str()
        .expect("jti")
        .to_string();

    let list = gql(
        &server,
        &admin_jwt,
        r#"
        query($tenantId: String!) {
          credentials(tenantId: $tenantId) {
            jti userId tenantId revoked grants
          }
        }
        "#,
        json!({ "tenantId": tenant_id }),
    )
    .await;
    assert_no_errors(&list);
    let credentials = list["data"]["credentials"]
        .as_array()
        .expect("credentials array");
    assert_eq!(credentials.len(), 1);
    assert_eq!(credentials[0]["jti"], jti);
    assert!(
        !list.to_string().contains("\"jwt\""),
        "credential list must not expose signed JWT material: {list}"
    );

    let owner_jwt = tenant_jwt(&issuer, &target_user, tenant_id, vec![Op::Read, Op::Write]);
    let owner_list = gql(
        &server,
        &owner_jwt,
        r#"query($tenantId: String!) { credentials(tenantId: $tenantId) { userId } }"#,
        json!({ "tenantId": tenant_id }),
    )
    .await;
    assert_no_errors(&owner_list);
    assert_eq!(owner_list["data"]["credentials"][0]["userId"], target_user);

    let revoked = gql(
        &server,
        &owner_jwt,
        r#"
        mutation($tenantId: String!, $jti: String!) {
          revokeCredential(tenantId: $tenantId, jti: $jti) { jti revoked }
        }
        "#,
        json!({ "tenantId": tenant_id, "jti": jti }),
    )
    .await;
    assert_no_errors(&revoked);
    assert_eq!(revoked["data"]["revokeCredential"]["revoked"], true);
}
