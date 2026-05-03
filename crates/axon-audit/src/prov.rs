use std::collections::HashMap;

use axon_core::id::{CollectionId, EntityId};
use serde_json::{json, Map, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::entry::{AuditAttribution, AuditEntry, MutationIntentAuditMetadata, MutationType};

pub const PROV_NAMESPACE: &str = "http://www.w3.org/ns/prov#";
const XSD_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema#";

const OFFICIAL_PROV_TERMS_USED: &[&str] = &[
    "Activity",
    "Agent",
    "Entity",
    "endedAtTime",
    "startedAtTime",
    "used",
    "wasAssociatedWith",
    "wasGeneratedBy",
    "wasInformedBy",
];

/// Convert audit entries into additive PROV-O JSON-LD.
///
/// The PROV graph carries the standard provenance shape while
/// `axon:nativeAuditEntry` preserves Axon's native audit facts for lossless
/// re-import.
pub fn audit_entries_to_prov_json(entries: &[AuditEntry], tenant: &str, database: &str) -> Value {
    let mut graph = Vec::new();
    let mut previous_activity_by_tx: HashMap<String, String> = HashMap::new();

    for entry in entries {
        let activity_id = activity_iri(tenant, database, entry.id);
        let agent_id = agent_iri(tenant, &entry.actor);
        let entity_url = canonical_entity_url(
            tenant,
            database,
            entry.collection.as_str(),
            entry.entity_id.as_str(),
        );

        let mut activity = Map::new();
        activity.insert("@id".to_string(), Value::String(activity_id.clone()));
        activity.insert(
            "@type".to_string(),
            Value::String("prov:Activity".to_string()),
        );
        activity.insert(
            "prov:startedAtTime".to_string(),
            typed_datetime(entry.timestamp_ns),
        );
        activity.insert(
            "prov:endedAtTime".to_string(),
            typed_datetime(entry.timestamp_ns),
        );
        activity.insert(
            "prov:wasAssociatedWith".to_string(),
            json!({ "@id": agent_id }),
        );
        activity.insert(
            "axon:operation".to_string(),
            Value::String(entry.mutation.to_string()),
        );
        activity.insert("axon:auditId".to_string(), json!(entry.id));
        activity.insert("axon:version".to_string(), json!(entry.version));
        activity.insert(
            "axon:nativeAuditEntry".to_string(),
            audit_entry_to_native_json(entry),
        );

        if entry.data_before.is_some() {
            activity.insert(
                "prov:used".to_string(),
                json!({ "@id": before_snapshot_iri(&entity_url, entry.id) }),
            );
        }
        if let Some(transaction_id) = &entry.transaction_id {
            if let Some(previous_activity) = previous_activity_by_tx.get(transaction_id) {
                activity.insert(
                    "prov:wasInformedBy".to_string(),
                    json!({ "@id": previous_activity }),
                );
            }
            previous_activity_by_tx.insert(transaction_id.clone(), activity_id.clone());
        }
        graph.push(Value::Object(activity));

        graph.push(json!({
            "@id": agent_iri(tenant, &entry.actor),
            "@type": "prov:Agent",
            "axon:actor": entry.actor,
        }));

        if let Some(before) = &entry.data_before {
            graph.push(json!({
                "@id": before_snapshot_iri(&entity_url, entry.id),
                "@type": "prov:Entity",
                "axon:collection": entry.collection.to_string(),
                "axon:entityId": entry.entity_id.to_string(),
                "axon:snapshotRole": "before",
                "axon:state": before,
            }));
        }

        let mut affected = Map::new();
        affected.insert("@id".to_string(), Value::String(entity_url));
        affected.insert(
            "@type".to_string(),
            Value::String("prov:Entity".to_string()),
        );
        affected.insert(
            "axon:collection".to_string(),
            Value::String(entry.collection.to_string()),
        );
        affected.insert(
            "axon:entityId".to_string(),
            Value::String(entry.entity_id.to_string()),
        );
        affected.insert("axon:version".to_string(), json!(entry.version));
        if let Some(after) = &entry.data_after {
            affected.insert("axon:state".to_string(), after.clone());
            affected.insert(
                "prov:wasGeneratedBy".to_string(),
                json!({ "@id": activity_id }),
            );
        }
        graph.push(Value::Object(affected));
    }

    json!({
        "@context": {
            "prov": PROV_NAMESPACE,
            "xsd": XSD_NAMESPACE,
            "axon": format!("/tenants/{tenant}/databases/{database}/vocab#audit-"),
        },
        "@graph": graph,
    })
}

/// Native HTTP audit JSON shape used by `/audit/query`.
pub fn audit_entry_to_native_json(entry: &AuditEntry) -> Value {
    json!({
        "id": entry.id,
        "timestamp_ns": entry.timestamp_ns,
        "collection": entry.collection.to_string(),
        "entity_id": entry.entity_id.to_string(),
        "version": entry.version,
        "mutation": entry.mutation.to_string(),
        "operation": entry.mutation.to_string(),
        "data_before": &entry.data_before,
        "data_after": &entry.data_after,
        "diff": &entry.diff,
        "actor": entry.actor,
        "metadata": &entry.metadata,
        "transaction_id": entry.transaction_id,
        "intent_lineage": &entry.intent_lineage,
    })
}

/// Validate that Axon's PROV-O JSON-LD uses the official PROV namespace and
/// only the official PROV terms required by FEAT-003 US-010.
pub fn validate_prov_o_json(document: &Value) -> Result<(), String> {
    let context = document
        .get("@context")
        .and_then(Value::as_object)
        .ok_or_else(|| "PROV-O document is missing @context".to_string())?;
    match context.get("prov").and_then(Value::as_str) {
        Some(PROV_NAMESPACE) => {}
        Some(other) => return Err(format!("invalid prov namespace: {other}")),
        None => return Err("PROV-O document is missing prov namespace".to_string()),
    }

    let graph = document
        .get("@graph")
        .and_then(Value::as_array)
        .ok_or_else(|| "PROV-O document is missing @graph".to_string())?;
    for node in graph {
        validate_node_prov_terms(node)?;
    }
    Ok(())
}

/// Re-import entries from Axon's additive PROV-O JSON-LD form.
pub fn audit_entries_from_prov_json(document: &Value) -> Result<Vec<AuditEntry>, String> {
    validate_prov_o_json(document)?;
    let graph = document
        .get("@graph")
        .and_then(Value::as_array)
        .ok_or_else(|| "PROV-O document is missing @graph".to_string())?;

    let mut entries = Vec::new();
    for node in graph {
        let Some(obj) = node.as_object() else {
            continue;
        };
        if obj.get("@type").and_then(Value::as_str) != Some("prov:Activity") {
            continue;
        }
        let native = obj
            .get("axon:nativeAuditEntry")
            .ok_or_else(|| "prov:Activity is missing axon:nativeAuditEntry".to_string())?;
        entries.push(audit_entry_from_native_json(native)?);
    }
    entries.sort_by_key(|entry| entry.id);
    Ok(entries)
}

fn validate_node_prov_terms(node: &Value) -> Result<(), String> {
    let obj = node
        .as_object()
        .ok_or_else(|| "PROV-O graph node must be an object".to_string())?;
    if !obj.contains_key("@id") {
        return Err("PROV-O graph node is missing @id".to_string());
    }
    if let Some(type_value) = obj.get("@type").and_then(Value::as_str) {
        validate_prefixed_prov_term(type_value)?;
    }
    for key in obj.keys().filter(|key| key.starts_with("prov:")) {
        validate_prefixed_prov_term(key)?;
    }
    Ok(())
}

fn validate_prefixed_prov_term(term: &str) -> Result<(), String> {
    let Some(local) = term.strip_prefix("prov:") else {
        return Ok(());
    };
    if OFFICIAL_PROV_TERMS_USED.contains(&local) {
        Ok(())
    } else {
        Err(format!("unsupported PROV-O term: {term}"))
    }
}

fn audit_entry_from_native_json(value: &Value) -> Result<AuditEntry, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "native audit entry must be an object".to_string())?;
    let mut entry = AuditEntry {
        id: required_u64(obj, "id")?,
        timestamp_ns: required_u64(obj, "timestamp_ns")?,
        collection: CollectionId::new(required_str(obj, "collection")?),
        entity_id: EntityId::new(required_str(obj, "entity_id")?),
        version: required_u64(obj, "version")?,
        mutation: parse_mutation(
            required_str(obj, "operation").or_else(|_| required_str(obj, "mutation"))?,
        )?,
        data_before: nullable_value(obj, "data_before"),
        data_after: nullable_value(obj, "data_after"),
        diff: optional_deserialize(obj, "diff")?,
        actor: required_str(obj, "actor")?.to_string(),
        metadata: optional_deserialize(obj, "metadata")?.unwrap_or_default(),
        transaction_id: obj
            .get("transaction_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        attribution: optional_deserialize::<AuditAttribution>(obj, "attribution")?,
        intent_lineage: optional_deserialize::<MutationIntentAuditMetadata>(obj, "intent_lineage")?
            .map(Box::new),
    };
    if entry.diff.is_none() {
        if let (Some(before), Some(after)) = (&entry.data_before, &entry.data_after) {
            let diff = crate::entry::compute_diff(before, after);
            if !diff.is_empty() {
                entry.diff = Some(diff);
            }
        }
    }
    Ok(entry)
}

