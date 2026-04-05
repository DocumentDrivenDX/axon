//! PROP-001 — Schema Round-Trip property-based tests.
//!
//! Property: For any valid ESF schema definition, generating a random valid
//! entity and validating it against the schema always succeeds.  Generating a
//! random invalid entity (type mismatch, missing required field) always fails
//! with a structured error.

use proptest::prelude::*;
use serde_json::json;

use crate::schema::EsfDocument;
use crate::validation::validate_entity;

/// Invoice schema used as the reference ESF document for PROP-001.
const INVOICE_ESF: &str = r#"
esf_version: "1.0"
collection: invoices
entity_schema:
  type: object
  required:
    - vendor_id
    - amount
    - status
  properties:
    vendor_id:
      type: string
    amount:
      type: object
      properties:
        value:
          type: number
          minimum: 0
        currency:
          type: string
          enum: [USD, EUR, GBP]
    status:
      type: string
      enum: [draft, submitted, approved, paid, reconciled]
"#;

fn invoice_schema() -> crate::schema::CollectionSchema {
    EsfDocument::parse(INVOICE_ESF)
        .expect("INVOICE_ESF is valid")
        .into_collection_schema()
        .expect("INVOICE_ESF converts to CollectionSchema")
}

proptest! {
    /// PROP-001a: any structurally valid invoice entity always passes validation.
    ///
    /// Generates arbitrary vendor IDs, non-negative amounts, recognised
    /// currencies, and allowed statuses, then asserts `validate_entity` returns
    /// `Ok`.
    #[test]
    fn valid_invoice_entity_always_passes(
        vendor_id in "[a-zA-Z0-9_-]{1,20}",
        amount_value in 0.0_f64..1_000_000.0_f64,
        currency in proptest::sample::select(vec!["USD", "EUR", "GBP"]),
        status in proptest::sample::select(vec![
            "draft", "submitted", "approved", "paid", "reconciled",
        ]),
    ) {
        let schema = invoice_schema();
        let entity = json!({
            "vendor_id": vendor_id,
            "amount": { "value": amount_value, "currency": currency },
            "status": status,
        });
        prop_assert!(
            validate_entity(&schema, &entity).is_ok(),
            "valid invoice entity should pass validation: {entity}"
        );
    }

    /// PROP-001b: an entity missing at least one required field always fails.
    ///
    /// Iterates over all combinations of present/absent required fields,
    /// constraining the input so that at least one field is always missing.
    /// The resulting `Err` must contain at least one structured error.
    #[test]
    fn missing_required_field_always_fails(
        include_vendor_id in proptest::bool::ANY,
        include_amount    in proptest::bool::ANY,
        include_status    in proptest::bool::ANY,
        vendor_id in "[a-zA-Z0-9]{1,10}",
    ) {
        // Guarantee at least one required field is absent.
        prop_assume!(!include_vendor_id || !include_amount || !include_status);

        let schema = invoice_schema();
        let mut obj = serde_json::Map::new();
        if include_vendor_id {
            obj.insert("vendor_id".into(), json!(vendor_id));
        }
        if include_amount {
            obj.insert("amount".into(), json!({"value": 10.0, "currency": "USD"}));
        }
        if include_status {
            obj.insert("status".into(), json!("draft"));
        }

        let result = validate_entity(&schema, &serde_json::Value::Object(obj));
        prop_assert!(
            result.is_err(),
            "entity missing a required field should fail validation"
        );
        let errs = result.unwrap_err();
        prop_assert!(
            !errs.is_empty(),
            "validation error list must not be empty"
        );
    }

    /// PROP-001c: an entity with a `status` value outside the allowed enum
    /// always fails validation.
    ///
    /// Generates status strings that are not members of the allowed enum and
    /// asserts that `validate_entity` returns a non-empty error.
    #[test]
    fn invalid_status_enum_always_fails(
        bad_status in "[a-z]{6,20}",
        vendor_id  in "[a-zA-Z0-9]{1,10}",
    ) {
        prop_assume!(!matches!(
            bad_status.as_str(),
            "draft" | "submitted" | "approved" | "paid" | "reconciled"
        ));

        let schema = invoice_schema();
        let entity = json!({
            "vendor_id": vendor_id,
            "amount": { "value": 10.0, "currency": "USD" },
            "status": bad_status,
        });

        let result = validate_entity(&schema, &entity);
        prop_assert!(
            result.is_err(),
            "unrecognised status '{bad_status}' should fail validation"
        );
    }
}
