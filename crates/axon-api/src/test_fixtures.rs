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
const FIXTURE_ACTOR: &str = "shared-test-fixture";

#[derive(Debug, Clone, PartialEq)]
pub struct FixtureSeedEntity {
    pub collection: CollectionId,
    pub id: EntityId,
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FixtureSeedLink {
    pub source_collection: CollectionId,
    pub source_id: EntityId,
    pub target_collection: CollectionId,
    pub target_id: EntityId,
    pub link_type: String,
    pub metadata: Value,
}

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
pub struct ProcurementFixture {
    pub subjects: ProcurementSubjects,
    pub collections: ProcurementCollections,
    pub ids: ProcurementEntityIds,
    pub schemas: Vec<CollectionSchema>,
    pub entities: Vec<FixtureSeedEntity>,
    pub links: Vec<FixtureSeedLink>,
}

impl ProcurementFixture {
    pub fn entity(&self, collection: &CollectionId, id: &EntityId) -> Option<&FixtureSeedEntity> {
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
) -> Vec<FixtureSeedEntity> {
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
) -> Vec<FixtureSeedLink> {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NexiqReferenceSubjects {
    pub admin: &'static str,
    pub partner: &'static str,
    pub consultant: &'static str,
    pub consultant_peer: &'static str,
    pub contractor: &'static str,
    pub ops_manager: &'static str,
}

impl NexiqReferenceSubjects {
    pub const fn new() -> Self {
        Self {
            admin: "admin",
            partner: "partner-lead",
            consultant: "consultant-alex",
            consultant_peer: "consultant-brooke",
            contractor: "contractor-casey",
            ops_manager: "ops-manager",
        }
    }
}

impl Default for NexiqReferenceSubjects {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NexiqReferenceCollections {
    pub users: CollectionId,
    pub engagements: CollectionId,
    pub contracts: CollectionId,
    pub tasks: CollectionId,
    pub invoices: CollectionId,
    pub time_entries: CollectionId,
}

impl NexiqReferenceCollections {
    pub fn new() -> Self {
        Self {
            users: CollectionId::new("users"),
            engagements: CollectionId::new("engagements"),
            contracts: CollectionId::new("contracts"),
            tasks: CollectionId::new("tasks"),
            invoices: CollectionId::new("invoices"),
            time_entries: CollectionId::new("time_entries"),
        }
    }

    pub fn all(&self) -> [&CollectionId; 6] {
        [
            &self.users,
            &self.engagements,
            &self.contracts,
            &self.tasks,
            &self.invoices,
            &self.time_entries,
        ]
    }
}

impl Default for NexiqReferenceCollections {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NexiqReferenceEntityIds {
    pub admin: EntityId,
    pub partner: EntityId,
    pub consultant: EntityId,
    pub consultant_peer: EntityId,
    pub contractor: EntityId,
    pub ops_manager: EntityId,
    pub engagement_alpha: EntityId,
    pub engagement_beta: EntityId,
    pub contract_alpha: EntityId,
    pub contract_beta: EntityId,
    pub task_alpha: EntityId,
    pub task_beta: EntityId,
    pub invoice_alpha: EntityId,
    pub time_entry_alpha: EntityId,
    pub time_entry_beta: EntityId,
}

impl NexiqReferenceEntityIds {
    pub fn new(subjects: &NexiqReferenceSubjects) -> Self {
        Self {
            admin: EntityId::new(subjects.admin),
            partner: EntityId::new(subjects.partner),
            consultant: EntityId::new(subjects.consultant),
            consultant_peer: EntityId::new(subjects.consultant_peer),
            contractor: EntityId::new(subjects.contractor),
            ops_manager: EntityId::new(subjects.ops_manager),
            engagement_alpha: EntityId::new("engagement-alpha"),
            engagement_beta: EntityId::new("engagement-beta"),
            contract_alpha: EntityId::new("contract-alpha"),
            contract_beta: EntityId::new("contract-beta"),
            task_alpha: EntityId::new("task-alpha"),
            task_beta: EntityId::new("task-beta"),
            invoice_alpha: EntityId::new("invoice-alpha"),
            time_entry_alpha: EntityId::new("time-entry-alpha"),
            time_entry_beta: EntityId::new("time-entry-beta"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NexiqReferenceFixture {
    pub subjects: NexiqReferenceSubjects,
    pub collections: NexiqReferenceCollections,
    pub ids: NexiqReferenceEntityIds,
    pub schemas: Vec<CollectionSchema>,
    pub entities: Vec<FixtureSeedEntity>,
    pub links: Vec<FixtureSeedLink>,
}

impl NexiqReferenceFixture {
    pub fn entity(&self, collection: &CollectionId, id: &EntityId) -> Option<&FixtureSeedEntity> {
        self.entities
            .iter()
            .find(|entity| &entity.collection == collection && &entity.id == id)
    }
}

pub fn nexiq_reference_fixture() -> Result<NexiqReferenceFixture, AxonError> {
    let subjects = NexiqReferenceSubjects::new();
    let collections = NexiqReferenceCollections::new();
    let ids = NexiqReferenceEntityIds::new(&subjects);
    let schemas = nexiq_reference_schemas(&collections)?;
    let entities = nexiq_reference_entities(&collections, &ids);
    let links = nexiq_reference_links(&collections, &ids);

    Ok(NexiqReferenceFixture {
        subjects,
        collections,
        ids,
        schemas,
        entities,
        links,
    })
}

pub fn nexiq_reference_schemas(
    collections: &NexiqReferenceCollections,
) -> Result<Vec<CollectionSchema>, AxonError> {
    Ok(vec![
        nexiq_users_schema(collections)?,
        nexiq_engagements_schema(collections)?,
        nexiq_contracts_schema(collections)?,
        nexiq_tasks_schema(collections)?,
        nexiq_invoices_schema(collections)?,
        nexiq_time_entries_schema(collections)?,
    ])
}

pub fn seed_nexiq_reference_fixture<S: StorageAdapter>(
    handler: &mut AxonHandler<S>,
) -> Result<NexiqReferenceFixture, AxonError> {
    let fixture = nexiq_reference_fixture()?;

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

fn nexiq_users_schema(
    collections: &NexiqReferenceCollections,
) -> Result<CollectionSchema, AxonError> {
    Ok(CollectionSchema {
        collection: collections.users.clone(),
        description: Some("Nexiq reference policy users".into()),
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": ["user_id", "display_name", "user_role", "email", "tailscale_login"],
            "properties": {
                "user_id": { "type": "string" },
                "display_name": { "type": "string" },
                "user_role": {
                    "type": "string",
                    "enum": ["admin", "partner", "consultant", "contractor", "ops_manager"]
                },
                "email": { "type": "string" },
                "tailscale_login": { "type": "string" }
            }
        })),
        link_types: HashMap::new(),
        access_control: Some(nexiq_users_access_control(collections)?),
        gates: HashMap::new(),
        validation_rules: Vec::new(),
        indexes: vec![
            index("user_id", IndexType::String, true),
            index("user_role", IndexType::String, false),
        ],
        compound_indexes: Vec::new(),
        lifecycles: HashMap::new(),
    })
}

fn nexiq_engagements_schema(
    collections: &NexiqReferenceCollections,
) -> Result<CollectionSchema, AxonError> {
    Ok(CollectionSchema {
        collection: collections.engagements.clone(),
        description: Some("Nexiq reference engagements with consultant visibility".into()),
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": [
                "name",
                "status",
                "lead_partner_id",
                "budget_cents",
                "rate_card_id",
                "member_user_ids"
            ],
            "properties": {
                "name": { "type": "string" },
                "status": { "type": "string", "enum": ["draft", "active", "closed"] },
                "lead_partner_id": { "type": "string" },
                "budget_cents": { "type": "integer" },
                "rate_card_id": { "type": "string" },
                "member_user_ids": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "members": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["user_id", "role"],
                        "properties": {
                            "user_id": { "type": "string" },
                            "role": { "type": "string" }
                        }
                    }
                }
            }
        })),
        link_types: HashMap::new(),
        access_control: Some(nexiq_engagements_access_control(collections)?),
        gates: HashMap::new(),
        validation_rules: Vec::new(),
        indexes: vec![
            index("lead_partner_id", IndexType::String, false),
            index("member_user_ids[]", IndexType::String, false),
            index("status", IndexType::String, false),
        ],
        compound_indexes: Vec::new(),
        lifecycles: HashMap::new(),
    })
}

fn nexiq_contracts_schema(
    collections: &NexiqReferenceCollections,
) -> Result<CollectionSchema, AxonError> {
    Ok(CollectionSchema {
        collection: collections.contracts.clone(),
        description: Some("Nexiq reference contracts inheriting engagement visibility".into()),
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": ["title", "engagement_id", "rate_card_entries"],
            "properties": {
                "title": { "type": "string" },
                "engagement_id": { "type": "string" },
                "rate_card_entries": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["role", "rate_cents"],
                        "properties": {
                            "role": { "type": "string" },
                            "rate_cents": { "type": "integer" }
                        }
                    }
                }
            }
        })),
        link_types: HashMap::from([(
            "belongs_to_engagement".into(),
            link_type(collections.engagements.as_str(), Cardinality::ManyToOne),
        )]),
        access_control: Some(nexiq_contracts_access_control(collections)?),
        gates: HashMap::new(),
        validation_rules: Vec::new(),
        indexes: vec![index("engagement_id", IndexType::String, false)],
        compound_indexes: Vec::new(),
        lifecycles: HashMap::new(),
    })
}

