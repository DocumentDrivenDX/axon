//! FEAT-029 parent integration target.
//!
//! This target keeps the shared SCN-017 procurement fixture and the FEAT-029
//! nexiq reference fixture aligned across GraphQL, MCP, parity, and
//! denial/audit assertions under fixed subject and policy-version snapshots.

#![allow(clippy::unwrap_used)]

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use axon_api::{
    handler::AxonHandler,
    test_fixtures::{
        seed_nexiq_reference_fixture, seed_procurement_fixture, NexiqReferenceFixture,
        ProcurementFixture,
    },
};
use axon_server::{gateway::build_router, tenant_router::TenantRouter};
use axon_storage::{adapter::StorageAdapter, SqliteStorageAdapter};
use serde_json::{json, Value};
use tokio::sync::Mutex;

type TestStorage = Box<dyn StorageAdapter + Send + Sync>;
type TestHandler = Arc<Mutex<AxonHandler<TestStorage>>>;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct DecisionCounts {
    allow: usize,
    deny: usize,
    needs_approval: usize,
}

impl DecisionCounts {
    fn push(&mut self, decision: &str) {
        match decision {
            "allow" => self.allow += 1,
            "deny" | "denied" => self.deny += 1,
            "needs_approval" => self.needs_approval += 1,
            other => panic!("unexpected policy decision: {other}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SuiteSnapshot {
    subjects: BTreeSet<String>,
    policy_versions: BTreeMap<String, u64>,
    decision_counts: DecisionCounts,
}

fn test_server_with_handler() -> (axum_test::TestServer, TestHandler) {
    let storage: TestStorage =
        Box::new(SqliteStorageAdapter::open_in_memory().expect("in-memory SQLite"));
    let handler = Arc::new(Mutex::new(AxonHandler::new(storage)));
    let tenant_router = Arc::new(TenantRouter::single(Arc::clone(&handler)));
    let server = axum_test::TestServer::new(build_router(tenant_router, "memory", None));
    (server, handler)
}

async fn seed_procurement(handler: &TestHandler) -> ProcurementFixture {
    let mut guard = handler.lock().await;
    seed_procurement_fixture(&mut *guard).expect("procurement fixture should seed")
}

async fn seed_nexiq(handler: &TestHandler) -> NexiqReferenceFixture {
    let mut guard = handler.lock().await;
    seed_nexiq_reference_fixture(&mut *guard).expect("nexiq fixture should seed")
}

async fn gql_as(server: &axum_test::TestServer, actor: &str, query: &str) -> Value {
    server
        .post("/tenants/default/databases/default/graphql")
        .add_header("x-axon-actor", actor)
        .json(&json!({ "query": query }))
        .await
        .json::<Value>()
}

async fn mcp_as(server: &axum_test::TestServer, actor: &str, request: &Value) -> Value {
    server
        .post("/mcp")
        .add_header("x-axon-actor", actor)
        .json(request)
        .await
        .json::<Value>()
}

async fn mcp_query_as(server: &axum_test::TestServer, actor: &str, query: &str) -> Value {
    let response = mcp_as(
        server,
        actor,
        &json!({
            "jsonrpc": "2.0",
            "id": "query",
            "method": "tools/call",
            "params": {
                "name": "axon.query",
                "arguments": {
                    "query": query
                }
            }
        }),
    )
    .await;
    assert!(
        response["error"].is_null(),
        "unexpected MCP protocol error: {response}"
    );
    assert!(
        response["result"]["isError"].is_null() || response["result"]["isError"] == false,
        "unexpected MCP tool error: {response}"
    );
    serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("axon.query should return text content"),
    )
    .expect("axon.query text should parse as a GraphQL JSON response")
}

async fn effective_policy_as(
    server: &axum_test::TestServer,
    actor: &str,
    collection: &str,
    entity_id: &str,
) -> Value {
    let body = gql_as(
        server,
        actor,
        &format!(
            r#"{{
                effectivePolicy(collection: "{collection}", entityId: "{entity_id}") {{
                    policyVersion
                    redactedFields
                    deniedFields
                }}
            }}"#
        ),
    )
    .await;
    assert!(
        body["errors"].is_null(),
        "unexpected effectivePolicy errors for {actor}: {body}"
    );
    body["data"]["effectivePolicy"].clone()
}