fn optional_deserialize<T: serde::de::DeserializeOwned>(
    obj: &Map<String, Value>,
    key: &str,
) -> Result<Option<T>, String> {
    match obj.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| format!("invalid {key}: {error}")),
    }
}

fn nullable_value(obj: &Map<String, Value>, key: &str) -> Option<Value> {
    obj.get(key).filter(|value| !value.is_null()).cloned()
}

fn required_u64(obj: &Map<String, Value>, key: &str) -> Result<u64, String> {
    obj.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("native audit entry is missing numeric {key}"))
}

fn required_str<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    obj.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("native audit entry is missing string {key}"))
}

fn parse_mutation(value: &str) -> Result<MutationType, String> {
    match value {
        "entity.create" | "entity_create" => Ok(MutationType::EntityCreate),
        "entity.update" | "entity_update" => Ok(MutationType::EntityUpdate),
        "entity.delete" | "entity_delete" => Ok(MutationType::EntityDelete),
        "entity.revert" | "entity_revert" => Ok(MutationType::EntityRevert),
        "link.create" | "link_create" => Ok(MutationType::LinkCreate),
        "link.delete" | "link_delete" => Ok(MutationType::LinkDelete),
        "collection.create" | "collection_create" => Ok(MutationType::CollectionCreate),
        "collection.drop" | "collection_drop" => Ok(MutationType::CollectionDrop),
        "template.create" | "template_create" => Ok(MutationType::TemplateCreate),
        "template.update" | "template_update" => Ok(MutationType::TemplateUpdate),
        "template.delete" | "template_delete" => Ok(MutationType::TemplateDelete),
        "schema.update" | "schema_update" => Ok(MutationType::SchemaUpdate),
        "guardrail_rejection" => Ok(MutationType::GuardrailRejection),
        "mutation_intent.preview" | "intent.preview" | "intent_preview" => {
            Ok(MutationType::IntentPreview)
        }
        "intent.approve" | "intent_approve" => Ok(MutationType::IntentApprove),
        "intent.reject" | "intent_reject" => Ok(MutationType::IntentReject),
        "intent.expire" | "intent_expire" => Ok(MutationType::IntentExpire),
        "intent.commit" | "intent_commit" => Ok(MutationType::IntentCommit),
        other => Err(format!("unknown mutation type: {other}")),
    }
}