fn nexiq_tasks_schema(
    collections: &NexiqReferenceCollections,
) -> Result<CollectionSchema, AxonError> {
    Ok(CollectionSchema {
        collection: collections.tasks.clone(),
        description: Some("Nexiq reference tasks inheriting engagement visibility".into()),
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": ["title", "engagement_id", "status"],
            "properties": {
                "title": { "type": "string" },
                "engagement_id": { "type": "string" },
                "status": { "type": "string", "enum": ["draft", "in_progress", "done"] }
            }
        })),
        link_types: HashMap::from([(
            "belongs_to_engagement".into(),
            link_type(collections.engagements.as_str(), Cardinality::ManyToOne),
        )]),
        access_control: Some(nexiq_tasks_access_control(collections)?),
        gates: HashMap::new(),
        validation_rules: Vec::new(),
        indexes: vec![
            index("engagement_id", IndexType::String, false),
            index("status", IndexType::String, false),
        ],
        compound_indexes: Vec::new(),
        lifecycles: HashMap::new(),
    })
}

fn nexiq_invoices_schema(
    collections: &NexiqReferenceCollections,
) -> Result<CollectionSchema, AxonError> {
    Ok(CollectionSchema {
        collection: collections.invoices.clone(),
        description: Some(
            "Nexiq reference invoices hidden from consultants and contractors".into(),
        ),
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": ["number", "engagement_id", "lead_partner_id", "status", "amount_cents"],
            "properties": {
                "number": { "type": "string" },
                "engagement_id": { "type": "string" },
                "lead_partner_id": { "type": "string" },
                "status": { "type": "string", "enum": ["draft", "submitted", "approved", "paid"] },
                "amount_cents": { "type": "integer" }
            }
        })),
        link_types: HashMap::new(),
        access_control: Some(nexiq_invoices_access_control(collections)?),
        gates: HashMap::new(),
        validation_rules: Vec::new(),
        indexes: vec![
            index("engagement_id", IndexType::String, false),
            index("lead_partner_id", IndexType::String, false),
            index("status", IndexType::String, false),
        ],
        compound_indexes: Vec::new(),
        lifecycles: HashMap::new(),
    })
}