async fn explain_policy_as(server: &axum_test::TestServer, actor: &str, input: &str) -> Value {
    let body = gql_as(
        server,
        actor,
        &format!(
            r#"{{
                explainPolicy(input: {input}) {{
                    decision
                    reason
                    policyVersion
                    fieldPaths
                    deniedFields
                    approval {{
                        name
                        decision
                        role
                        reasonRequired
                    }}
                }}
            }}"#
        ),
    )
    .await;
    assert!(
        body["errors"].is_null(),
        "unexpected explainPolicy errors for {actor}: {body}"
    );
    body["data"]["explainPolicy"].clone()
}

async fn tool_policy_as(server: &axum_test::TestServer, actor: &str, tool_name: &str) -> Value {
    let tools = mcp_as(
        server,
        actor,
        &json!({
            "jsonrpc": "2.0",
            "id": "tools",
            "method": "tools/list"
        }),
    )
    .await;
    assert!(
        tools["error"].is_null(),
        "unexpected MCP tools/list error for {actor}: {tools}"
    );
    tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == tool_name)
        .unwrap_or_else(|| panic!("missing MCP tool {tool_name} for {actor}: {tools}"))["policy"]
        .clone()
}

async fn audit_entity_as(
    server: &axum_test::TestServer,
    actor: &str,
    collection: &str,
    entity_id: &str,
) -> Value {
    server
        .get(&format!(
            "/tenants/default/databases/default/audit/entity/{collection}/{entity_id}"
        ))
        .add_header("x-axon-actor", actor)
        .await
        .json::<Value>()
}

fn subject_set(subjects: &[&str]) -> BTreeSet<String> {
    subjects
        .iter()
        .map(|subject| (*subject).to_string())
        .collect()
}

fn string_set(value: &Value) -> BTreeSet<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry.as_str().unwrap().to_string())
        .collect()
}

fn decision_from_mcp(response: &Value) -> &'static str {
    if response["result"]["isError"].is_null() || response["result"]["isError"] == false {
        return "allow";
    }

    match response["result"]["structuredContent"]["outcome"]
        .as_str()
        .expect("structured policy outcome should be a string")
    {
        "denied" => "deny",
        "needs_approval" => "needs_approval",
        other => panic!("unexpected structured MCP outcome: {other}"),
    }
}

async fn run_procurement_graphql_suite(
    server: &axum_test::TestServer,
    fixture: &ProcurementFixture,
) -> SuiteSnapshot {
    let invoice_collection = fixture.collections.invoices.as_str();
    let under_invoice = fixture.ids.under_threshold_invoice.as_str();
    let over_invoice = fixture.ids.over_threshold_invoice.as_str();

    let contractor_query = format!(
        r#"{{
            invoice: entity(collection: "{invoice_collection}", id: "{under_invoice}") {{
                id
                data
            }}
        }}"#
    );
    let contractor_invoice = gql_as(server, fixture.subjects.contractor, &contractor_query).await;
    assert!(
        contractor_invoice["errors"].is_null(),
        "unexpected procurement GraphQL errors: {contractor_invoice}"
    );
    assert_eq!(
        contractor_invoice["data"]["invoice"]["id"], under_invoice,
        "contractor should read the assigned invoice"
    );
    assert_eq!(
        contractor_invoice["data"]["invoice"]["data"]["amount_cents"],
        Value::Null
    );
    assert_eq!(
        contractor_invoice["data"]["invoice"]["data"]["commercial_terms"],
        Value::Null
    );

    let finance_policy = effective_policy_as(
        server,
        fixture.subjects.finance_agent,
        invoice_collection,
        under_invoice,
    )
    .await;
    let contractor_policy = effective_policy_as(
        server,
        fixture.subjects.contractor,
        invoice_collection,
        under_invoice,
    )
    .await;
    assert_eq!(finance_policy["policyVersion"], 1);
    assert_eq!(contractor_policy["policyVersion"], 1);
    assert_eq!(
        string_set(&contractor_policy["redactedFields"]),
        BTreeSet::from(["amount_cents".to_string(), "commercial_terms".to_string(),])
    );

    let allow = explain_policy_as(
        server,
        fixture.subjects.finance_agent,
        &format!(
            r#"{{
                operation: "patch",
                collection: "{invoice_collection}",
                entityId: "{under_invoice}",
                expectedVersion: 1,
                patch: {{
                    amount_cents: 800000,
                    commercial_terms: "net-25 aligned parent harness terms"
                }}
            }}"#
        ),
    )
    .await;
    let needs_approval = explain_policy_as(
        server,
        fixture.subjects.finance_agent,
        &format!(
            r#"{{
                operation: "patch",
                collection: "{invoice_collection}",
                entityId: "{over_invoice}",
                expectedVersion: 1,
                patch: {{
                    amount_cents: 1500000
                }}
            }}"#
        ),
    )
    .await;
    let denied = explain_policy_as(
        server,
        fixture.subjects.contractor,
        &format!(
            r#"{{
                operation: "patch",
                collection: "{invoice_collection}",
                entityId: "{over_invoice}",
                expectedVersion: 1,
                patch: {{
                    commercial_terms: "leaked"
                }}
            }}"#
        ),
    )
    .await;

    assert_eq!(allow["policyVersion"], 1);
    assert_eq!(allow["decision"], "allow");
    assert_eq!(needs_approval["policyVersion"], 1);
    assert_eq!(needs_approval["decision"], "needs_approval");
    assert_eq!(needs_approval["reason"], "needs_approval");
    assert_eq!(needs_approval["approval"]["role"], "finance_approver");
    assert_eq!(needs_approval["approval"]["reasonRequired"], true);
    assert_eq!(denied["policyVersion"], 1);
    assert_eq!(denied["decision"], "deny");
    assert_eq!(denied["reason"], "row_write_denied");
    assert!(denied["fieldPaths"].as_array().unwrap().is_empty());

    let mut decision_counts = DecisionCounts::default();
    decision_counts.push(allow["decision"].as_str().unwrap());
    decision_counts.push(needs_approval["decision"].as_str().unwrap());
    decision_counts.push(denied["decision"].as_str().unwrap());

    SuiteSnapshot {
        subjects: subject_set(&[fixture.subjects.finance_agent, fixture.subjects.contractor]),
        policy_versions: BTreeMap::from([(invoice_collection.to_string(), 1)]),
        decision_counts,
    }
}

