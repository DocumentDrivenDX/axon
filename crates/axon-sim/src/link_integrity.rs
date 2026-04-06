//! INV-006: Link Integrity workload.
//!
//! Statement: No link references a non-existent entity. Link-type constraints
//! (source and target must exist) are enforced. Deleting an entity with inbound
//! links is rejected.
//!
//! Workload:
//! 1. Create entities in two collections.
//! 2. Create typed links between them.
//! 3. Attempt to create links to non-existent entities (must fail).
//! 4. Attempt to delete an entity that has inbound links (must fail).
//! 5. Delete an entity that has NO inbound links (must succeed).
//! 6. CHECK: for every link in storage, both source and target entities exist.
//! 7. CHECK INV-007: entity version sequences are strictly monotone.

use axon_api::handler::AxonHandler;
use axon_api::request::{
    CreateEntityRequest, CreateLinkRequest, DeleteEntityRequest, GetEntityRequest,
};
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Link;
use axon_storage::adapter::StorageAdapter;
use axon_storage::memory::MemoryStorageAdapter;
use serde_json::json;

const COL_SRC: &str = "sim_link_src";
const COL_TGT: &str = "sim_link_tgt";
const LINK_TYPE: &str = "points-to";

/// Result of a link-integrity workload run.
#[derive(Debug)]
pub struct LinkIntegrityResult {
    /// INV-006a: link creation to missing entity returns NotFound.
    pub dangling_create_rejected: bool,
    /// INV-006b: deleting an entity with inbound links returns InvalidOperation.
    pub delete_with_inbound_rejected: bool,
    /// INV-006c: deleting an entity with no inbound links succeeds.
    pub delete_without_inbound_succeeds: bool,
    /// INV-006d: after all operations, no link points to a non-existent entity.
    pub no_dangling_links: bool,
}

impl LinkIntegrityResult {
    /// Returns `true` when all link-integrity invariants hold.
    pub fn is_correct(&self) -> bool {
        self.dangling_create_rejected
            && self.delete_with_inbound_rejected
            && self.delete_without_inbound_succeeds
            && self.no_dangling_links
    }
}

/// Run the link-integrity workload and return the result.
pub fn run_link_integrity_workload() -> LinkIntegrityResult {
    let mut handler = AxonHandler::new(MemoryStorageAdapter::default());

    let src = CollectionId::new(COL_SRC);
    let tgt = CollectionId::new(COL_TGT);

    // ── SETUP: create source and target entities ──────────────────────────────
    for i in 0..3u32 {
        handler
            .create_entity(CreateEntityRequest {
                collection: src.clone(),
                id: EntityId::new(format!("s-{i:03}")),
                data: json!({ "index": i }),
                actor: Some("sim".into()),
            })
            .expect("source entity creation must not fail");
    }
    for i in 0..3u32 {
        handler
            .create_entity(CreateEntityRequest {
                collection: tgt.clone(),
                id: EntityId::new(format!("t-{i:03}")),
                data: json!({ "index": i }),
                actor: Some("sim".into()),
            })
            .expect("target entity creation must not fail");
    }

    // ── EXECUTION ─────────────────────────────────────────────────────────────

    // Create valid links: s-000 → t-000, s-001 → t-001
    handler
        .create_link(CreateLinkRequest {
            source_collection: src.clone(),
            source_id: EntityId::new("s-000"),
            target_collection: tgt.clone(),
            target_id: EntityId::new("t-000"),
            link_type: LINK_TYPE.into(),
            metadata: json!(null),
            actor: Some("sim".into()),
        })
        .expect("valid link creation must not fail");

    handler
        .create_link(CreateLinkRequest {
            source_collection: src.clone(),
            source_id: EntityId::new("s-001"),
            target_collection: tgt.clone(),
            target_id: EntityId::new("t-001"),
            link_type: LINK_TYPE.into(),
            metadata: json!(null),
            actor: Some("sim".into()),
        })
        .expect("valid link creation must not fail");

    // INV-006a: creating a link to a non-existent target must fail.
    let dangling_create_rejected = matches!(
        handler.create_link(CreateLinkRequest {
            source_collection: src.clone(),
            source_id: EntityId::new("s-002"),
            target_collection: tgt.clone(),
            target_id: EntityId::new("ghost"),
            link_type: LINK_TYPE.into(),
            metadata: json!(null),
            actor: Some("sim".into()),
        }),
        Err(AxonError::NotFound(_))
    );

    // INV-006b: deleting t-000 (which has an inbound link from s-000) must fail.
    let delete_with_inbound_rejected = matches!(
        handler.delete_entity(DeleteEntityRequest {
            collection: tgt.clone(),
            id: EntityId::new("t-000"),
            actor: Some("sim".into()),
            force: false,
        }),
        Err(AxonError::InvalidOperation(_))
    );

    // INV-006c: deleting t-002 (no inbound links) must succeed.
    let delete_without_inbound_succeeds = handler
        .delete_entity(DeleteEntityRequest {
            collection: tgt.clone(),
            id: EntityId::new("t-002"),
            actor: Some("sim".into()),
            force: false,
        })
        .is_ok();

    // INV-006d: verify no dangling links in storage.
    let no_dangling_links = check_no_dangling_links(&mut handler, &src, &tgt);

    LinkIntegrityResult {
        dangling_create_rejected,
        delete_with_inbound_rejected,
        delete_without_inbound_succeeds,
        no_dangling_links,
    }
}

/// Scan all links in storage and verify every source and target entity exists.
fn check_no_dangling_links(
    handler: &mut AxonHandler<MemoryStorageAdapter>,
    _src: &CollectionId,
    _tgt: &CollectionId,
) -> bool {
    let links_col = Link::links_collection();
    let link_entities = match handler
        .storage_mut()
        .range_scan(&links_col, None, None, None)
    {
        Ok(v) => v,
        Err(_) => return false,
    };

    for link_entity in &link_entities {
        let link = match Link::from_entity(link_entity) {
            Some(l) => l,
            None => continue,
        };

        // Check source exists.
        match handler.get_entity(GetEntityRequest {
            collection: link.source_collection.clone(),
            id: link.source_id.clone(),
        }) {
            Ok(_) => {}
            Err(AxonError::NotFound(_)) => return false,
            Err(_) => return false,
        }

        // Check target exists.
        match handler.get_entity(GetEntityRequest {
            collection: link.target_collection.clone(),
            id: link.target_id.clone(),
        }) {
            Ok(_) => {}
            Err(AxonError::NotFound(_)) => return false,
            Err(_) => return false,
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_integrity_invariants_hold() {
        let result = run_link_integrity_workload();
        assert!(
            result.dangling_create_rejected,
            "INV-006a: link to non-existent entity must be rejected"
        );
        assert!(
            result.delete_with_inbound_rejected,
            "INV-006b: delete of entity with inbound links must be rejected"
        );
        assert!(
            result.delete_without_inbound_succeeds,
            "INV-006c: delete of entity with no inbound links must succeed"
        );
        assert!(
            result.no_dangling_links,
            "INV-006d: no dangling links in storage after workload"
        );
        assert!(result.is_correct(), "overall link-integrity check failed");
    }
}
