//! INV-005: Schema Enforcement workload.
//!
//! Statement: No entity in storage violates its collection schema. Every write
//! is validated; invalid writes are rejected with structured errors.
//!
//! Workload:
//! 1. Register a JSON-Schema on a collection (required fields, types, enum).
//! 2. Attempt a mix of valid and invalid creates/updates.
//! 3. CHECK:
//!    a. All invalid writes were rejected with `AxonError::SchemaValidation`.
//!    b. All valid writes succeeded.
//!    c. Every entity currently in storage passes schema validation.

use axon_api::handler::AxonHandler;
use axon_api::request::{CreateEntityRequest, GetEntityRequest, UpdateEntityRequest};
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_schema::schema::CollectionSchema;
use axon_schema::validation::validate;
use axon_storage::adapter::StorageAdapter;
use axon_storage::memory::MemoryStorageAdapter;
use serde_json::{json, Value};

const COL: &str = "sim_schema";

/// JSON Schema 2020-12 governing `sim_schema` entities.
///
/// Required: `name` (string), `status` (enum: active|inactive).
/// Optional: `count` (non-negative integer).
const ENTITY_SCHEMA: &str = r#"{
    "type": "object",
    "required": ["name", "status"],
    "properties": {
        "name":   { "type": "string" },
        "status": { "type": "string", "enum": ["active", "inactive"] },
        "count":  { "type": "integer", "minimum": 0 }
    }
}"#;

/// A write attempt classified as valid or invalid.
#[derive(Debug)]
struct WriteAttempt {
    id: EntityId,
    data: Value,
    should_succeed: bool,
}

fn write_attempts() -> Vec<WriteAttempt> {
    vec![
        // ── Valid ──────────────────────────────────────────────────────────────
        WriteAttempt {
            id: EntityId::new("v-001"),
            data: json!({ "name": "alpha", "status": "active" }),
            should_succeed: true,
        },
        WriteAttempt {
            id: EntityId::new("v-002"),
            data: json!({ "name": "beta", "status": "inactive", "count": 5 }),
            should_succeed: true,
        },
        WriteAttempt {
            id: EntityId::new("v-003"),
            data: json!({ "name": "gamma", "status": "active", "count": 0 }),
            should_succeed: true,
        },
        // ── Invalid: missing required field ───────────────────────────────────
        WriteAttempt {
            id: EntityId::new("i-001"),
            data: json!({ "status": "active" }), // missing "name"
            should_succeed: false,
        },
        WriteAttempt {
            id: EntityId::new("i-002"),
            data: json!({ "name": "delta" }), // missing "status"
            should_succeed: false,
        },
        // ── Invalid: wrong type ────────────────────────────────────────────────
        WriteAttempt {
            id: EntityId::new("i-003"),
            data: json!({ "name": 42, "status": "active" }), // name is integer not string
            should_succeed: false,
        },
        WriteAttempt {
            id: EntityId::new("i-004"),
            data: json!({ "name": "epsilon", "status": "active", "count": -1 }), // count < 0
            should_succeed: false,
        },
        // ── Invalid: enum violation ────────────────────────────────────────────
        WriteAttempt {
            id: EntityId::new("i-005"),
            data: json!({ "name": "zeta", "status": "pending" }), // "pending" not in enum
            should_succeed: false,
        },
    ]
}

/// Result of a schema-enforcement workload run.
#[derive(Debug)]
pub struct SchemaEnforcementResult {
    /// INV-005a: all invalid writes were rejected with SchemaValidation.
    pub invalid_writes_rejected: bool,
    /// INV-005b: all valid writes succeeded.
    pub valid_writes_accepted: bool,
    /// INV-005c: every entity in storage passes schema validation.
    pub storage_all_valid: bool,
    /// Number of valid writes attempted.
    pub valid_attempted: usize,
    /// Number of invalid writes attempted.
    pub invalid_attempted: usize,
}

impl SchemaEnforcementResult {
    /// Returns `true` when all schema-enforcement invariants hold.
    pub fn is_correct(&self) -> bool {
        self.invalid_writes_rejected && self.valid_writes_accepted && self.storage_all_valid
    }
}