async fn run_procurement_mcp_suite(
    server: &axum_test::TestServer,
    fixture: &ProcurementFixture,
) -> SuiteSnapshot {
    let invoice_collection = fixture.collections.invoices.as_str();
    let under_invoice = fixture.ids.under_threshold_invoice.as_str();
    let over_invoice = fixture.ids.over_threshold_invoice.as_str();

    let contractor_query = format!(
        r#"{{
            invoice: entity(collection: "{invoice_collection}", id: "{under_invoice}") {{
                id
                data
            }}
        }}"#
    );
    let contractor_gql = gql_as(server, fixture.subjects.contractor, &contractor_query).await;
    let contractor_mcp = mcp_query_as(server, fixture.subjects.contractor, &contractor_query).await;
    assert!(
        contractor_gql["errors"].is_null(),
        "unexpected procurement GraphQL parity errors: {contractor_gql}"
    );
    assert!(
        contractor_mcp["errors"].is_null(),
        "unexpected procurement MCP parity errors: {contractor_mcp}"
    );
    assert_eq!(contractor_mcp["data"], contractor_gql["data"]);

    let finance_patch_policy =
        tool_policy_as(server, fixture.subjects.finance_agent, "invoices.patch").await;
    let contractor_get_policy =
        tool_policy_as(server, fixture.subjects.contractor, "invoices.get").await;
    assert_eq!(finance_patch_policy["policyVersion"], 1);
    assert_eq!(contractor_get_policy["policyVersion"], 1);
    assert_eq!(
        string_set(&contractor_get_policy["redactedFields"]),
        BTreeSet::from(["amount_cents".to_string(), "commercial_terms".to_string(),])
    );

    let allow = mcp_as(
        server,
        fixture.subjects.finance_agent,
        &json!({
            "jsonrpc": "2.0",
            "id": "procurement-allow",
            "method": "tools/call",
            "params": {
                "name": "invoices.patch",
                "arguments": {
                    "id": under_invoice,
                    "data": {
                        "amount_cents": 800000,
                        "commercial_terms": "net-25 aligned parent harness terms"
                    },
                    "expected_version": 1
                }
            }
        }),
    )
    .await;
    assert!(
        allow["error"].is_null(),
        "unexpected MCP allow protocol error: {allow}"
    );
    let allow_entity: Value = serde_json::from_str(
        allow["result"]["content"][0]["text"]
            .as_str()
            .expect("allowed MCP patch should return entity JSON"),
    )
    .expect("allowed MCP entity payload should parse");
    assert_eq!(allow_entity["version"], 2);
    assert_eq!(allow_entity["data"]["amount_cents"], 800000);
    assert_eq!(
        allow_entity["data"]["commercial_terms"],
        "net-25 aligned parent harness terms"
    );

    let needs_approval = mcp_as(
        server,
        fixture.subjects.finance_agent,
        &json!({
            "jsonrpc": "2.0",
            "id": "procurement-needs-approval",
            "method": "tools/call",
            "params": {
                "name": "invoices.patch",
                "arguments": {
                    "id": over_invoice,
                    "data": {
                        "amount_cents": 1500000
                    },
                    "expected_version": 1
                }
            }
        }),
    )
    .await;
    assert!(
        needs_approval["error"].is_null(),
        "unexpected MCP approval protocol error: {needs_approval}"
    );
    assert_eq!(needs_approval["result"]["isError"], true);
    assert_eq!(
        needs_approval["result"]["structuredContent"]["policyVersion"],
        1
    );
    assert_eq!(
        needs_approval["result"]["structuredContent"]["outcome"],
        "needs_approval"
    );
    assert_eq!(
        needs_approval["result"]["structuredContent"]["approval"]["role"],
        "finance_approver"
    );

    let denied = mcp_as(
        server,
        fixture.subjects.contractor,
        &json!({
            "jsonrpc": "2.0",
            "id": "procurement-denied",
            "method": "tools/call",
            "params": {
                "name": "invoices.patch",
                "arguments": {
                    "id": over_invoice,
                    "data": {
                        "commercial_terms": "leaked"
                    },
                    "expected_version": 1
                }
            }
        }),
    )
    .await;
    assert!(
        denied["error"].is_null(),
        "unexpected MCP denied protocol error: {denied}"
    );
    assert_eq!(denied["result"]["isError"], true);
    assert_eq!(denied["result"]["structuredContent"]["policyVersion"], 1);
    assert_eq!(denied["result"]["structuredContent"]["outcome"], "denied");
    assert_eq!(
        denied["result"]["structuredContent"]["reason"],
        "row_write_denied"
    );
    assert!(
        denied["result"]["structuredContent"]["fieldPath"].is_null(),
        "row denials must not report a field path: {denied}"
    );

    let mut decision_counts = DecisionCounts::default();
    decision_counts.push(decision_from_mcp(&allow));
    decision_counts.push(decision_from_mcp(&needs_approval));
    decision_counts.push(decision_from_mcp(&denied));

    SuiteSnapshot {
        subjects: subject_set(&[fixture.subjects.finance_agent, fixture.subjects.contractor]),
        policy_versions: BTreeMap::from([(invoice_collection.to_string(), 1)]),
        decision_counts,
    }
}

