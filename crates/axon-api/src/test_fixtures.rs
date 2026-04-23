//! Shared backend fixtures used by transport-level tests.

use std::collections::HashMap;

use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_schema::{
    AccessControlPolicy, Cardinality, CollectionSchema, CompoundIndexDef, CompoundIndexField,
    IndexDef, IndexType, LifecycleDef, LinkTypeDef,
};
use axon_storage::StorageAdapter;
use serde_json::{json, Value};

use crate::handler::AxonHandler;
use crate::request::{CreateCollectionRequest, CreateEntityRequest, CreateLinkRequest};

pub const PROCUREMENT_APPROVAL_THRESHOLD_CENTS: i64 = 1_000_000;
const FIXTURE_ACTOR: &str = "procurement-fixture";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcurementSubjects {
    pub finance_agent: &'static str,
    pub finance_approver: &'static str,
    pub requester: &'static str,
    pub contractor: &'static str,
    pub operator: &'static str,
}

impl ProcurementSubjects {
    pub const fn new() -> Self {
        Self {
            finance_agent: "finance-agent",
            finance_approver: "finance-approver",
            requester: "requester",
            contractor: "contractor",
            operator: "operator",
        }
    }
}

impl Default for ProcurementSubjects {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcurementCollections {
    pub users: CollectionId,
    pub vendors: CollectionId,
    pub invoices: CollectionId,
    pub purchase_orders: CollectionId,
    pub approvals: CollectionId,
}

impl ProcurementCollections {
    pub fn new() -> Self {
        Self {
            users: CollectionId::new("users"),
            vendors: CollectionId::new("vendors"),
            invoices: CollectionId::new("invoices"),
            purchase_orders: CollectionId::new("purchase_orders"),
            approvals: CollectionId::new("approvals"),
        }
    }

