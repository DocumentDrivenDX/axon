//! Schema evolution: compatibility detection and field-level diffing.
//!
//! When a schema is updated, the system classifies the change as compatible,
//! breaking, or metadata-only, and produces a structured diff showing which
//! fields were added, removed, or modified.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Classification of a schema change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compatibility {
    /// All existing entities remain valid under the new schema.
    Compatible,
    /// Some existing entities may be invalid under the new schema.
    Breaking,
    /// No entity validation impact (description change, etc.).
    MetadataOnly,
}

/// A single field-level change between two schema versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldChange {
    /// Dot-separated field path (e.g., "status", "amount.currency").
    pub path: String,
    /// What kind of change occurred.
    pub kind: FieldChangeKind,
    /// Human-readable description of the change.
    pub description: String,
}

/// The kind of change to a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldChangeKind {
    /// Field was added.
    Added,
    /// Field was removed.
    Removed,
    /// Field type or constraints changed.
    Modified,
    /// Field moved from optional to required.
    MadeRequired,
    /// Field moved from required to optional.
    MadeOptional,
    /// Enum values added (widened).
    EnumWidened,
    /// Enum values removed (narrowed).
    EnumNarrowed,
    /// Constraint tightened (e.g., minLength increased).
    ConstraintTightened,
    /// Constraint relaxed (e.g., minLength decreased).
    ConstraintRelaxed,
}

/// Result of comparing two schema versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaDiff {
    /// Overall compatibility classification.
    pub compatibility: Compatibility,
    /// Individual field-level changes.
    pub changes: Vec<FieldChange>,
}

/// Compare two entity schemas (JSON Schema documents) and produce a diff.
///
/// If both schemas are `None`, the result is `MetadataOnly` with no changes.
/// If only one is `None`, all fields in the other are added/removed.
pub fn diff_schemas(old: Option<&Value>, new: Option<&Value>) -> SchemaDiff {
    match (old, new) {
        (None, None) => SchemaDiff {
            compatibility: Compatibility::MetadataOnly,
            changes: vec![],
        },
        (None, Some(new_schema)) => {
            let mut changes = vec![];
            collect_field_additions(new_schema, "", &mut changes);
            SchemaDiff {
                compatibility: if changes.is_empty() {
                    Compatibility::MetadataOnly
                } else {
                    Compatibility::Compatible
                },
                changes,
            }
        }
        (Some(_), None) => SchemaDiff {
            compatibility: Compatibility::Breaking,
            changes: vec![FieldChange {
                path: "(root)".to_string(),
                kind: FieldChangeKind::Removed,
                description: "Entity schema removed entirely".to_string(),
            }],
        },
        (Some(old_schema), Some(new_schema)) => diff_object_schemas(old_schema, new_schema),
    }
}

/// Classify a schema change based on the diff.
pub fn classify(diff: &SchemaDiff) -> Compatibility {
    diff.compatibility.clone()
}