async fn run_procurement_denial_audit_suite(
    server: &axum_test::TestServer,
    fixture: &ProcurementFixture,
) {
    let invoice_collection = fixture.collections.invoices.as_str();
    let under_invoice = fixture.ids.under_threshold_invoice.as_str();
    let over_invoice = fixture.ids.over_threshold_invoice.as_str();

    let finance_audit = audit_entity_as(
        server,
        fixture.subjects.finance_agent,
        invoice_collection,
        under_invoice,
    )
    .await;
    let contractor_audit = audit_entity_as(
        server,
        fixture.subjects.contractor,
        invoice_collection,
        under_invoice,
    )
    .await;

    let finance_entries = finance_audit["entries"].as_array().unwrap();
    let contractor_entries = contractor_audit["entries"].as_array().unwrap();
    assert_eq!(finance_entries.len(), 2);
    assert_eq!(contractor_entries.len(), 2);

    let finance_update = finance_entries
        .iter()
        .find(|entry| entry["mutation"] == "entity.update")
        .expect("finance audit should include the allowed update");
    assert_eq!(finance_update["data_after"]["amount_cents"], 800000);
    assert_eq!(
        finance_update["data_after"]["commercial_terms"],
        "net-25 aligned parent harness terms"
    );

    let contractor_update = contractor_entries
        .iter()
        .find(|entry| entry["mutation"] == "entity.update")
        .expect("contractor audit should include the same update");
    assert_eq!(
        contractor_update["data_before"]["amount_cents"],
        Value::Null
    );
    assert_eq!(contractor_update["data_after"]["amount_cents"], Value::Null);
    assert_eq!(
        contractor_update["data_before"]["commercial_terms"],
        Value::Null
    );
    assert_eq!(
        contractor_update["data_after"]["commercial_terms"],
        Value::Null
    );
    let contractor_audit_text = contractor_audit.to_string();
    assert!(!contractor_audit_text.contains("net-30 standard procurement terms"));
    assert!(!contractor_audit_text.contains("net-25 aligned parent harness terms"));

    let denied_audit = audit_entity_as(
        server,
        fixture.subjects.finance_agent,
        invoice_collection,
        over_invoice,
    )
    .await;
    assert_eq!(
        denied_audit["entries"].as_array().unwrap().len(),
        1,
        "needs-approval and denied writes must not append invoice mutation audit entries"
    );
}