    pub fn all(&self) -> [&CollectionId; 5] {
        [
            &self.users,
            &self.vendors,
            &self.invoices,
            &self.purchase_orders,
            &self.approvals,
        ]
    }
}

impl Default for ProcurementCollections {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcurementEntityIds {
    pub finance_agent: EntityId,
    pub finance_approver: EntityId,
    pub requester: EntityId,
    pub contractor: EntityId,
    pub operator: EntityId,
    pub primary_vendor: EntityId,
    pub secondary_vendor: EntityId,
    pub under_threshold_invoice: EntityId,
    pub over_threshold_invoice: EntityId,
    pub under_threshold_purchase_order: EntityId,
    pub over_threshold_purchase_order: EntityId,
    pub invoice_approval: EntityId,
    pub purchase_order_approval: EntityId,
}

impl ProcurementEntityIds {
    pub fn new(subjects: &ProcurementSubjects) -> Self {
        Self {
            finance_agent: EntityId::new(subjects.finance_agent),
            finance_approver: EntityId::new(subjects.finance_approver),
            requester: EntityId::new(subjects.requester),
            contractor: EntityId::new(subjects.contractor),
            operator: EntityId::new(subjects.operator),
            primary_vendor: EntityId::new("vendor-acme"),
            secondary_vendor: EntityId::new("vendor-zenith"),
            under_threshold_invoice: EntityId::new("inv-under-threshold"),
            over_threshold_invoice: EntityId::new("inv-over-threshold"),
            under_threshold_purchase_order: EntityId::new("po-under-threshold"),
            over_threshold_purchase_order: EntityId::new("po-over-threshold"),
            invoice_approval: EntityId::new("approval-invoice-over"),
            purchase_order_approval: EntityId::new("approval-po-over"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcurementSeedEntity {
    pub collection: CollectionId,
    pub id: EntityId,
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcurementSeedLink {
    pub source_collection: CollectionId,
    pub source_id: EntityId,
    pub target_collection: CollectionId,
    pub target_id: EntityId,
    pub link_type: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcurementFixture {
    pub subjects: ProcurementSubjects,
    pub collections: ProcurementCollections,
    pub ids: ProcurementEntityIds,
    pub schemas: Vec<CollectionSchema>,
    pub entities: Vec<ProcurementSeedEntity>,
    pub links: Vec<ProcurementSeedLink>,
}

impl ProcurementFixture {
    pub fn entity(
        &self,
        collection: &CollectionId,
        id: &EntityId,
    ) -> Option<&ProcurementSeedEntity> {
        self.entities
            .iter()
            .find(|entity| &entity.collection == collection && &entity.id == id)
    }
}

pub fn procurement_fixture() -> Result<ProcurementFixture, AxonError> {
    let subjects = ProcurementSubjects::new();
    let collections = ProcurementCollections::new();
    let ids = ProcurementEntityIds::new(&subjects);
    let schemas = procurement_schemas(&collections)?;
    let entities = procurement_entities(&collections, &ids);
    let links = procurement_links(&collections, &ids);

    Ok(ProcurementFixture {
        subjects,
        collections,
        ids,
        schemas,
        entities,
        links,
    })
}

pub fn procurement_schemas(
    collections: &ProcurementCollections,
) -> Result<Vec<CollectionSchema>, AxonError> {
    Ok(vec![
        users_schema(collections),
        vendors_schema(collections)?,
        invoices_schema(collections)?,
        purchase_orders_schema(collections)?,
        approvals_schema(collections)?,
    ])
}

pub fn seed_procurement_fixture<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
) -> Result<ProcurementFixture, AxonError> {
    let fixture = procurement_fixture()?;

    for schema in &fixture.schemas {
        handler.create_collection(CreateCollectionRequest {
            name: schema.collection.clone(),
            schema: schema.clone(),
            actor: Some(FIXTURE_ACTOR.into()),
        })?;
    }

    for seed in &fixture.entities {
        handler.create_entity(CreateEntityRequest {
            collection: seed.collection.clone(),
            id: seed.id.clone(),
            data: seed.data.clone(),
            actor: Some(FIXTURE_ACTOR.into()),
            audit_metadata: None,
            attribution: None,
        })?;
    }

    for link in &fixture.links {
        handler.create_link(CreateLinkRequest {
            source_collection: link.source_collection.clone(),
            source_id: link.source_id.clone(),
            target_collection: link.target_collection.clone(),
            target_id: link.target_id.clone(),
            link_type: link.link_type.clone(),
            metadata: link.metadata.clone(),
            actor: Some(FIXTURE_ACTOR.into()),
            attribution: None,
        })?;
    }

    Ok(fixture)
}

fn users_schema(collections: &ProcurementCollections) -> CollectionSchema {
    CollectionSchema {
        collection: collections.users.clone(),
        description: Some("Procurement fixture subjects and roles".into()),
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": ["user_id", "display_name", "procurement_role", "department_id"],
            "properties": {
                "user_id": { "type": "string" },
                "display_name": { "type": "string" },
                "procurement_role": {
                    "type": "string",
                    "enum": ["finance_agent", "finance_approver", "requester", "contractor", "operator"]
                },
                "department_id": { "type": "string" }
            }
        })),
        link_types: HashMap::new(),
        access_control: None,
        gates: HashMap::new(),
        validation_rules: Vec::new(),
        indexes: vec![
            index("user_id", IndexType::String, true),
            index("procurement_role", IndexType::String, false),
            index("department_id", IndexType::String, false),
        ],
        compound_indexes: Vec::new(),
        lifecycles: HashMap::new(),
    }
}

fn vendors_schema(collections: &ProcurementCollections) -> Result<CollectionSchema, AxonError> {
    Ok(CollectionSchema {
        collection: collections.vendors.clone(),
        description: Some("Procurement fixture vendors".into()),
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": ["name", "risk_rating", "tax_id"],
            "properties": {
                "name": { "type": "string" },
                "risk_rating": { "type": "string", "enum": ["low", "medium", "high"] },
                "tax_id": { "type": "string" }
            }
        })),
        link_types: HashMap::new(),
        access_control: Some(read_all_procurement_roles_policy()?),
        gates: HashMap::new(),
        validation_rules: Vec::new(),
        indexes: vec![
            index("name", IndexType::String, false),
            index("risk_rating", IndexType::String, false),
        ],
        compound_indexes: Vec::new(),
        lifecycles: HashMap::new(),
    })
}

fn invoices_schema(collections: &ProcurementCollections) -> Result<CollectionSchema, AxonError> {
    Ok(CollectionSchema {
        collection: collections.invoices.clone(),
        description: Some("Procurement fixture invoices with SCN-017 policy envelopes".into()),
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": [
                "number",
                "vendor_id",
                "requester_id",
                "assigned_contractor_id",
                "status",
                "amount_cents",
                "currency",
                "commercial_terms",
                "received_at"
            ],
            "properties": {
                "number": { "type": "string" },
                "vendor_id": { "type": "string" },
                "requester_id": { "type": "string" },
                "assigned_contractor_id": { "type": "string" },
                "purchase_order_id": { "type": "string" },
                "status": { "type": "string", "enum": ["draft", "submitted", "approved", "paid", "void"] },
                "amount_cents": { "type": "integer" },
                "currency": { "type": "string", "enum": ["USD"] },
                "commercial_terms": { "type": "string" },
                "received_at": { "type": "string" },
                "metadata": { "type": "object" }
            }
        })),
        link_types: HashMap::from([
            (
                "vendor".into(),
                link_type(collections.vendors.as_str(), Cardinality::ManyToOne),
            ),
            (
                "requester".into(),
                link_type(collections.users.as_str(), Cardinality::ManyToOne),
            ),
            (
                "assigned_contractor".into(),
                link_type(collections.users.as_str(), Cardinality::ManyToOne),
            ),
            (
                "purchase_order".into(),
                link_type(collections.purchase_orders.as_str(), Cardinality::ManyToOne),
            ),
        ]),
        access_control: Some(invoice_access_control()?),
        gates: HashMap::new(),
        validation_rules: Vec::new(),
        indexes: vec![
            index("status", IndexType::String, false),
            index("vendor_id", IndexType::String, false),
            index("requester_id", IndexType::String, false),
            index("assigned_contractor_id", IndexType::String, false),
            index("amount_cents", IndexType::Integer, false),
            index("received_at", IndexType::Datetime, false),
        ],
        compound_indexes: vec![compound_index(&[
            ("status", IndexType::String),
            ("vendor_id", IndexType::String),
        ])],
        lifecycles: HashMap::from([(
            "status".into(),
            LifecycleDef {
                field: "status".into(),
                initial: "draft".into(),
                transitions: HashMap::from([
                    ("draft".into(), vec!["submitted".into(), "void".into()]),
                    ("submitted".into(), vec!["approved".into(), "void".into()]),
                    ("approved".into(), vec!["paid".into()]),
                    ("paid".into(), Vec::new()),
                    ("void".into(), Vec::new()),
                ]),
            },
        )]),
    })
}