fn diff_object_schemas(old: &Value, new: &Value) -> SchemaDiff {
    let mut changes = vec![];
    let mut has_breaking = false;

    // Compare required fields
    let old_required = extract_required(old);
    let new_required = extract_required(new);

    // New required fields that weren't required before → breaking
    for field in new_required.difference(&old_required) {
        let was_property = has_property(old, field);
        let (kind, desc) = if was_property {
            (
                FieldChangeKind::MadeRequired,
                format!("Field '{field}' changed from optional to required"),
            )
        } else {
            (
                FieldChangeKind::Added,
                format!("Required field '{field}' added"),
            )
        };
        changes.push(FieldChange {
            path: field.clone(),
            kind,
            description: desc,
        });
        has_breaking = true;
    }

    // Previously required fields that are no longer required → compatible
    for field in old_required.difference(&new_required) {
        if has_property(new, field) {
            changes.push(FieldChange {
                path: field.clone(),
                kind: FieldChangeKind::MadeOptional,
                description: format!("Field '{field}' changed from required to optional"),
            });
        } else {
            changes.push(FieldChange {
                path: field.clone(),
                kind: FieldChangeKind::Removed,
                description: format!("Required field '{field}' removed"),
            });
            has_breaking = true;
        }
    }

    // Compare properties
    let old_props = extract_properties(old);
    let new_props = extract_properties(new);

    let old_names: BTreeSet<&str> = old_props.keys().copied().collect();
    let new_names: BTreeSet<&str> = new_props.keys().copied().collect();

    // Added fields (not already handled as new required)
    for name in new_names.difference(&old_names) {
        if !changes.iter().any(|c| c.path == *name) {
            changes.push(FieldChange {
                path: (*name).to_string(),
                kind: FieldChangeKind::Added,
                description: format!("Optional field '{name}' added"),
            });
        }
    }

    // Removed fields (not already handled as removed required)
    for name in old_names.difference(&new_names) {
        if !changes.iter().any(|c| c.path == *name) {
            changes.push(FieldChange {
                path: (*name).to_string(),
                kind: FieldChangeKind::Removed,
                description: format!("Field '{name}' removed"),
            });
            has_breaking = true;
        }
    }

    // Modified fields (present in both)
    for name in old_names.intersection(&new_names) {
        let old_def = &old_props[name];
        let new_def = &new_props[name];
        diff_field(name, old_def, new_def, &mut changes, &mut has_breaking);
    }

    let compatibility = if changes.is_empty() {
        Compatibility::MetadataOnly
    } else if has_breaking {
        Compatibility::Breaking
    } else {
        Compatibility::Compatible
    };

    SchemaDiff {
        compatibility,
        changes,
    }
}

fn diff_field(
    path: &str,
    old: &Value,
    new: &Value,
    changes: &mut Vec<FieldChange>,
    has_breaking: &mut bool,
) {
    // Type change
    if old.get("type") != new.get("type") {
        changes.push(FieldChange {
            path: path.to_string(),
            kind: FieldChangeKind::Modified,
            description: format!(
                "Field '{path}' type changed from {} to {}",
                old.get("type").unwrap_or(&Value::Null),
                new.get("type").unwrap_or(&Value::Null),
            ),
        });
        *has_breaking = true;
        return;
    }

    // Enum changes
    let old_enum = extract_enum(old);
    let new_enum = extract_enum(new);
    if let (Some(old_vals), Some(new_vals)) = (&old_enum, &new_enum) {
        let added: Vec<_> = new_vals.difference(old_vals).collect();
        let removed: Vec<_> = old_vals.difference(new_vals).collect();
        if !removed.is_empty() {
            changes.push(FieldChange {
                path: path.to_string(),
                kind: FieldChangeKind::EnumNarrowed,
                description: format!("Field '{path}' enum values removed: {:?}", removed),
            });
            *has_breaking = true;
        }
        if !added.is_empty() && removed.is_empty() {
            changes.push(FieldChange {
                path: path.to_string(),
                kind: FieldChangeKind::EnumWidened,
                description: format!("Field '{path}' enum values added: {:?}", added),
            });
        }
    } else if old_enum.is_some() != new_enum.is_some() {
        // Enum added or removed
        if old_enum.is_some() {
            changes.push(FieldChange {
                path: path.to_string(),
                kind: FieldChangeKind::ConstraintRelaxed,
                description: format!("Field '{path}' enum constraint removed"),
            });
        } else {
            changes.push(FieldChange {
                path: path.to_string(),
                kind: FieldChangeKind::ConstraintTightened,
                description: format!("Field '{path}' enum constraint added"),
            });
            *has_breaking = true;
        }
    }

    // Numeric constraint changes (minLength, maxLength, minimum, maximum)
    for constraint in &["minLength", "maxLength", "minimum", "maximum"] {
        let old_val = old.get(constraint);
        let new_val = new.get(constraint);
        if old_val != new_val {
            let is_tightened = match *constraint {
                "minLength" | "minimum" => {
                    // Increasing min is tightening
                    new_val
                        .and_then(|v| v.as_f64())
                        .unwrap_or(f64::NEG_INFINITY)
                        > old_val
                            .and_then(|v| v.as_f64())
                            .unwrap_or(f64::NEG_INFINITY)
                }
                "maxLength" | "maximum" => {
                    // Decreasing max is tightening
                    new_val.and_then(|v| v.as_f64()).unwrap_or(f64::INFINITY)
                        < old_val.and_then(|v| v.as_f64()).unwrap_or(f64::INFINITY)
                }
                _ => false,
            };

            if is_tightened {
                changes.push(FieldChange {
                    path: path.to_string(),
                    kind: FieldChangeKind::ConstraintTightened,
                    description: format!(
                        "Field '{path}' {constraint} tightened: {:?} → {:?}",
                        old_val, new_val,
                    ),
                });
                *has_breaking = true;
            } else {
                changes.push(FieldChange {
                    path: path.to_string(),
                    kind: FieldChangeKind::ConstraintRelaxed,
                    description: format!(
                        "Field '{path}' {constraint} relaxed: {:?} → {:?}",
                        old_val, new_val,
                    ),
                });
            }
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn extract_required(schema: &Value) -> BTreeSet<String> {
    schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn has_property(schema: &Value, name: &str) -> bool {
    schema
        .get("properties")
        .and_then(|v| v.as_object())
        .is_some_and(|obj| obj.contains_key(name))
}

fn extract_properties(schema: &Value) -> std::collections::BTreeMap<&str, &Value> {
    schema
        .get("properties")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.as_str(), v)).collect())
        .unwrap_or_default()
}

fn extract_enum(field_def: &Value) -> Option<BTreeSet<String>> {
    field_def.get("enum").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    })
}