async fn run_nexiq_graphql_suite(
    server: &axum_test::TestServer,
    fixture: &NexiqReferenceFixture,
) -> SuiteSnapshot {
    let consultant_query = format!(
        r#"{{
            visibleEngagement: entity(collection: "{engagements}", id: "{engagement_alpha}") {{ id data }}
            hiddenEngagement: entity(collection: "{engagements}", id: "{engagement_beta}") {{ id data }}
            contracts: entities(collection: "{contracts}", limit: 10) {{
                totalCount
                edges {{ node {{ id }} }}
            }}
            tasks: entities(collection: "{tasks}", limit: 10) {{
                totalCount
                edges {{ node {{ id }} }}
            }}
            invoices: entities(collection: "{invoices}", limit: 10) {{
                totalCount
                edges {{ node {{ id }} }}
            }}
        }}"#,
        engagements = fixture.collections.engagements.as_str(),
        engagement_alpha = fixture.ids.engagement_alpha.as_str(),
        engagement_beta = fixture.ids.engagement_beta.as_str(),
        contracts = fixture.collections.contracts.as_str(),
        tasks = fixture.collections.tasks.as_str(),
        invoices = fixture.collections.invoices.as_str(),
    );
    let consultant = gql_as(server, fixture.subjects.consultant, &consultant_query).await;
    assert!(
        consultant["errors"].is_null(),
        "unexpected consultant GraphQL errors: {consultant}"
    );
    assert_eq!(
        consultant["data"]["visibleEngagement"]["id"],
        fixture.ids.engagement_alpha.as_str()
    );
    assert_eq!(consultant["data"]["hiddenEngagement"], Value::Null);
    assert_eq!(consultant["data"]["contracts"]["totalCount"], 1);
    assert_eq!(consultant["data"]["tasks"]["totalCount"], 1);
    assert_eq!(consultant["data"]["invoices"]["totalCount"], 0);

    let contractor_query = format!(
        r#"{{
            engagement: entity(collection: "{engagements}", id: "{engagement_alpha}") {{ id data }}
            invoices: entities(collection: "{invoices}", limit: 10) {{
                totalCount
                edges {{ node {{ id }} }}
            }}
        }}"#,
        engagements = fixture.collections.engagements.as_str(),
        engagement_alpha = fixture.ids.engagement_alpha.as_str(),
        invoices = fixture.collections.invoices.as_str(),
    );
    let contractor = gql_as(server, fixture.subjects.contractor, &contractor_query).await;
    assert!(
        contractor["errors"].is_null(),
        "unexpected contractor GraphQL errors: {contractor}"
    );
    assert_eq!(
        contractor["data"]["engagement"]["data"]["budget_cents"],
        Value::Null
    );
    assert_eq!(
        contractor["data"]["engagement"]["data"]["rate_card_id"],
        Value::Null
    );
    assert_eq!(contractor["data"]["invoices"]["totalCount"], 0);

    let ops_query = format!(
        r#"{{
            contract: entity(collection: "{contracts}", id: "{contract_alpha}") {{ id data }}
            invoice: entity(collection: "{invoices}", id: "{invoice_alpha}") {{ id data }}
            timeVisible: entity(collection: "{time_entries}", id: "{time_entry_alpha}") {{ id data }}
            timeHidden: entity(collection: "{time_entries}", id: "{time_entry_beta}") {{ id data }}
        }}"#,
        contracts = fixture.collections.contracts.as_str(),
        contract_alpha = fixture.ids.contract_alpha.as_str(),
        invoices = fixture.collections.invoices.as_str(),
        invoice_alpha = fixture.ids.invoice_alpha.as_str(),
        time_entries = fixture.collections.time_entries.as_str(),
        time_entry_alpha = fixture.ids.time_entry_alpha.as_str(),
        time_entry_beta = fixture.ids.time_entry_beta.as_str(),
    );
    let ops_manager = gql_as(server, fixture.subjects.ops_manager, &ops_query).await;
    assert!(
        ops_manager["errors"].is_null(),
        "unexpected ops-manager GraphQL errors: {ops_manager}"
    );
    assert_eq!(
        ops_manager["data"]["contract"]["data"]["rate_card_entries"],
        Value::Null
    );
    assert_eq!(
        ops_manager["data"]["invoice"]["id"],
        fixture.ids.invoice_alpha.as_str()
    );
    assert_eq!(ops_manager["data"]["timeHidden"], Value::Null);

    let consultant_policy = effective_policy_as(
        server,
        fixture.subjects.consultant,
        fixture.collections.engagements.as_str(),
        fixture.ids.engagement_alpha.as_str(),
    )
    .await;
    let ops_policy = effective_policy_as(
        server,
        fixture.subjects.ops_manager,
        fixture.collections.time_entries.as_str(),
        fixture.ids.time_entry_alpha.as_str(),
    )
    .await;
    assert_eq!(consultant_policy["policyVersion"], 1);
    assert_eq!(ops_policy["policyVersion"], 1);

    let engagement_denial = gql_as(
        server,
        fixture.subjects.consultant,
        &format!(
            r#"mutation {{
                patchEngagements(
                    id: "{engagement_alpha}",
                    version: 1,
                    typedInput: {{ patch: {{ status: "closed" }} }}
                ) {{ id }}
            }}"#,
            engagement_alpha = fixture.ids.engagement_alpha.as_str(),
        ),
    )
    .await;
    let time_denial = gql_as(
        server,
        fixture.subjects.ops_manager,
        &format!(
            r#"mutation {{
                patchTimeEntries(
                    id: "{time_entry_alpha}",
                    version: 1,
                    typedInput: {{ patch: {{ status: "approved" }} }}
                ) {{ id }}
            }}"#,
            time_entry_alpha = fixture.ids.time_entry_alpha.as_str(),
        ),
    )
    .await;
    assert_eq!(
        engagement_denial["errors"][0]["extensions"]["code"],
        "forbidden"
    );
    assert_eq!(
        engagement_denial["errors"][0]["extensions"]["detail"]["reason"],
        "field_write_denied"
    );
    assert_eq!(
        engagement_denial["errors"][0]["extensions"]["detail"]["field_path"],
        "status"
    );
    assert_eq!(time_denial["errors"][0]["extensions"]["code"], "forbidden");
    assert_eq!(
        time_denial["errors"][0]["extensions"]["detail"]["reason"],
        "row_write_denied"
    );
    assert!(
        time_denial["errors"][0]["extensions"]["detail"]["field_path"].is_null(),
        "row denial should not name a field path: {time_denial}"
    );

    let mut decision_counts = DecisionCounts::default();
    decision_counts.push("deny");
    decision_counts.push("deny");

    SuiteSnapshot {
        subjects: subject_set(&[
            fixture.subjects.consultant,
            fixture.subjects.contractor,
            fixture.subjects.ops_manager,
        ]),
        policy_versions: BTreeMap::from([
            (fixture.collections.engagements.as_str().to_string(), 1),
            (fixture.collections.time_entries.as_str().to_string(), 1),
        ]),
        decision_counts,
    }
}