fn purchase_orders_schema(
    collections: &ProcurementCollections,
) -> Result<CollectionSchema, AxonError> {
    Ok(CollectionSchema {
        collection: collections.purchase_orders.clone(),
        description: Some("Procurement fixture purchase orders with approval envelopes".into()),
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": [
                "status",
                "amount_cents",
                "currency",
                "requester_id",
                "vendor_id",
                "department_id",
                "line_items",
                "restricted_notes"
            ],
            "properties": {
                "status": { "type": "string", "enum": ["draft", "submitted", "approved", "rejected"] },
                "amount_cents": { "type": "integer" },
                "currency": { "type": "string", "enum": ["USD"] },
                "requester_id": { "type": "string" },
                "vendor_id": { "type": "string" },
                "department_id": { "type": "string" },
                "line_items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["sku", "quantity", "cost_cents"],
                        "properties": {
                            "sku": { "type": "string" },
                            "quantity": { "type": "integer" },
                            "cost_cents": { "type": "integer" }
                        }
                    }
                },
                "restricted_notes": { "type": "string" }
            }
        })),
        link_types: HashMap::from([
            (
                "vendor".into(),
                link_type(collections.vendors.as_str(), Cardinality::ManyToOne),
            ),
            (
                "requester".into(),
                link_type(collections.users.as_str(), Cardinality::ManyToOne),
            ),
        ]),
        access_control: Some(purchase_order_access_control()?),
        gates: HashMap::new(),
        validation_rules: Vec::new(),
        indexes: vec![
            index("status", IndexType::String, false),
            index("vendor_id", IndexType::String, false),
            index("requester_id", IndexType::String, false),
            index("department_id", IndexType::String, false),
            index("amount_cents", IndexType::Integer, false),
        ],
        compound_indexes: vec![compound_index(&[
            ("status", IndexType::String),
            ("department_id", IndexType::String),
        ])],
        lifecycles: HashMap::from([(
            "status".into(),
            LifecycleDef {
                field: "status".into(),
                initial: "draft".into(),
                transitions: HashMap::from([
                    ("draft".into(), vec!["submitted".into()]),
                    (
                        "submitted".into(),
                        vec!["approved".into(), "rejected".into()],
                    ),
                    ("approved".into(), Vec::new()),
                    ("rejected".into(), Vec::new()),
                ]),
            },
        )]),
    })
}