fn collect_field_additions(schema: &Value, prefix: &str, changes: &mut Vec<FieldChange>) {
    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        let required = extract_required(schema);
        for (name, _) in props {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}.{name}")
            };
            let is_required = required.contains(name.as_str());
            changes.push(FieldChange {
                path,
                kind: FieldChangeKind::Added,
                description: format!(
                    "{} field '{name}' added",
                    if is_required { "Required" } else { "Optional" }
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema_v1() -> Value {
        json!({
            "type": "object",
            "required": ["title", "status"],
            "properties": {
                "title": { "type": "string", "minLength": 1 },
                "status": { "type": "string", "enum": ["draft", "active", "done"] },
                "priority": { "type": "integer", "minimum": 0, "maximum": 4 },
                "description": { "type": "string" }
            }
        })
    }

    // ── Compatible changes ──────────────────────────────────────────────

    #[test]
    fn adding_optional_field_is_compatible() {
        let old = schema_v1();
        let mut new = schema_v1();
        new["properties"]["tags"] = json!({"type": "array", "items": {"type": "string"}});

        let diff = diff_schemas(Some(&old), Some(&new));
        assert_eq!(diff.compatibility, Compatibility::Compatible);
        assert!(diff
            .changes
            .iter()
            .any(|c| c.path == "tags" && c.kind == FieldChangeKind::Added));
    }

    #[test]
    fn widening_enum_is_compatible() {
        let old = schema_v1();
        let mut new = schema_v1();
        new["properties"]["status"]["enum"] = json!(["draft", "active", "done", "archived"]);

        let diff = diff_schemas(Some(&old), Some(&new));
        assert_eq!(diff.compatibility, Compatibility::Compatible);
        assert!(diff
            .changes
            .iter()
            .any(|c| c.path == "status" && c.kind == FieldChangeKind::EnumWidened));
    }

    #[test]
    fn relaxing_constraint_is_compatible() {
        let old = schema_v1();
        let mut new = schema_v1();
        // Decrease minimum from 0 to -1 → relaxing
        new["properties"]["priority"]["minimum"] = json!(-1);

        let diff = diff_schemas(Some(&old), Some(&new));
        assert_eq!(diff.compatibility, Compatibility::Compatible);
        assert!(diff
            .changes
            .iter()
            .any(|c| c.path == "priority" && c.kind == FieldChangeKind::ConstraintRelaxed));
    }

    #[test]
    fn making_required_field_optional_is_compatible() {
        let old = schema_v1();
        let mut new = schema_v1();
        new["required"] = json!(["title"]); // "status" no longer required

        let diff = diff_schemas(Some(&old), Some(&new));
        assert_eq!(diff.compatibility, Compatibility::Compatible);
        assert!(diff
            .changes
            .iter()
            .any(|c| c.path == "status" && c.kind == FieldChangeKind::MadeOptional));
    }

    // ── Breaking changes ────────────────────────────────────────────────

    #[test]
    fn adding_required_field_is_breaking() {
        let old = schema_v1();
        let mut new = schema_v1();
        new["required"] = json!(["title", "status", "assignee"]);
        new["properties"]["assignee"] = json!({"type": "string"});

        let diff = diff_schemas(Some(&old), Some(&new));
        assert_eq!(diff.compatibility, Compatibility::Breaking);
        assert!(diff.changes.iter().any(|c| c.path == "assignee"));
    }

    #[test]
    fn removing_field_is_breaking() {
        let old = schema_v1();
        let mut new = schema_v1();
        new["properties"]
            .as_object_mut()
            .expect("schema fixture should expose properties as an object")
            .remove("description");

        let diff = diff_schemas(Some(&old), Some(&new));
        assert_eq!(diff.compatibility, Compatibility::Breaking);
        assert!(diff
            .changes
            .iter()
            .any(|c| c.path == "description" && c.kind == FieldChangeKind::Removed));
    }

    #[test]
    fn narrowing_enum_is_breaking() {
        let old = schema_v1();
        let mut new = schema_v1();
        new["properties"]["status"]["enum"] = json!(["draft", "done"]); // removed "active"

        let diff = diff_schemas(Some(&old), Some(&new));
        assert_eq!(diff.compatibility, Compatibility::Breaking);
        assert!(diff
            .changes
            .iter()
            .any(|c| c.path == "status" && c.kind == FieldChangeKind::EnumNarrowed));
    }

    #[test]
    fn tightening_constraint_is_breaking() {
        let old = schema_v1();
        let mut new = schema_v1();
        new["properties"]["title"]["minLength"] = json!(5); // was 1

        let diff = diff_schemas(Some(&old), Some(&new));
        assert_eq!(diff.compatibility, Compatibility::Breaking);
        assert!(diff
            .changes
            .iter()
            .any(|c| c.path == "title" && c.kind == FieldChangeKind::ConstraintTightened));
    }

    #[test]
    fn changing_field_type_is_breaking() {
        let old = schema_v1();
        let mut new = schema_v1();
        new["properties"]["priority"]["type"] = json!("string"); // was integer

        let diff = diff_schemas(Some(&old), Some(&new));
        assert_eq!(diff.compatibility, Compatibility::Breaking);
        assert!(diff
            .changes
            .iter()
            .any(|c| c.path == "priority" && c.kind == FieldChangeKind::Modified));
    }

    #[test]
    fn making_optional_field_required_is_breaking() {
        let old = schema_v1();
        let mut new = schema_v1();
        new["required"] = json!(["title", "status", "description"]);

        let diff = diff_schemas(Some(&old), Some(&new));
        assert_eq!(diff.compatibility, Compatibility::Breaking);
        assert!(diff
            .changes
            .iter()
            .any(|c| c.path == "description" && c.kind == FieldChangeKind::MadeRequired));
    }

    #[test]
    fn removing_schema_entirely_is_breaking() {
        let old = schema_v1();
        let diff = diff_schemas(Some(&old), None);
        assert_eq!(diff.compatibility, Compatibility::Breaking);
    }

    // ── Metadata-only ───────────────────────────────────────────────────

    #[test]
    fn identical_schemas_is_metadata_only() {
        let schema = schema_v1();
        let diff = diff_schemas(Some(&schema), Some(&schema));
        assert_eq!(diff.compatibility, Compatibility::MetadataOnly);
        assert!(diff.changes.is_empty());
    }

    #[test]
    fn both_none_is_metadata_only() {
        let diff = diff_schemas(None, None);
        assert_eq!(diff.compatibility, Compatibility::MetadataOnly);
    }

    // ── Adding schema to schemaless ─────────────────────────────────────

    #[test]
    fn adding_schema_to_schemaless_is_compatible() {
        let new = schema_v1();
        let diff = diff_schemas(None, Some(&new));
        assert_eq!(diff.compatibility, Compatibility::Compatible);
        assert!(!diff.changes.is_empty());
    }
}