async fn run_nexiq_mcp_suite(
    server: &axum_test::TestServer,
    fixture: &NexiqReferenceFixture,
) -> SuiteSnapshot {
    let consultant_query = format!(
        r#"{{
            visibleEngagement: entity(collection: "{engagements}", id: "{engagement_alpha}") {{ id data }}
            hiddenEngagement: entity(collection: "{engagements}", id: "{engagement_beta}") {{ id data }}
            engagements: entities(collection: "{engagements}", limit: 10) {{
                totalCount
                edges {{ node {{ id }} }}
            }}
            visibleContract: entity(collection: "{contracts}", id: "{contract_alpha}") {{ id data }}
            hiddenContract: entity(collection: "{contracts}", id: "{contract_beta}") {{ id data }}
            visibleTask: entity(collection: "{tasks}", id: "{task_alpha}") {{ id data }}
            hiddenTask: entity(collection: "{tasks}", id: "{task_beta}") {{ id data }}
            invoices: entities(collection: "{invoices}", limit: 10) {{
                totalCount
                edges {{ node {{ id }} }}
            }}
        }}"#,
        engagements = fixture.collections.engagements.as_str(),
        engagement_alpha = fixture.ids.engagement_alpha.as_str(),
        engagement_beta = fixture.ids.engagement_beta.as_str(),
        contracts = fixture.collections.contracts.as_str(),
        contract_alpha = fixture.ids.contract_alpha.as_str(),
        contract_beta = fixture.ids.contract_beta.as_str(),
        tasks = fixture.collections.tasks.as_str(),
        task_alpha = fixture.ids.task_alpha.as_str(),
        task_beta = fixture.ids.task_beta.as_str(),
        invoices = fixture.collections.invoices.as_str(),
    );
    let consultant_gql = gql_as(server, fixture.subjects.consultant, &consultant_query).await;
    let consultant_mcp = mcp_query_as(server, fixture.subjects.consultant, &consultant_query).await;
    assert!(
        consultant_gql["errors"].is_null(),
        "unexpected consultant GraphQL parity errors: {consultant_gql}"
    );
    assert!(
        consultant_mcp["errors"].is_null(),
        "unexpected consultant MCP parity errors: {consultant_mcp}"
    );
    assert_eq!(consultant_mcp["data"], consultant_gql["data"]);

    let contractor_query = format!(
        r#"{{
            engagement: entity(collection: "{engagements}", id: "{engagement_alpha}") {{ id data }}
            invoice: entity(collection: "{invoices}", id: "{invoice_alpha}") {{ id data }}
        }}"#,
        engagements = fixture.collections.engagements.as_str(),
        engagement_alpha = fixture.ids.engagement_alpha.as_str(),
        invoices = fixture.collections.invoices.as_str(),
        invoice_alpha = fixture.ids.invoice_alpha.as_str(),
    );
    let contractor_gql = gql_as(server, fixture.subjects.contractor, &contractor_query).await;
    let contractor_mcp = mcp_query_as(server, fixture.subjects.contractor, &contractor_query).await;
    assert!(
        contractor_gql["errors"].is_null(),
        "unexpected contractor GraphQL parity errors: {contractor_gql}"
    );
    assert!(
        contractor_mcp["errors"].is_null(),
        "unexpected contractor MCP parity errors: {contractor_mcp}"
    );
    assert_eq!(contractor_mcp["data"], contractor_gql["data"]);

    let ops_query = format!(
        r#"{{
            contract: entity(collection: "{contracts}", id: "{contract_alpha}") {{ id data }}
            invoice: entity(collection: "{invoices}", id: "{invoice_alpha}") {{ id data }}
            timeVisible: entity(collection: "{time_entries}", id: "{time_entry_alpha}") {{ id data }}
            timeHidden: entity(collection: "{time_entries}", id: "{time_entry_beta}") {{ id data }}
            timeEntries: entities(collection: "{time_entries}", limit: 10) {{
                totalCount
                edges {{ node {{ id data }} }}
            }}
        }}"#,
        contracts = fixture.collections.contracts.as_str(),
        contract_alpha = fixture.ids.contract_alpha.as_str(),
        invoices = fixture.collections.invoices.as_str(),
        invoice_alpha = fixture.ids.invoice_alpha.as_str(),
        time_entries = fixture.collections.time_entries.as_str(),
        time_entry_alpha = fixture.ids.time_entry_alpha.as_str(),
        time_entry_beta = fixture.ids.time_entry_beta.as_str(),
    );
    let ops_gql = gql_as(server, fixture.subjects.ops_manager, &ops_query).await;
    let ops_mcp = mcp_query_as(server, fixture.subjects.ops_manager, &ops_query).await;
    assert!(
        ops_gql["errors"].is_null(),
        "unexpected ops-manager GraphQL parity errors: {ops_gql}"
    );
    assert!(
        ops_mcp["errors"].is_null(),
        "unexpected ops-manager MCP parity errors: {ops_mcp}"
    );
    assert_eq!(ops_mcp["data"], ops_gql["data"]);

    let consultant_policy =
        tool_policy_as(server, fixture.subjects.consultant, "engagements.patch").await;
    let ops_policy =
        tool_policy_as(server, fixture.subjects.ops_manager, "time_entries.patch").await;
    assert_eq!(consultant_policy["policyVersion"], 1);
    assert_eq!(ops_policy["policyVersion"], 1);

    let engagement_denial = mcp_as(
        server,
        fixture.subjects.consultant,
        &json!({
            "jsonrpc": "2.0",
            "id": "nexiq-engagement-denied",
            "method": "tools/call",
            "params": {
                "name": "engagements.patch",
                "arguments": {
                    "id": fixture.ids.engagement_alpha.as_str(),
                    "data": { "status": "closed" },
                    "expected_version": 1
                }
            }
        }),
    )
    .await;
    let time_denial = mcp_as(
        server,
        fixture.subjects.ops_manager,
        &json!({
            "jsonrpc": "2.0",
            "id": "nexiq-time-denied",
            "method": "tools/call",
            "params": {
                "name": "time_entries.patch",
                "arguments": {
                    "id": fixture.ids.time_entry_alpha.as_str(),
                    "data": { "status": "approved" },
                    "expected_version": 1
                }
            }
        }),
    )
    .await;
    assert!(
        engagement_denial["error"].is_null(),
        "unexpected MCP engagement denial protocol error: {engagement_denial}"
    );
    assert!(
        time_denial["error"].is_null(),
        "unexpected MCP time-entry denial protocol error: {time_denial}"
    );
    assert_eq!(
        engagement_denial["result"]["structuredContent"]["policyVersion"],
        1
    );
    assert_eq!(
        engagement_denial["result"]["structuredContent"]["reason"],
        "field_write_denied"
    );
    assert_eq!(
        engagement_denial["result"]["structuredContent"]["fieldPath"],
        "status"
    );
    assert_eq!(
        time_denial["result"]["structuredContent"]["policyVersion"],
        1
    );
    assert_eq!(
        time_denial["result"]["structuredContent"]["reason"],
        "row_write_denied"
    );
    assert!(
        time_denial["result"]["structuredContent"]["fieldPath"].is_null(),
        "row denial should not name a field path: {time_denial}"
    );

    let mut decision_counts = DecisionCounts::default();
    decision_counts.push(decision_from_mcp(&engagement_denial));
    decision_counts.push(decision_from_mcp(&time_denial));

    SuiteSnapshot {
        subjects: subject_set(&[
            fixture.subjects.consultant,
            fixture.subjects.contractor,
            fixture.subjects.ops_manager,
        ]),
        policy_versions: BTreeMap::from([
            (fixture.collections.engagements.as_str().to_string(), 1),
            (fixture.collections.time_entries.as_str().to_string(), 1),
        ]),
        decision_counts,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn feat_029_contract_parent_keeps_reference_policy_contracts_in_sync() {
    let (procurement_server, procurement_handler) = test_server_with_handler();
    let procurement_fixture = seed_procurement(&procurement_handler).await;

    let procurement_graphql =
        run_procurement_graphql_suite(&procurement_server, &procurement_fixture).await;
    let procurement_mcp =
        run_procurement_mcp_suite(&procurement_server, &procurement_fixture).await;
    assert_eq!(procurement_graphql, procurement_mcp);
    assert_eq!(
        procurement_graphql.decision_counts,
        DecisionCounts {
            allow: 1,
            deny: 1,
            needs_approval: 1,
        }
    );
    run_procurement_denial_audit_suite(&procurement_server, &procurement_fixture).await;

    let (nexiq_server, nexiq_handler) = test_server_with_handler();
    let nexiq_fixture = seed_nexiq(&nexiq_handler).await;

    let nexiq_graphql = run_nexiq_graphql_suite(&nexiq_server, &nexiq_fixture).await;
    let nexiq_mcp = run_nexiq_mcp_suite(&nexiq_server, &nexiq_fixture).await;
    assert_eq!(nexiq_graphql, nexiq_mcp);
    assert_eq!(
        nexiq_graphql.decision_counts,
        DecisionCounts {
            allow: 0,
            deny: 2,
            needs_approval: 0,
        }
    );
}