fn approvals_schema(collections: &ProcurementCollections) -> Result<CollectionSchema, AxonError> {
    Ok(CollectionSchema {
        collection: collections.approvals.clone(),
        description: Some("Procurement fixture approval records".into()),
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": ["target_collection", "target_id", "approver_id", "status", "reason"],
            "properties": {
                "target_collection": { "type": "string", "enum": ["invoices", "purchase_orders"] },
                "target_id": { "type": "string" },
                "approver_id": { "type": "string" },
                "status": { "type": "string", "enum": ["pending", "approved", "rejected"] },
                "reason": { "type": "string" }
            }
        })),
        link_types: HashMap::from([
            (
                "approves_invoice".into(),
                link_type(collections.invoices.as_str(), Cardinality::ManyToOne),
            ),
            (
                "approves_purchase_order".into(),
                link_type(collections.purchase_orders.as_str(), Cardinality::ManyToOne),
            ),
        ]),
        access_control: Some(approval_access_control()?),
        gates: HashMap::new(),
        validation_rules: Vec::new(),
        indexes: vec![
            index("target_collection", IndexType::String, false),
            index("target_id", IndexType::String, false),
            index("approver_id", IndexType::String, false),
            index("status", IndexType::String, false),
        ],
        compound_indexes: vec![compound_index(&[
            ("target_collection", IndexType::String),
            ("target_id", IndexType::String),
        ])],
        lifecycles: HashMap::new(),
    })
}

