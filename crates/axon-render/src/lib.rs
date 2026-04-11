#![forbid(unsafe_code)]
//! Template rendering and validation for Axon markdown templates.
//!
//! This crate handles Mustache-based markdown template validation against
//! collection entity schemas.  It is deliberately free of `axon-schema`
//! dependency: field validation receives the schema as a plain
//! `serde_json::Value` so that `axon-api` can bridge the two crates.
//!
//! # Key entry points
//!
//! - [`compile`] — compile a template once for repeated renders.
//! - [`render`] — render an [`axon_core::Entity`] into markdown using a Mustache
//!   template string.
//! - [`render_compiled`] — render an entity with a precompiled template.
//! - [`extract_template_fields`] — extract all `{{field}}` references from a
//!   template string.
//! - [`validate_template`] — check those references against a JSON Schema and
//!   return errors (unknown fields) and warnings (optional fields).

pub mod fields;

use std::collections::HashSet;

use axon_core::{AxonError, Entity};
use ramhorns::{encoding::Encoder, Content, Section, Template};
use serde_json::Value;

pub use fields::extract_field_refs;

/// System-level fields always available in the render context.
///
/// These come from Axon's internal entity metadata, not the user-defined
/// `entity_schema`, so they are unconditionally accepted.
const SYSTEM_FIELDS: &[&str] = &[
    "_id",
    "_version",
    "_created_at",
    "_updated_at",
    "_created_by",
    "_updated_by",
];

#[derive(Clone, Copy)]
struct JsonContent<'a>(&'a Value);

impl JsonContent<'_> {
    fn resolved(&self, name: &str) -> Option<&Value> {
        resolve_path(self.0, name)
    }
}