fn typed_datetime(timestamp_ns: u64) -> Value {
    let value = match OffsetDateTime::from_unix_timestamp_nanos(i128::from(timestamp_ns)) {
        Ok(datetime) => datetime
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
        Err(_) => "1970-01-01T00:00:00Z".to_string(),
    };
    json!({
        "@value": value,
        "@type": "xsd:dateTime",
    })
}

fn canonical_entity_url(tenant: &str, database: &str, collection: &str, id: &str) -> String {
    format!("/tenants/{tenant}/databases/{database}/collections/{collection}/entities/{id}")
}

fn activity_iri(tenant: &str, database: &str, audit_id: u64) -> String {
    format!("/tenants/{tenant}/databases/{database}/audit/{audit_id}/activity")
}

fn before_snapshot_iri(entity_url: &str, audit_id: u64) -> String {
    format!("{entity_url}#audit-{audit_id}-before")
}

fn agent_iri(tenant: &str, actor: &str) -> String {
    let safe_actor: String = actor
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '@') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    format!("/tenants/{tenant}/agents/{safe_actor}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::id::{CollectionId, EntityId};
    use serde_json::json;

    fn sample_update_entry() -> AuditEntry {
        let mut entry = AuditEntry::new(
            CollectionId::new("tasks"),
            EntityId::new("t-001"),
            2,
            MutationType::EntityUpdate,
            Some(json!({"title": "v1", "done": false})),
            Some(json!({"title": "v2", "done": false})),
            Some("agent-1".to_string()),
        )
        .with_metadata(HashMap::from([(
            "reason".to_string(),
            "scheduled update".to_string(),
        )]));
        entry.id = 42;
        entry.timestamp_ns = 1_712_345_678_123_456_789;
        entry.transaction_id = Some("tx-1".to_string());
        entry
    }

    #[test]
    fn prov_output_uses_official_prov_o_terms_and_canonical_subjects() {
        let entry = sample_update_entry();
        let doc = audit_entries_to_prov_json(&[entry], "acme", "orders");

        validate_prov_o_json(&doc).expect("PROV-O terms should validate");
        assert_eq!(doc["@context"]["prov"], PROV_NAMESPACE);

        let graph = doc["@graph"].as_array().expect("graph array");
        assert!(graph.iter().any(|node| {
            node["@id"] == "/tenants/acme/databases/orders/collections/tasks/entities/t-001"
                && node["@type"] == "prov:Entity"
                && node["prov:wasGeneratedBy"]["@id"]
                    == "/tenants/acme/databases/orders/audit/42/activity"
        }));
        assert!(graph.iter().any(|node| {
            node["@type"] == "prov:Activity"
                && node["prov:used"]["@id"]
                    == "/tenants/acme/databases/orders/collections/tasks/entities/t-001#audit-42-before"
                && node["prov:wasAssociatedWith"]["@id"] == "/tenants/acme/agents/agent-1"
        }));
    }

    #[test]
    fn transaction_entries_emit_was_informed_by_chain() {
        let mut first = sample_update_entry();
        first.id = 1;
        let mut second = sample_update_entry();
        second.id = 2;
        second.entity_id = EntityId::new("t-002");

        let doc = audit_entries_to_prov_json(&[first, second], "acme", "orders");
        validate_prov_o_json(&doc).expect("PROV-O terms should validate");
        let graph = doc["@graph"].as_array().expect("graph array");
        assert!(graph.iter().any(|node| {
            node["@id"] == "/tenants/acme/databases/orders/audit/2/activity"
                && node["prov:wasInformedBy"]["@id"]
                    == "/tenants/acme/databases/orders/audit/1/activity"
        }));
    }

    #[test]
    fn native_json_to_prov_to_reimport_preserves_auditable_facts() {
        let native = audit_entry_to_native_json(&sample_update_entry());
        let entry = audit_entry_from_native_json(&native).expect("native JSON should parse");
        let doc = audit_entries_to_prov_json(&[entry], "acme", "orders");
        let round_tripped =
            audit_entries_from_prov_json(&doc).expect("PROV-O should re-import cleanly");

        assert_eq!(round_tripped.len(), 1);
        assert_eq!(audit_entry_to_native_json(&round_tripped[0]), native);
    }

    #[test]
    fn validator_rejects_non_official_prov_terms() {
        let mut doc = audit_entries_to_prov_json(&[sample_update_entry()], "acme", "orders");
        doc["@graph"][0]["prov:notAProvOTerm"] = json!({"@id": "x"});

        let err = validate_prov_o_json(&doc).expect_err("unknown PROV term should fail");
        assert!(err.contains("prov:notAProvOTerm"));
    }
}