fn procurement_entities(
    collections: &ProcurementCollections,
    ids: &ProcurementEntityIds,
) -> Vec<ProcurementSeedEntity> {
    vec![
        entity(
            &collections.users,
            &ids.finance_agent,
            json!({
                "user_id": ids.finance_agent.as_str(),
                "display_name": "Finance Agent",
                "procurement_role": "finance_agent",
                "department_id": "finance"
            }),
        ),
        entity(
            &collections.users,
            &ids.finance_approver,
            json!({
                "user_id": ids.finance_approver.as_str(),
                "display_name": "Finance Approver",
                "procurement_role": "finance_approver",
                "department_id": "finance"
            }),
        ),
        entity(
            &collections.users,
            &ids.requester,
            json!({
                "user_id": ids.requester.as_str(),
                "display_name": "Requesting User",
                "procurement_role": "requester",
                "department_id": "engineering"
            }),
        ),
        entity(
            &collections.users,
            &ids.contractor,
            json!({
                "user_id": ids.contractor.as_str(),
                "display_name": "Assigned Contractor",
                "procurement_role": "contractor",
                "department_id": "external"
            }),
        ),
        entity(
            &collections.users,
            &ids.operator,
            json!({
                "user_id": ids.operator.as_str(),
                "display_name": "Audit Operator",
                "procurement_role": "operator",
                "department_id": "operations"
            }),
        ),
        entity(
            &collections.vendors,
            &ids.primary_vendor,
            json!({
                "name": "Acme Office Supply",
                "risk_rating": "low",
                "tax_id": "12-0000001"
            }),
        ),
        entity(
            &collections.vendors,
            &ids.secondary_vendor,
            json!({
                "name": "Zenith Infrastructure",
                "risk_rating": "medium",
                "tax_id": "12-0000002"
            }),
        ),
        entity(
            &collections.purchase_orders,
            &ids.under_threshold_purchase_order,
            json!({
                "status": "submitted",
                "amount_cents": 750000,
                "currency": "USD",
                "requester_id": ids.requester.as_str(),
                "vendor_id": ids.primary_vendor.as_str(),
                "department_id": "engineering",
                "line_items": [
                    { "sku": "SUP-100", "quantity": 3, "cost_cents": 250000 }
                ],
                "restricted_notes": "Standard office hardware refresh"
            }),
        ),
        entity(
            &collections.purchase_orders,
            &ids.over_threshold_purchase_order,
            json!({
                "status": "submitted",
                "amount_cents": 1250000,
                "currency": "USD",
                "requester_id": ids.requester.as_str(),
                "vendor_id": ids.secondary_vendor.as_str(),
                "department_id": "engineering",
                "line_items": [
                    { "sku": "INF-900", "quantity": 1, "cost_cents": 1250000 }
                ],
                "restricted_notes": "Requires finance approval before commit"
            }),
        ),
        entity(
            &collections.invoices,
            &ids.under_threshold_invoice,
            json!({
                "number": "INV-1001",
                "vendor_id": ids.primary_vendor.as_str(),
                "requester_id": ids.requester.as_str(),
                "assigned_contractor_id": ids.contractor.as_str(),
                "purchase_order_id": ids.under_threshold_purchase_order.as_str(),
                "status": "submitted",
                "amount_cents": 750000,
                "currency": "USD",
                "commercial_terms": "net-30 standard procurement terms",
                "received_at": "2026-04-01T10:00:00Z",
                "metadata": { "source": "graphql" }
            }),
        ),
        entity(
            &collections.invoices,
            &ids.over_threshold_invoice,
            json!({
                "number": "INV-2001",
                "vendor_id": ids.secondary_vendor.as_str(),
                "requester_id": ids.requester.as_str(),
                "assigned_contractor_id": ids.contractor.as_str(),
                "purchase_order_id": ids.over_threshold_purchase_order.as_str(),
                "status": "submitted",
                "amount_cents": 1250000,
                "currency": "USD",
                "commercial_terms": "net-15 expedited infrastructure terms",
                "received_at": "2026-04-02T10:00:00Z",
                "metadata": { "source": "mcp" }
            }),
        ),
        entity(
            &collections.approvals,
            &ids.invoice_approval,
            json!({
                "target_collection": collections.invoices.as_str(),
                "target_id": ids.over_threshold_invoice.as_str(),
                "approver_id": ids.finance_approver.as_str(),
                "status": "pending",
                "reason": "Amount exceeds autonomous finance-agent threshold"
            }),
        ),
        entity(
            &collections.approvals,
            &ids.purchase_order_approval,
            json!({
                "target_collection": collections.purchase_orders.as_str(),
                "target_id": ids.over_threshold_purchase_order.as_str(),
                "approver_id": ids.finance_approver.as_str(),
                "status": "pending",
                "reason": "Purchase order requires finance approval"
            }),
        ),
    ]
}