impl Content for JsonContent<'_> {
    fn is_truthy(&self) -> bool {
        match self.0 {
            Value::Null => false,
            Value::Bool(value) => *value,
            Value::Number(value) => value.as_f64().is_some_and(|number| number != 0.0),
            Value::String(value) => !value.is_empty(),
            Value::Array(values) => !values.is_empty(),
            Value::Object(_) => true,
        }
    }

    fn capacity_hint(&self, _tpl: &Template) -> usize {
        match self.0 {
            Value::String(value) => value.len(),
            Value::Array(values) => values.len() * 8,
            Value::Object(values) => values.len() * 8,
            _ => 8,
        }
    }

    fn render_escaped<E: Encoder>(&self, encoder: &mut E) -> Result<(), E::Error> {
        render_json_value(self.0, true, encoder)
    }

    fn render_unescaped<E: Encoder>(&self, encoder: &mut E) -> Result<(), E::Error> {
        render_json_value(self.0, false, encoder)
    }

    fn render_field_escaped<E: Encoder>(
        &self,
        _hash: u64,
        name: &str,
        encoder: &mut E,
    ) -> Result<bool, E::Error> {
        if let Some(value) = self.resolved(name) {
            JsonContent(value).render_escaped(encoder)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn render_field_unescaped<E: Encoder>(
        &self,
        _hash: u64,
        name: &str,
        encoder: &mut E,
    ) -> Result<bool, E::Error> {
        if let Some(value) = self.resolved(name) {
            JsonContent(value).render_unescaped(encoder)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn render_field_section<C, E>(
        &self,
        _hash: u64,
        name: &str,
        section: Section<C>,
        encoder: &mut E,
    ) -> Result<bool, E::Error>
    where
        C: ramhorns::traits::ContentSequence,
        E: Encoder,
    {
        if let Some(value) = self.resolved(name) {
            render_section_value(value, section, encoder)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn render_field_inverse<C, E>(
        &self,
        _hash: u64,
        name: &str,
        section: Section<C>,
        encoder: &mut E,
    ) -> Result<bool, E::Error>
    where
        C: ramhorns::traits::ContentSequence,
        E: Encoder,
    {
        if let Some(value) = self.resolved(name) {
            JsonContent(value).render_inverse(section, encoder)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

fn render_json_value<E: Encoder>(
    value: &Value,
    escaped: bool,
    encoder: &mut E,
) -> Result<(), E::Error> {
    match value {
        Value::Null => Ok(()),
        Value::Bool(value) => encoder.format_unescaped(*value),
        Value::Number(value) => encoder.format_unescaped(value),
        Value::String(value) => {
            if escaped {
                encoder.write_escaped(value)
            } else {
                encoder.write_unescaped(value)
            }
        }
        Value::Array(_) | Value::Object(_) => {
            if let Ok(serialized) = serde_json::to_string(value) {
                if escaped {
                    encoder.write_escaped(&serialized)
                } else {
                    encoder.write_unescaped(&serialized)
                }
            } else {
                Ok(())
            }
        }
    }
}

fn render_section_value<C, E>(
    value: &Value,
    section: Section<C>,
    encoder: &mut E,
) -> Result<(), E::Error>
where
    C: ramhorns::traits::ContentSequence,
    E: Encoder,
{
    match value {
        Value::Array(values) => {
            for item in values {
                let content = JsonContent(item);
                section.with(&content).render(encoder)?;
            }
            Ok(())
        }
        _ => {
            let content = JsonContent(value);
            content.render_section(section, encoder)
        }
    }
}

fn resolve_path<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    if name == "." {
        return Some(value);
    }

    let mut current = value;

    for segment in name.split('.') {
        match current {
            Value::Object(values) => {
                current = values.get(segment)?;
            }
            _ => return None,
        }
    }

    Some(current)
}

fn build_render_context(entity: &Entity) -> Value {
    let mut context = match &entity.data {
        Value::Object(values) => values.clone(),
        _ => serde_json::Map::new(),
    };

    if !matches!(entity.data, Value::Object(_)) {
        context.insert("data".to_string(), entity.data.clone());
    }

    context.insert("_id".to_string(), Value::String(entity.id.to_string()));
    context.insert("_version".to_string(), Value::from(entity.version));
    context.insert(
        "_created_at".to_string(),
        entity.created_at_ns.map_or(Value::Null, Value::from),
    );
    context.insert(
        "_updated_at".to_string(),
        entity.updated_at_ns.map_or(Value::Null, Value::from),
    );
    context.insert(
        "_created_by".to_string(),
        entity
            .created_by
            .as_ref()
            .map_or(Value::Null, |value| Value::String(value.clone())),
    );
    context.insert(
        "_updated_by".to_string(),
        entity
            .updated_by
            .as_ref()
            .map_or(Value::Null, |value| Value::String(value.clone())),
    );

    Value::Object(context)
}

/// A precompiled markdown template that can be reused across renders.
#[derive(Debug)]
pub struct CompiledTemplate {
    template: Template<'static>,
}

impl CompiledTemplate {
    /// Render `entity` using this compiled template.
    pub fn render(&self, entity: &Entity) -> String {
        let context = build_render_context(entity);
        self.template.render(&JsonContent(&context))
    }
}

/// Compile a Mustache template for repeated markdown renders.
pub fn compile(template: impl Into<String>) -> Result<CompiledTemplate, AxonError> {
    let template = Template::new(template.into()).map_err(|error| {
        AxonError::InvalidArgument(format!("invalid markdown template: {error}"))
    })?;
    Ok(CompiledTemplate { template })
}

/// Render an entity with a precompiled template.
pub fn render_compiled(entity: &Entity, template: &CompiledTemplate) -> String {
    template.render(entity)
}

/// Render an entity to markdown with a Mustache template string.
///
/// Entity fields are available at the template root alongside system fields
/// such as `_id` and `_version`.
pub fn render(entity: &Entity, template: &str) -> Result<String, AxonError> {
    let template = compile(template.to_owned())?;
    Ok(render_compiled(entity, &template))
}

/// An error produced when a template references a field that does not exist in
/// the entity schema.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateFieldError {
    /// The field path as written in the template (e.g. `"foo.bar"`).
    pub field: String,
    /// Human-readable explanation.
    pub message: String,
}

impl std::fmt::Display for TemplateFieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// An advisory warning produced when a template references a field that exists
/// in the schema but is not listed as `required`.
///
/// The template is still accepted; callers may surface the warning to the user.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateFieldWarning {
    /// The field path as written in the template.
    pub field: String,
    /// Human-readable advisory.
    pub message: String,
}

impl std::fmt::Display for TemplateFieldWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Combined result of template validation.
///
/// - `errors` is non-empty when the template must be rejected (unknown fields).
/// - `warnings` is non-empty when the template references optional fields.
///   The template is still valid; the warning is advisory only.
#[derive(Debug, Clone, Default)]
pub struct TemplateValidationResult {
    /// Fields that do not exist in the schema.  Non-empty → reject the template.
    pub errors: Vec<TemplateFieldError>,
    /// Fields that exist but are not required.  Advisory only.
    pub warnings: Vec<TemplateFieldWarning>,
}

impl TemplateValidationResult {
    /// Returns `true` when there are no errors (template may be saved).
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Extract the top-level field name from a dot-path.
///
/// `"amount.value"` → `"amount"`.
fn top_level_field(path: &str) -> &str {
    path.split('.').next().unwrap_or(path)
}

/// Validate all `{{field}}` references in `template` against `entity_schema`.
///
/// # Behaviour
///
/// - If `entity_schema` is `None`, field validation is skipped (any reference
///   is accepted because the collection has no structural constraints).
/// - System fields (`_id`, `_version`, etc.) are always accepted.
/// - A field whose top-level name appears in `entity_schema.properties` is
///   accepted.  If the top-level name is **not** in the schema's `required`
///   array an advisory warning is added.
/// - A field whose top-level name is **not** in `entity_schema.properties` at
///   all produces an error.
///
/// # Returns
///
/// A [`TemplateValidationResult`] containing zero or more errors and warnings.
/// Call [`TemplateValidationResult::is_valid`] to decide whether to accept the
/// save.
pub fn validate_template(
    template: &str,
    entity_schema: Option<&Value>,
) -> TemplateValidationResult {
    let mut result = TemplateValidationResult::default();

    let field_refs = extract_field_refs(template);

    let Some(schema) = entity_schema else {
        // No schema → accept everything.
        return result;
    };

    // Collect property names from the schema.
    let properties: HashSet<String> = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default();

    // Collect required field names.
    let required: HashSet<String> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    // Sort for deterministic output ordering.
    let mut sorted_refs: Vec<String> = field_refs.into_iter().collect();
    sorted_refs.sort();

    for field in sorted_refs {
        let top = top_level_field(&field);

        // System fields are always present.
        if SYSTEM_FIELDS.contains(&top) {
            continue;
        }

        if !properties.contains(top) {
            result.errors.push(TemplateFieldError {
                message: format!(
                    "template references field '{}' which does not exist in the entity schema",
                    field
                ),
                field,
            });
        } else if !required.contains(top) {
            result.warnings.push(TemplateFieldWarning {
                message: format!(
                    "field '{}' is optional — template output may be incomplete for entities \
                     without this field",
                    field
                ),
                field,
            });
        }
    }

    result
}

/// Convenience wrapper: extract field references as a sorted `Vec<String>`.
///
/// Useful when callers only need the list without running validation.
pub fn extract_template_fields(template: &str) -> Vec<String> {
    let mut refs: Vec<String> = extract_field_refs(template).into_iter().collect();
    refs.sort();
    refs
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::{CollectionId, EntityId};
    use serde_json::json;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn schema_with_required(required: &[&str], optional: &[&str]) -> Value {
        let mut props = serde_json::Map::new();
        for f in required.iter().chain(optional.iter()) {
            props.insert((*f).to_string(), json!({"type": "string"}));
        }
        json!({
            "type": "object",
            "required": required,
            "properties": props,
        })
    }

    fn sample_entity(data: Value) -> Entity {
        let mut entity = Entity::new(CollectionId::new("tasks"), EntityId::new("task-001"), data);
        entity.version = 7;
        entity.created_at_ns = Some(111);
        entity.updated_at_ns = Some(222);
        entity.created_by = Some("creator".to_string());
        entity.updated_by = Some("editor".to_string());
        entity
    }

    // ── acceptance criteria ───────────────────────────────────────────────────

    /// AC-1: Saving a template referencing a nonexistent field returns a
    /// validation error.
    #[test]
    fn nonexistent_field_produces_error() {
        let schema = schema_with_required(&["title"], &[]);
        let result = validate_template("{{title}} {{ghost}}", Some(&schema));
        assert!(!result.is_valid());
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].field, "ghost");
        assert!(
            result.errors[0].message.contains("ghost"),
            "error message should name the invalid field"
        );
    }

    /// AC-2: Saving a template referencing an optional field succeeds with a
    /// warning.
    #[test]
    fn optional_field_produces_warning_not_error() {
        let schema = schema_with_required(&["title"], &["notes"]);
        let result = validate_template("{{title}} {{notes}}", Some(&schema));
        assert!(result.is_valid(), "optional field must not block save");
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].field, "notes");
        assert!(result.warnings[0].message.contains("optional"));
    }

    /// AC-3a: Field extraction covers {{field}} syntax.
    #[test]
    fn extraction_scalar_field() {
        let fields = extract_template_fields("{{name}}");
        assert_eq!(fields, vec!["name"]);
    }

    /// AC-3b: Field extraction covers {{nested.field}} syntax.
    #[test]
    fn extraction_nested_field() {
        let fields = extract_template_fields("{{amount.value}}");
        assert_eq!(fields, vec!["amount.value"]);
    }

    /// AC-3c: Field extraction covers {{#each array}} / {{#array}} syntax.
    #[test]
    fn extraction_section_array() {
        let fields = extract_template_fields("{{#line_items}}{{description}}{{/line_items}}");
        assert!(fields.contains(&"line_items".to_string()));
        assert!(fields.contains(&"description".to_string()));
    }

    // ── additional coverage ───────────────────────────────────────────────────

    #[test]
    fn system_fields_always_accepted() {
        let schema = schema_with_required(&["title"], &[]);
        let template = "{{_id}} {{_version}} {{_created_at}} {{title}}";
        let result = validate_template(template, Some(&schema));
        assert!(result.is_valid());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn no_schema_accepts_all_fields() {
        let result = validate_template("{{anything}} {{goes}}", None);
        assert!(result.is_valid());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn multiple_errors_reported() {
        let schema = schema_with_required(&["title"], &[]);
        let result = validate_template("{{title}} {{bad1}} {{bad2}}", Some(&schema));
        assert!(!result.is_valid());
        assert_eq!(result.errors.len(), 2);
        let error_fields: Vec<&str> = result.errors.iter().map(|e| e.field.as_str()).collect();
        assert!(error_fields.contains(&"bad1"));
        assert!(error_fields.contains(&"bad2"));
    }

    #[test]
    fn nested_path_validates_top_level() {
        // amount.value is valid if "amount" is in the schema
        let schema = json!({
            "type": "object",
            "required": ["amount"],
            "properties": {
                "amount": {"type": "object"},
            }
        });
        let result = validate_template("{{amount.value}} {{amount.currency}}", Some(&schema));
        assert!(result.is_valid());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn nested_path_top_level_missing_produces_error() {
        let schema = schema_with_required(&["title"], &[]);
        let result = validate_template("{{missing.child}}", Some(&schema));
        assert!(!result.is_valid());
        assert_eq!(result.errors[0].field, "missing.child");
    }

    #[test]
    fn unescaped_triple_mustache_validated() {
        let schema = schema_with_required(&["title"], &["body"]);
        let result = validate_template("{{{body}}}", Some(&schema));
        // body is optional → warning, no error
        assert!(result.is_valid());
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].field, "body");
    }

    #[test]
    fn static_template_no_refs_is_valid() {
        let schema = schema_with_required(&["title"], &[]);
        let result = validate_template("# Static content only", Some(&schema));
        assert!(result.is_valid());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn invoice_template_example() {
        // Full example from the FEAT-026 spec
        let schema = json!({
            "type": "object",
            "required": ["invoice_number", "vendor", "status", "amount", "line_items"],
            "properties": {
                "invoice_number": {"type": "string"},
                "vendor": {"type": "string"},
                "status": {"type": "string"},
                "amount": {"type": "object"},
                "line_items": {"type": "array"},
                "notes": {"type": "string"},
                "approver": {"type": "string"},
            }
        });

        let template = "# Invoice {{invoice_number}}\n\
                        **Vendor:** {{vendor}}\n\
                        **Status:** {{status}}\n\
                        **Amount:** {{amount.currency}} {{amount.value}}\n\
                        {{#approver}}**Approved by:** {{approver}}{{/approver}}\n\
                        {{#line_items}}- {{description}}{{/line_items}}\n\
                        {{#notes}}{{{notes}}}{{/notes}}";

        let result = validate_template(template, Some(&schema));

        // notes and approver are optional → warnings, not errors
        let warn_fields: Vec<&str> = result.warnings.iter().map(|w| w.field.as_str()).collect();
        assert!(warn_fields.contains(&"approver"), "approver should warn");
        assert!(warn_fields.contains(&"notes"), "notes should warn");

        // `description` is referenced bare inside {{#line_items}} but is not a
        // top-level property on the collection schema.  Top-level validation
        // flags it as unknown.  Template authors must qualify it or move it to
        // the schema.  This is intentional and documented behaviour.
        assert!(
            result.errors.iter().any(|e| e.field == "description"),
            "description (not in top-level schema) should produce an error"
        );

        // All explicitly-required top-level fields and system fields are clean.
        let error_fields: Vec<&str> = result.errors.iter().map(|e| e.field.as_str()).collect();
        for required in &["invoice_number", "vendor", "status", "amount", "line_items"] {
            assert!(
                !error_fields.contains(required),
                "required field '{}' must not produce an error",
                required
            );
        }
    }

    #[test]
    fn render_interpolates_scalar_fields() {
        let entity = sample_entity(json!({
            "title": "Fix release",
            "status": "open",
        }));

        let rendered =
            render(&entity, "# {{title}}\n\nStatus: {{status}}\n").expect("template should render");

        assert_eq!(rendered, "# Fix release\n\nStatus: open");
    }

    #[test]
    fn compiled_template_renders_repeatedly() {
        let template = compile("# {{title}}").expect("template should compile");
        let entity = sample_entity(json!({
            "title": "Compiled",
        }));

        assert_eq!(render_compiled(&entity, &template), "# Compiled");
        assert_eq!(template.render(&entity), "# Compiled");
    }

    #[test]
    fn render_supports_nested_objects() {
        let entity = sample_entity(json!({
            "customer": {
                "name": "Acme Corp",
                "tier": "gold",
            }
        }));

        let rendered = render(&entity, "{{customer.name}} ({{customer.tier}})")
            .expect("template should render");

        assert_eq!(rendered, "Acme Corp (gold)");
    }

    #[test]
    fn render_supports_arrays() {
        let entity = sample_entity(json!({
            "line_items": [
                {"description": "Widget", "qty": 2},
                {"description": "Cable", "qty": 1}
            ]
        }));

        let rendered = render(
            &entity,
            "{{#line_items}}- {{description}} x{{qty}}\n{{/line_items}}",
        )
        .expect("template should render");

        assert_eq!(rendered, "- Widget x2\n- Cable x1\n");
    }

    #[test]
    fn render_missing_optional_fields_as_empty_and_falsey() {
        let entity = sample_entity(json!({
            "title": "Fix release",
        }));

        let rendered = render(&entity, "{{title}}|{{notes}}|{{^notes}}missing{{/notes}}")
            .expect("template should render");

        assert_eq!(rendered, "Fix release||missing");
    }

    #[test]
    fn render_exposes_system_fields() {
        let entity = sample_entity(json!({
            "title": "Fix release",
        }));

        let rendered = render(
            &entity,
            "{{_id}}|{{_version}}|{{_created_by}}|{{_updated_by}}",
        )
        .expect("template should render");

        assert_eq!(rendered, "task-001|7|creator|editor");
    }

    #[test]
    fn render_rejects_invalid_template_syntax() {
        let entity = sample_entity(json!({
            "title": "Fix release",
        }));

        let error =
            render(&entity, "{{title").expect_err("invalid template should return an error");

        assert!(matches!(error, AxonError::InvalidArgument(_)));
    }

    #[test]
    fn compile_rejects_invalid_template_syntax() {
        let error = compile("{{title").expect_err("invalid template should return an error");

        assert!(matches!(error, AxonError::InvalidArgument(_)));
    }
}