fn nexiq_time_entries_schema(
    collections: &NexiqReferenceCollections,
) -> Result<CollectionSchema, AxonError> {
    Ok(CollectionSchema {
        collection: collections.time_entries.clone(),
        description: Some("Nexiq reference time entries with ops-manager billing access".into()),
        version: 1,
        entity_schema: Some(json!({
            "type": "object",
            "required": ["user_id", "engagement_id", "status", "hours", "rate_cents", "cost_cents"],
            "properties": {
                "user_id": { "type": "string" },
                "engagement_id": { "type": "string" },
                "status": { "type": "string", "enum": ["draft", "submitted", "approved", "invoiced", "paid"] },
                "hours": { "type": "number" },
                "rate_cents": { "type": "integer" },
                "cost_cents": { "type": "integer" }
            }
        })),
        link_types: HashMap::from([(
            "belongs_to_engagement".into(),
            link_type(collections.engagements.as_str(), Cardinality::ManyToOne),
        )]),
        access_control: Some(nexiq_time_entries_access_control(collections)?),
        gates: HashMap::new(),
        validation_rules: Vec::new(),
        indexes: vec![
            index("user_id", IndexType::String, false),
            index("engagement_id", IndexType::String, false),
            index("status", IndexType::String, false),
        ],
        compound_indexes: Vec::new(),
        lifecycles: HashMap::new(),
    })
}