fn procurement_links(
    collections: &ProcurementCollections,
    ids: &ProcurementEntityIds,
) -> Vec<ProcurementSeedLink> {
    vec![
        link(
            &collections.purchase_orders,
            &ids.under_threshold_purchase_order,
            &collections.vendors,
            &ids.primary_vendor,
            "vendor",
        ),
        link(
            &collections.purchase_orders,
            &ids.under_threshold_purchase_order,
            &collections.users,
            &ids.requester,
            "requester",
        ),
        link(
            &collections.purchase_orders,
            &ids.over_threshold_purchase_order,
            &collections.vendors,
            &ids.secondary_vendor,
            "vendor",
        ),
        link(
            &collections.purchase_orders,
            &ids.over_threshold_purchase_order,
            &collections.users,
            &ids.requester,
            "requester",
        ),
        link(
            &collections.invoices,
            &ids.under_threshold_invoice,
            &collections.vendors,
            &ids.primary_vendor,
            "vendor",
        ),
        link(
            &collections.invoices,
            &ids.under_threshold_invoice,
            &collections.users,
            &ids.requester,
            "requester",
        ),
        link(
            &collections.invoices,
            &ids.under_threshold_invoice,
            &collections.users,
            &ids.contractor,
            "assigned_contractor",
        ),
        link(
            &collections.invoices,
            &ids.under_threshold_invoice,
            &collections.purchase_orders,
            &ids.under_threshold_purchase_order,
            "purchase_order",
        ),
        link(
            &collections.invoices,
            &ids.over_threshold_invoice,
            &collections.vendors,
            &ids.secondary_vendor,
            "vendor",
        ),
        link(
            &collections.invoices,
            &ids.over_threshold_invoice,
            &collections.users,
            &ids.requester,
            "requester",
        ),
        link(
            &collections.invoices,
            &ids.over_threshold_invoice,
            &collections.users,
            &ids.contractor,
            "assigned_contractor",
        ),
        link(
            &collections.invoices,
            &ids.over_threshold_invoice,
            &collections.purchase_orders,
            &ids.over_threshold_purchase_order,
            "purchase_order",
        ),
        link(
            &collections.approvals,
            &ids.invoice_approval,
            &collections.invoices,
            &ids.over_threshold_invoice,
            "approves_invoice",
        ),
        link(
            &collections.approvals,
            &ids.purchase_order_approval,
            &collections.purchase_orders,
            &ids.over_threshold_purchase_order,
            "approves_purchase_order",
        ),
    ]
}

fn read_all_procurement_roles_policy() -> Result<AccessControlPolicy, AxonError> {
    access_control(json!({
        "identity": procurement_identity_json(),
        "read": {
            "allow": [
                {
                    "name": "procurement-subjects-read-reference-data",
                    "when": {
                        "subject": "role",
                        "in": ["finance_agent", "finance_approver", "requester", "contractor", "operator"]
                    }
                }
            ]
        },
        "create": { "allow": [{ "name": "fixture-seed-create" }] }
    }))
}

fn invoice_access_control() -> Result<AccessControlPolicy, AxonError> {
    access_control(json!({
        "identity": procurement_identity_json(),
        "read": {
            "allow": [
                {
                    "name": "finance-and-operators-read-invoices",
                    "when": { "subject": "role", "in": ["finance_agent", "finance_approver", "operator"] }
                },
                {
                    "name": "requester-reads-own-invoices",
                    "where": { "field": "requester_id", "eq_subject": "user_id" }
                },
                {
                    "name": "contractor-reads-assigned-invoices",
                    "when": { "subject": "role", "eq": "contractor" },
                    "where": { "field": "assigned_contractor_id", "eq_subject": "user_id" }
                }
            ]
        },
        "create": { "allow": [{ "name": "fixture-seed-create" }] },
        "update": {
            "allow": [
                {
                    "name": "finance-agent-updates-invoice-metadata",
                    "when": { "subject": "role", "eq": "finance_agent" }
                }
            ]
        },
        "fields": {
            "amount_cents": contractor_redaction_rule("contractors-do-not-see-invoice-amounts"),
            "commercial_terms": contractor_redaction_rule("contractors-do-not-see-commercial-terms")
        },
        "envelopes": {
            "write": [
                {
                    "name": "auto-approve-small-invoice-update",
                    "when": {
                        "all": [
                            { "operation": "update" },
                            { "field": "amount_cents", "lt": PROCUREMENT_APPROVAL_THRESHOLD_CENTS }
                        ]
                    },
                    "decision": "allow"
                },
                {
                    "name": "require-approval-large-invoice-update",
                    "when": {
                        "all": [
                            { "operation": "update" },
                            { "field": "amount_cents", "gt": PROCUREMENT_APPROVAL_THRESHOLD_CENTS }
                        ]
                    },
                    "decision": "needs_approval",
                    "approval": {
                        "role": "finance_approver",
                        "reason_required": true,
                        "deadline_seconds": 86400,
                        "separation_of_duties": true
                    }
                }
            ]
        }
    }))
}

