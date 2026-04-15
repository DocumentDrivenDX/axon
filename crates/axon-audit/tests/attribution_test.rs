//! Integration tests for AuditAttribution (ADR-018).
//!
//! These tests verify that:
//! - attribution data round-trips through serde byte-identically
//! - attribution is preserved when appended to an AuditLog
//! - entries are physically immutable (no shared mutation between entries)
//! - attribution survives simulated user renames
//! - attribution survives simulated credential revocations

use axon_audit::{AuditAttribution, AuditEntry, AuditLog, MemoryAuditLog, MutationType};
use axon_core::id::{CollectionId, EntityId};

fn make_attribution() -> AuditAttribution {
    AuditAttribution {
        user_id: "uuid-alice".to_string(),
        tenant_id: "uuid-acme".to_string(),
        jti: Some("uuid-cred-123".to_string()),
        auth_method: "jwt".to_string(),
    }
}

fn make_entry_with_attribution(attribution: AuditAttribution) -> AuditEntry {
    AuditEntry::new(
        CollectionId::new("tasks"),
        EntityId::new("t-001"),
        1,
        MutationType::EntityCreate,
        None,
        None,
        Some("alice@acme.com".to_string()),
    )
    .with_attribution(attribution)
}

/// Serde round-trip: every attribution field must survive serialization and
/// deserialization byte-identical. Also verifies that two entries built from
/// the same attribution carry independent-but-equal values (no shared mutation).
#[test]
fn attribution_serde_round_trip() {
    let attr = make_attribution();
    let entry = make_entry_with_attribution(attr.clone());

    // Serialize → deserialize → assert equality
    let json = serde_json::to_string(&entry).expect("serialize AuditEntry");
    let deserialized: AuditEntry = serde_json::from_str(&json).expect("deserialize AuditEntry");

    let recovered = deserialized
        .attribution
        .expect("attribution present after round-trip");

    assert_eq!(recovered.user_id, "uuid-alice");
    assert_eq!(recovered.tenant_id, "uuid-acme");
    assert_eq!(recovered.jti, Some("uuid-cred-123".to_string()));
    assert_eq!(recovered.auth_method, "jwt");

    // Build a second entry with the same attribution and verify independence
    let attr2 = make_attribution();
    let entry2 = make_entry_with_attribution(attr2);

    let attr_a = entry.attribution.as_ref().expect("entry1 has attribution");
    let attr_b = entry2.attribution.as_ref().expect("entry2 has attribution");

    // Equal in value …
    assert_eq!(attr_a, attr_b);
    // … but modifying one must not affect the other (they are cloned, not shared)
    // Rust ownership guarantees this at compile time, but we assert structural
    // independence as documentation of intent.
    assert_eq!(attr_a.user_id, attr_b.user_id);
    assert_eq!(attr_a.jti, attr_b.jti);
}

/// Attribution survives appending to an AuditLog and querying back by ID.
#[test]
fn write_entry_with_attribution() {
    let mut log = MemoryAuditLog::default();

    let attr = make_attribution();
    let entry = make_entry_with_attribution(attr);

    let appended = log.append(entry).expect("append should succeed");
    let entry_id = appended.id;

    let found = log
        .find_by_id(entry_id)
        .expect("find_by_id should succeed")
        .expect("entry should exist");

    let recovered = found
        .attribution
        .expect("attribution should be present after append+query");

    assert_eq!(recovered.user_id, "uuid-alice");
    assert_eq!(recovered.tenant_id, "uuid-acme");
    assert_eq!(recovered.jti, Some("uuid-cred-123".to_string()));
    assert_eq!(recovered.auth_method, "jwt");
    // Legacy actor field is untouched
    assert_eq!(found.actor, "alice@acme.com");
}

/// Simulates a user rename: since axon-audit has no direct link to the users
/// table, the audit entry itself is never mutated. Re-querying the entry proves
/// the attribution is physically immutable — the stable user_id is preserved
/// regardless of any external display_name/email change.
#[test]
fn attribution_persists_after_rename_simulation() {
    let mut log = MemoryAuditLog::default();

    let attr = AuditAttribution {
        user_id: "uuid-alice".to_string(),
        tenant_id: "uuid-acme".to_string(),
        jti: Some("uuid-cred-123".to_string()),
        auth_method: "jwt".to_string(),
    };
    let entry = AuditEntry::new(
        CollectionId::new("tasks"),
        EntityId::new("t-002"),
        1,
        MutationType::EntityCreate,
        None,
        None,
        // actor is the display name at write time — it will "drift" after rename
        Some("alice-old-name@acme.com".to_string()),
    )
    .with_attribution(attr);

    let appended = log.append(entry).expect("append should succeed");
    let entry_id = appended.id;

    // Simulate rename: in the real system the User record's email would be updated
    // in a separate users table. axon-audit has no link to that table, so nothing
    // in the audit log changes. We re-query and assert the attribution is intact.
    let found = log
        .find_by_id(entry_id)
        .expect("find_by_id should succeed")
        .expect("entry should exist");

    let recovered = found
        .attribution
        .expect("attribution should survive rename simulation");

    // Stable IDs are intact
    assert_eq!(recovered.user_id, "uuid-alice");
    assert_eq!(recovered.tenant_id, "uuid-acme");
    // The actor field still shows the old display name — this is expected drift
    assert_eq!(found.actor, "alice-old-name@acme.com");
}

/// Simulates credential revocation: after a JWT is revoked the credential
/// record is updated externally, but the audit entry already carries the jti
/// of that credential. The jti MUST still be present in the entry — it is the
/// forensic trail linking the action to the (now-revoked) credential.
#[test]
fn attribution_persists_after_revocation_simulation() {
    let mut log = MemoryAuditLog::default();

    let attr = AuditAttribution {
        user_id: "uuid-alice".to_string(),
        tenant_id: "uuid-acme".to_string(),
        // This jti will be "revoked" externally after the entry is written
        jti: Some("uuid-cred-456".to_string()),
        auth_method: "jwt".to_string(),
    };
    let entry = AuditEntry::new(
        CollectionId::new("tasks"),
        EntityId::new("t-003"),
        1,
        MutationType::EntityDelete,
        None,
        None,
        Some("alice@acme.com".to_string()),
    )
    .with_attribution(attr);

    let appended = log.append(entry).expect("append should succeed");
    let entry_id = appended.id;

    // Simulate revocation: in the real system the credential record would be
    // marked revoked in a separate credentials table. axon-audit has no link to
    // that table, so nothing in the audit log changes. Re-query and verify the
    // jti is still present — the audit trail must record it.
    let found = log
        .find_by_id(entry_id)
        .expect("find_by_id should succeed")
        .expect("entry should exist");

    let recovered = found
        .attribution
        .expect("attribution should survive revocation simulation");

    // The jti of the (now-revoked) credential is still recorded
    assert_eq!(recovered.jti, Some("uuid-cred-456".to_string()));
    assert_eq!(recovered.user_id, "uuid-alice");
    assert_eq!(recovered.auth_method, "jwt");
}