fn nexiq_reference_entities(
    collections: &NexiqReferenceCollections,
    ids: &NexiqReferenceEntityIds,
) -> Vec<FixtureSeedEntity> {
    vec![
        entity(
            &collections.users,
            &ids.admin,
            json!({
                "user_id": ids.admin.as_str(),
                "display_name": "Admin Operator",
                "user_role": "admin",
                "email": "admin@nexiq.example",
                "tailscale_login": "admin@nexiq.example"
            }),
        ),
        entity(
            &collections.users,
            &ids.partner,
            json!({
                "user_id": ids.partner.as_str(),
                "display_name": "Lead Partner",
                "user_role": "partner",
                "email": "partner@nexiq.example",
                "tailscale_login": "partner@nexiq.example"
            }),
        ),
        entity(
            &collections.users,
            &ids.consultant,
            json!({
                "user_id": ids.consultant.as_str(),
                "display_name": "Consultant Alex",
                "user_role": "consultant",
                "email": "alex@nexiq.example",
                "tailscale_login": "alex@nexiq.example"
            }),
        ),
        entity(
            &collections.users,
            &ids.consultant_peer,
            json!({
                "user_id": ids.consultant_peer.as_str(),
                "display_name": "Consultant Brooke",
                "user_role": "consultant",
                "email": "brooke@nexiq.example",
                "tailscale_login": "brooke@nexiq.example"
            }),
        ),
        entity(
            &collections.users,
            &ids.contractor,
            json!({
                "user_id": ids.contractor.as_str(),
                "display_name": "Contractor Casey",
                "user_role": "contractor",
                "email": "casey@nexiq.example",
                "tailscale_login": "casey@nexiq.example"
            }),
        ),
        entity(
            &collections.users,
            &ids.ops_manager,
            json!({
                "user_id": ids.ops_manager.as_str(),
                "display_name": "Ops Manager",
                "user_role": "ops_manager",
                "email": "ops@nexiq.example",
                "tailscale_login": "ops@nexiq.example"
            }),
        ),
        entity(
            &collections.engagements,
            &ids.engagement_alpha,
            json!({
                "name": "Alpha Rollout",
                "status": "active",
                "lead_partner_id": ids.partner.as_str(),
                "budget_cents": 1250000,
                "rate_card_id": "rc-alpha",
                "member_user_ids": [
                    ids.consultant.as_str(),
                    ids.contractor.as_str()
                ],
                "members": [
                    { "user_id": ids.consultant.as_str(), "role": "consultant" },
                    { "user_id": ids.contractor.as_str(), "role": "contractor" }
                ]
            }),
        ),
        entity(
            &collections.engagements,
            &ids.engagement_beta,
            json!({
                "name": "Beta Renewal",
                "status": "active",
                "lead_partner_id": ids.partner.as_str(),
                "budget_cents": 2100000,
                "rate_card_id": "rc-beta",
                "member_user_ids": [
                    ids.consultant_peer.as_str()
                ],
                "members": [
                    { "user_id": ids.consultant_peer.as_str(), "role": "consultant" }
                ]
            }),
        ),
        entity(
            &collections.contracts,
            &ids.contract_alpha,
            json!({
                "title": "Alpha Master Services Agreement",
                "engagement_id": ids.engagement_alpha.as_str(),
                "rate_card_entries": [
                    { "role": "lead_consultant", "rate_cents": 22500 },
                    { "role": "delivery_consultant", "rate_cents": 17500 }
                ]
            }),
        ),
        entity(
            &collections.contracts,
            &ids.contract_beta,
            json!({
                "title": "Beta Support Addendum",
                "engagement_id": ids.engagement_beta.as_str(),
                "rate_card_entries": [
                    { "role": "delivery_consultant", "rate_cents": 19000 }
                ]
            }),
        ),
        entity(
            &collections.tasks,
            &ids.task_alpha,
            json!({
                "title": "Alpha discovery workshop",
                "engagement_id": ids.engagement_alpha.as_str(),
                "status": "in_progress"
            }),
        ),
        entity(
            &collections.tasks,
            &ids.task_beta,
            json!({
                "title": "Beta launch checklist",
                "engagement_id": ids.engagement_beta.as_str(),
                "status": "draft"
            }),
        ),
        entity(
            &collections.invoices,
            &ids.invoice_alpha,
            json!({
                "number": "INV-ALPHA-1001",
                "engagement_id": ids.engagement_alpha.as_str(),
                "lead_partner_id": ids.partner.as_str(),
                "status": "submitted",
                "amount_cents": 480000
            }),
        ),
        entity(
            &collections.time_entries,
            &ids.time_entry_alpha,
            json!({
                "user_id": ids.consultant.as_str(),
                "engagement_id": ids.engagement_alpha.as_str(),
                "status": "submitted",
                "hours": 7.5,
                "rate_cents": 17500,
                "cost_cents": 9000
            }),
        ),
        entity(
            &collections.time_entries,
            &ids.time_entry_beta,
            json!({
                "user_id": ids.consultant_peer.as_str(),
                "engagement_id": ids.engagement_beta.as_str(),
                "status": "draft",
                "hours": 4.0,
                "rate_cents": 19000,
                "cost_cents": 9800
            }),
        ),
    ]
}