fn purchase_order_access_control() -> Result<AccessControlPolicy, AxonError> {
    access_control(json!({
        "identity": procurement_identity_json(),
        "read": {
            "allow": [
                {
                    "name": "finance-and-operators-read-purchase-orders",
                    "when": { "subject": "role", "in": ["finance_agent", "finance_approver", "operator"] }
                },
                {
                    "name": "requester-reads-own-purchase-orders",
                    "where": { "field": "requester_id", "eq_subject": "user_id" }
                }
            ]
        },
        "create": { "allow": [{ "name": "fixture-seed-create" }] },
        "update": {
            "allow": [
                {
                    "name": "finance-agent-updates-purchase-orders",
                    "when": { "subject": "role", "eq": "finance_agent" }
                }
            ]
        },
        "fields": {
            "restricted_notes": contractor_redaction_rule("contractors-do-not-see-restricted-notes")
        },
        "envelopes": {
            "write": [
                {
                    "name": "auto-approve-small-purchase-order-update",
                    "when": {
                        "all": [
                            { "operation": "update" },
                            { "field": "amount_cents", "lt": PROCUREMENT_APPROVAL_THRESHOLD_CENTS }
                        ]
                    },
                    "decision": "allow"
                },
                {
                    "name": "require-approval-large-purchase-order-update",
                    "when": {
                        "all": [
                            { "operation": "update" },
                            { "field": "amount_cents", "gt": PROCUREMENT_APPROVAL_THRESHOLD_CENTS }
                        ]
                    },
                    "decision": "needs_approval",
                    "approval": {
                        "role": "finance_approver",
                        "reason_required": true,
                        "deadline_seconds": 86400,
                        "separation_of_duties": true
                    }
                }
            ]
        }
    }))
}

fn approval_access_control() -> Result<AccessControlPolicy, AxonError> {
    access_control(json!({
        "identity": procurement_identity_json(),
        "read": {
            "allow": [
                {
                    "name": "finance-approvers-and-operators-read-approvals",
                    "when": { "subject": "role", "in": ["finance_approver", "operator"] }
                }
            ]
        },
        "create": { "allow": [{ "name": "fixture-seed-create" }] },
        "update": {
            "allow": [
                {
                    "name": "finance-approver-updates-approval",
                    "when": { "subject": "role", "eq": "finance_approver" }
                }
            ]
        }
    }))
}

fn procurement_identity_json() -> Value {
    json!({
        "user_id": "subject.user_id",
        "role": "subject.attributes.procurement_role",
        "department_id": "subject.attributes.department_id",
        "attributes": {
            "procurement_role": {
                "from": "collection",
                "collection": "users",
                "key_field": "user_id",
                "key_subject": "user_id",
                "value_field": "procurement_role"
            },
            "department_id": {
                "from": "collection",
                "collection": "users",
                "key_field": "user_id",
                "key_subject": "user_id",
                "value_field": "department_id"
            }
        }
    })
}

fn contractor_redaction_rule(name: &str) -> Value {
    json!({
        "read": {
            "deny": [
                {
                    "name": name,
                    "when": { "subject": "role", "eq": "contractor" },
                    "redact_as": null
                }
            ]
        }
    })
}

fn access_control(value: Value) -> Result<AccessControlPolicy, AxonError> {
    serde_json::from_value(value).map_err(|err| {
        AxonError::SchemaValidation(format!("invalid procurement access_control fixture: {err}"))
    })
}

fn index(field: &str, index_type: IndexType, unique: bool) -> IndexDef {
    IndexDef {
        field: field.into(),
        index_type,
        unique,
    }
}

fn compound_index(fields: &[(&str, IndexType)]) -> CompoundIndexDef {
    CompoundIndexDef {
        fields: fields
            .iter()
            .map(|(field, index_type)| CompoundIndexField {
                field: (*field).into(),
                index_type: index_type.clone(),
            })
            .collect(),
        unique: false,
    }
}