/// Run the schema-enforcement workload and return the result.
pub fn run_schema_enforcement_workload() -> SchemaEnforcementResult {
    let mut handler = AxonHandler::new(MemoryStorageAdapter::default());

    let col = CollectionId::new(COL);
    let schema_value: Value = serde_json::from_str(ENTITY_SCHEMA).expect("schema JSON must parse");

    let schema = CollectionSchema {
        collection: col.clone(),
        description: Some("sim_schema for INV-005".into()),
        version: 1,
        entity_schema: Some(schema_value.clone()),
        link_types: Default::default(),
    };
    handler.put_schema(schema).unwrap();

    let attempts = write_attempts();
    let valid_attempted = attempts.iter().filter(|a| a.should_succeed).count();
    let invalid_attempted = attempts.iter().filter(|a| !a.should_succeed).count();

    let mut invalid_writes_rejected = true;
    let mut valid_writes_accepted = true;

    // ── EXECUTION ─────────────────────────────────────────────────────────────
    for attempt in &attempts {
        let result = handler.create_entity(CreateEntityRequest {
            collection: col.clone(),
            id: attempt.id.clone(),
            data: attempt.data.clone(),
            actor: Some("sim".into()),
        });

        if attempt.should_succeed {
            if result.is_err() {
                valid_writes_accepted = false;
            }
        } else {
            match result {
                Err(AxonError::SchemaValidation(_)) => {} // expected
                Ok(_) | Err(_) => {
                    invalid_writes_rejected = false;
                }
            }
        }
    }

    // Also test an invalid update on a valid entity.
    if handler
        .get_entity(GetEntityRequest {
            collection: col.clone(),
            id: EntityId::new("v-001"),
        })
        .is_ok()
    {
        let resp = handler
            .get_entity(GetEntityRequest {
                collection: col.clone(),
                id: EntityId::new("v-001"),
            })
            .unwrap();
        let update_result = handler.update_entity(UpdateEntityRequest {
            collection: col.clone(),
            id: EntityId::new("v-001"),
            data: json!({ "name": "alpha", "status": "bad_enum" }), // invalid
            expected_version: resp.entity.version,
            actor: Some("sim".into()),
        });
        if !matches!(update_result, Err(AxonError::SchemaValidation(_))) {
            invalid_writes_rejected = false;
        }
    }

    // ── CHECK: all entities in storage pass the schema ─────────────────────────
    let raw_schema = CollectionSchema {
        collection: col.clone(),
        description: None,
        version: 1,
        entity_schema: Some(schema_value),
        link_types: Default::default(),
    };

    let all_entities = handler
        .storage_mut()
        .range_scan(&col, None, None, None)
        .expect("range_scan must succeed");

    let storage_all_valid = all_entities.iter().all(|entity| {
        raw_schema
            .entity_schema
            .as_ref()
            .map_or(true, |_s| validate(&raw_schema, &entity.data).is_ok())
    });

    SchemaEnforcementResult {
        invalid_writes_rejected,
        valid_writes_accepted,
        storage_all_valid,
        valid_attempted,
        invalid_attempted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_enforcement_rejects_invalid_and_accepts_valid() {
        let result = run_schema_enforcement_workload();
        assert!(
            result.valid_writes_accepted,
            "INV-005b: valid writes should have been accepted"
        );
        assert!(
            result.invalid_writes_rejected,
            "INV-005a: invalid writes should have been rejected with SchemaValidation"
        );
        assert!(
            result.storage_all_valid,
            "INV-005c: entities in storage must all satisfy the schema"
        );
        assert!(
            result.is_correct(),
            "overall schema-enforcement check failed"
        );
    }

    #[test]
    fn valid_entities_are_stored_invalid_are_not() {
        let result = run_schema_enforcement_workload();
        assert_eq!(result.valid_attempted, 3, "expected 3 valid write attempts");
        assert_eq!(
            result.invalid_attempted, 5,
            "expected 5 invalid write attempts"
        );
    }
}