fn nexiq_reference_links(
    collections: &NexiqReferenceCollections,
    ids: &NexiqReferenceEntityIds,
) -> Vec<FixtureSeedLink> {
    vec![
        link(
            &collections.contracts,
            &ids.contract_alpha,
            &collections.engagements,
            &ids.engagement_alpha,
            "belongs_to_engagement",
        ),
        link(
            &collections.contracts,
            &ids.contract_beta,
            &collections.engagements,
            &ids.engagement_beta,
            "belongs_to_engagement",
        ),
        link(
            &collections.tasks,
            &ids.task_alpha,
            &collections.engagements,
            &ids.engagement_alpha,
            "belongs_to_engagement",
        ),
        link(
            &collections.tasks,
            &ids.task_beta,
            &collections.engagements,
            &ids.engagement_beta,
            "belongs_to_engagement",
        ),
        link(
            &collections.time_entries,
            &ids.time_entry_alpha,
            &collections.engagements,
            &ids.engagement_alpha,
            "belongs_to_engagement",
        ),
        link(
            &collections.time_entries,
            &ids.time_entry_beta,
            &collections.engagements,
            &ids.engagement_beta,
            "belongs_to_engagement",
        ),
    ]
}

fn nexiq_users_access_control(
    collections: &NexiqReferenceCollections,
) -> Result<AccessControlPolicy, AxonError> {
    access_control(json!({
        "identity": nexiq_identity_json(collections),
        "read": {
            "allow": [
                {
                    "name": "admins-partners-and-ops-read-users",
                    "when": {
                        "subject": "role",
                        "in": ["admin", "partner", "ops_manager", "consultant", "contractor"]
                    }
                }
            ]
        },
        "create": { "allow": [{ "name": "fixture-seed-create" }] },
        "fields": {
            "email": role_redaction_rule(
                "delivery-peers-do-not-see-user-email",
                &["consultant", "contractor"]
            ),
            "tailscale_login": role_redaction_rule(
                "delivery-peers-do-not-see-tailscale-login",
                &["consultant", "contractor"]
            )
        }
    }))
}

fn nexiq_engagements_access_control(
    collections: &NexiqReferenceCollections,
) -> Result<AccessControlPolicy, AxonError> {
    access_control(json!({
        "identity": nexiq_identity_json(collections),
        "read": {
            "allow": [
                {
                    "name": "admins-and-partners-read-engagements",
                    "when": { "subject": "role", "in": ["admin", "partner"] }
                },
                {
                    "name": "assigned-consultants-read-engagements",
                    "when": { "subject": "role", "in": ["consultant", "contractor"] },
                    "where": { "field": "member_user_ids[]", "contains_subject": "user_id" }
                }
            ]
        },
        "create": { "allow": [{ "name": "fixture-seed-create" }] },
        "update": {
            "allow": [
                {
                    "name": "admins-update-engagements",
                    "when": { "subject": "role", "eq": "admin" }
                },
                {
                    "name": "partners-update-led-engagements",
                    "when": { "subject": "role", "eq": "partner" },
                    "where": { "field": "lead_partner_id", "eq_subject": "user_id" }
                },
                {
                    "name": "delivery-team-updates-visible-engagements",
                    "when": { "subject": "role", "in": ["consultant", "contractor"] },
                    "where": { "field": "member_user_ids[]", "contains_subject": "user_id" }
                }
            ]
        },
        "fields": {
            "budget_cents": contractor_redaction_rule("contractors-do-not-see-engagement-budget"),
            "rate_card_id": contractor_redaction_rule("contractors-do-not-see-engagement-rate-card"),
            "status": {
                "write": {
                    "deny": [
                        {
                            "name": "delivery-team-cannot-change-engagement-status",
                            "when": { "subject": "role", "in": ["consultant", "contractor"] }
                        }
                    ]
                }
            },
            "lead_partner_id": {
                "write": {
                    "deny": [
                        {
                            "name": "delivery-team-cannot-reassign-engagements",
                            "when": { "subject": "role", "in": ["consultant", "contractor"] }
                        }
                    ]
                }
            }
        }
    }))
}