fn link_type(target_collection: &str, cardinality: Cardinality) -> LinkTypeDef {
    LinkTypeDef {
        target_collection: target_collection.into(),
        cardinality,
        required: false,
        metadata_schema: None,
    }
}

fn entity(collection: &CollectionId, id: &EntityId, data: Value) -> ProcurementSeedEntity {
    ProcurementSeedEntity {
        collection: collection.clone(),
        id: id.clone(),
        data,
    }
}

fn link(
    source_collection: &CollectionId,
    source_id: &EntityId,
    target_collection: &CollectionId,
    target_id: &EntityId,
    link_type: &str,
) -> ProcurementSeedLink {
    ProcurementSeedLink {
        source_collection: source_collection.clone(),
        source_id: source_id.clone(),
        target_collection: target_collection.clone(),
        target_id: target_id.clone(),
        link_type: link_type.into(),
        metadata: Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use axon_core::types::Link;
    use axon_schema::compile_policy_catalog;
    use axon_storage::{MemoryStorageAdapter, StorageAdapter};

    use super::*;
    use crate::request::ListCollectionsRequest;

    #[test]
    fn procurement_fixture_seeds_backend_state() {
        let mut handler = AxonHandler::new(MemoryStorageAdapter::default());
        let fixture =
            seed_procurement_fixture(&mut handler).expect("procurement fixture should seed");

        let collections = handler
            .list_collections(ListCollectionsRequest {})
            .expect("collections should list");
        let collection_names: Vec<String> = collections
            .collections
            .iter()
            .map(|collection| collection.name.clone())
            .collect();
        for collection in fixture.collections.all() {
            assert!(collection_names.contains(&collection.to_string()));
        }

        assert_eq!(
            count_entities(&handler, &fixture.collections.users),
            fixture.subjects_count()
        );
        assert_eq!(count_entities(&handler, &fixture.collections.vendors), 2);
        assert_eq!(count_entities(&handler, &fixture.collections.invoices), 2);
        assert_eq!(
            count_entities(&handler, &fixture.collections.purchase_orders),
            2
        );
        assert_eq!(count_entities(&handler, &fixture.collections.approvals), 2);

        let stored_links = handler
            .storage_ref()
            .range_scan(&Link::links_collection(), None, None, None)
            .expect("stored links should scan");
        assert_eq!(stored_links.len(), fixture.links.len());

        let catalog = compile_policy_catalog(&fixture.schemas).expect("policies should compile");
        assert!(catalog
            .plans
            .contains_key(fixture.collections.invoices.as_str()));
        assert!(catalog
            .plans
            .contains_key(fixture.collections.purchase_orders.as_str()));
    }

    #[test]
    fn procurement_fixture_exposes_shared_expected_data() {
        let fixture = procurement_fixture().expect("fixture should build");
        let invoice = fixture
            .entity(
                &fixture.collections.invoices,
                &fixture.ids.under_threshold_invoice,
            )
            .expect("under-threshold invoice should be present");

        assert_eq!(fixture.subjects.finance_agent, "finance-agent");
        assert_eq!(fixture.subjects.finance_approver, "finance-approver");
        assert_eq!(fixture.subjects.requester, "requester");
        assert_eq!(fixture.subjects.contractor, "contractor");
        assert_eq!(fixture.subjects.operator, "operator");
        assert_eq!(invoice.data["amount_cents"], json!(750000));
        assert!(
            invoice.data["amount_cents"]
                .as_i64()
                .expect("integer amount")
                < PROCUREMENT_APPROVAL_THRESHOLD_CENTS
        );
    }

    fn count_entities<S: StorageAdapter>(
        handler: &AxonHandler<S>,
        collection: &CollectionId,
    ) -> usize {
        handler
            .storage_ref()
            .count(collection)
            .expect("storage count should succeed")
    }

    trait ProcurementFixtureSubjectCount {
        fn subjects_count(&self) -> usize;
    }

    impl ProcurementFixtureSubjectCount for ProcurementFixture {
        fn subjects_count(&self) -> usize {
            5
        }
    }
}