fn nexiq_contracts_access_control(
    collections: &NexiqReferenceCollections,
) -> Result<AccessControlPolicy, AxonError> {
    access_control(json!({
        "identity": nexiq_identity_json(collections),
        "read": {
            "allow": [
                {
                    "name": "admins-partners-and-ops-read-contracts",
                    "when": { "subject": "role", "in": ["admin", "partner", "ops_manager"] }
                },
                {
                    "name": "contracts-visible-through-engagement",
                    "where": {
                        "related": {
                            "link_type": "belongs_to_engagement",
                            "direction": "outgoing",
                            "target_collection": collections.engagements.as_str(),
                            "target_policy": "read"
                        }
                    }
                }
            ]
        },
        "create": { "allow": [{ "name": "fixture-seed-create" }] },
        "fields": {
            "rate_card_entries": role_redaction_rule(
                "ops-managers-do-not-see-contract-rate-cards",
                &["ops_manager"]
            )
        }
    }))
}

fn nexiq_tasks_access_control(
    collections: &NexiqReferenceCollections,
) -> Result<AccessControlPolicy, AxonError> {
    access_control(json!({
        "identity": nexiq_identity_json(collections),
        "read": {
            "allow": [
                {
                    "name": "admins-partners-and-ops-read-tasks",
                    "when": { "subject": "role", "in": ["admin", "partner", "ops_manager"] }
                },
                {
                    "name": "tasks-visible-through-engagement",
                    "where": {
                        "related": {
                            "link_type": "belongs_to_engagement",
                            "direction": "outgoing",
                            "target_collection": collections.engagements.as_str(),
                            "target_policy": "read"
                        }
                    }
                }
            ]
        },
        "create": { "allow": [{ "name": "fixture-seed-create" }] }
    }))
}

fn nexiq_invoices_access_control(
    collections: &NexiqReferenceCollections,
) -> Result<AccessControlPolicy, AxonError> {
    access_control(json!({
        "identity": nexiq_identity_json(collections),
        "read": {
            "allow": [
                {
                    "name": "admins-and-ops-read-invoices",
                    "when": { "subject": "role", "in": ["admin", "ops_manager"] }
                },
                {
                    "name": "partners-read-led-invoices",
                    "when": { "subject": "role", "eq": "partner" },
                    "where": { "field": "lead_partner_id", "eq_subject": "user_id" }
                }
            ]
        },
        "create": { "allow": [{ "name": "fixture-seed-create" }] }
    }))
}

fn nexiq_time_entries_access_control(
    collections: &NexiqReferenceCollections,
) -> Result<AccessControlPolicy, AxonError> {
    access_control(json!({
        "identity": nexiq_identity_json(collections),
        "read": {
            "allow": [
                {
                    "name": "admins-read-time-entries",
                    "when": { "subject": "role", "eq": "admin" }
                },
                {
                    "name": "delivery-team-reads-own-time-entries",
                    "when": { "subject": "role", "in": ["consultant", "contractor"] },
                    "where": { "field": "user_id", "eq_subject": "user_id" }
                },
                {
                    "name": "partners-read-engagement-time-entries",
                    "when": { "subject": "role", "eq": "partner" },
                    "where": {
                        "related": {
                            "link_type": "belongs_to_engagement",
                            "direction": "outgoing",
                            "target_collection": collections.engagements.as_str(),
                            "target_policy": "read"
                        }
                    }
                },
                {
                    "name": "ops-managers-read-billing-time-entries",
                    "when": { "subject": "role", "eq": "ops_manager" },
                    "where": { "field": "status", "in": ["submitted", "approved", "invoiced", "paid"] }
                }
            ]
        },
        "create": { "allow": [{ "name": "fixture-seed-create" }] },
        "update": {
            "allow": [
                {
                    "name": "admins-approve-time-entries",
                    "when": { "subject": "role", "eq": "admin" }
                },
                {
                    "name": "partners-approve-engagement-time-entries",
                    "when": { "subject": "role", "eq": "partner" },
                    "where": {
                        "related": {
                            "link_type": "belongs_to_engagement",
                            "direction": "outgoing",
                            "target_collection": collections.engagements.as_str(),
                            "target_policy": "read"
                        }
                    }
                }
            ]
        },
        "fields": {
            "rate_cents": role_redaction_rule(
                "delivery-team-does-not-see-time-rate",
                &["consultant", "contractor"]
            ),
            "cost_cents": role_redaction_rule(
                "delivery-team-does-not-see-time-cost",
                &["consultant", "contractor"]
            )
        }
    }))
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

fn nexiq_identity_json(collections: &NexiqReferenceCollections) -> Value {
    json!({
        "user_id": "subject.user_id",
        "role": "subject.attributes.user_role",
        "attributes": {
            "user_role": {
                "from": "collection",
                "collection": collections.users.as_str(),
                "key_field": "user_id",
                "key_subject": "user_id",
                "value_field": "user_role"
            }
        }
    })
}

fn contractor_redaction_rule(name: &str) -> Value {
    role_redaction_rule(name, &["contractor"])
}

fn role_redaction_rule(name: &str, roles: &[&str]) -> Value {
    json!({
        "read": {
            "deny": [
                {
                    "name": name,
                    "when": { "subject": "role", "in": roles },
                    "redact_as": null
                }
            ]
        }
    })
}

fn access_control(value: Value) -> Result<AccessControlPolicy, AxonError> {
    serde_json::from_value(value).map_err(|err| {
        AxonError::SchemaValidation(format!(
            "invalid test fixture access_control fixture: {err}"
        ))
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

fn entity(collection: &CollectionId, id: &EntityId, data: Value) -> FixtureSeedEntity {
    FixtureSeedEntity {
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
) -> FixtureSeedLink {
    FixtureSeedLink {
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

    #[test]
    fn nexiq_reference_fixture_seeds_policy_graph() {
        let mut handler = AxonHandler::new(MemoryStorageAdapter::default());
        let fixture =
            seed_nexiq_reference_fixture(&mut handler).expect("nexiq fixture should seed");

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

        assert_eq!(count_entities(&handler, &fixture.collections.users), 6);
        assert_eq!(
            count_entities(&handler, &fixture.collections.engagements),
            2
        );
        assert_eq!(count_entities(&handler, &fixture.collections.contracts), 2);
        assert_eq!(count_entities(&handler, &fixture.collections.tasks), 2);
        assert_eq!(count_entities(&handler, &fixture.collections.invoices), 1);
        assert_eq!(
            count_entities(&handler, &fixture.collections.time_entries),
            2
        );

        let stored_links = handler
            .storage_ref()
            .range_scan(&Link::links_collection(), None, None, None)
            .expect("stored links should scan");
        assert_eq!(stored_links.len(), fixture.links.len());

        let catalog = compile_policy_catalog(&fixture.schemas).expect("policies should compile");
        for collection in [
            fixture.collections.engagements.as_str(),
            fixture.collections.contracts.as_str(),
            fixture.collections.tasks.as_str(),
            fixture.collections.invoices.as_str(),
            fixture.collections.time_entries.as_str(),
        ] {
            assert!(catalog.plans.contains_key(collection));
        }
    }

    #[test]
    fn nexiq_reference_fixture_exposes_expected_redaction_targets() {
        let fixture = nexiq_reference_fixture().expect("fixture should build");
        let engagement = fixture
            .entity(
                &fixture.collections.engagements,
                &fixture.ids.engagement_alpha,
            )
            .expect("visible engagement should exist");
        let contract = fixture
            .entity(&fixture.collections.contracts, &fixture.ids.contract_alpha)
            .expect("visible contract should exist");

        assert_eq!(fixture.subjects.consultant, "consultant-alex");
        assert_eq!(fixture.subjects.contractor, "contractor-casey");
        assert_eq!(engagement.data["budget_cents"], json!(1_250_000));
        assert_eq!(engagement.data["rate_card_id"], json!("rc-alpha"));
        assert_eq!(
            contract.data["rate_card_entries"][0]["rate_cents"],
            json!(22_500)
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
