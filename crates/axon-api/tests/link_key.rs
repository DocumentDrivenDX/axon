use std::collections::BTreeSet;

use axon_core::{CollectionId, EntityId, Link, LinkKey};
use axon_storage::{SqliteStorageAdapter, StorageAdapter};
use serde_json::json;

fn link(parts: [&str; 5]) -> Link {
    Link {
        source_collection: CollectionId::new(parts[0]),
        source_id: EntityId::new(parts[1]),
        link_type: parts[2].to_string(),
        target_collection: CollectionId::new(parts[3]),
        target_id: EntityId::new(parts[4]),
        metadata: json!({"vector": parts}),
    }
}

#[test]
fn link_key_property_vectors() {
    let vectors = [
        ["a/b", "c", "d", "e", "f"],
        ["a", "b/c", "d", "e", "f"],
        ["a", "b", "c/d", "e", "f"],
        ["a", "b", "c", "d/e", "f"],
        ["a", "b", "c", "d", "e/f"],
        ["", "", "", "", ""],
        ["集合", "源/一", "关系🔗", "目标", "尾/雪"],
        ["é", "e\u{301}", "/", "🔗", ""],
    ];
    let forward: BTreeSet<_> = vectors
        .iter()
        .map(|parts| LinkKey::forward(&link(*parts)).entity_id())
        .collect();
    let reverse: BTreeSet<_> = vectors
        .iter()
        .map(|parts| LinkKey::reverse(&link(*parts)).entity_id())
        .collect();
    assert_eq!(forward.len(), vectors.len());
    assert_eq!(reverse.len(), vectors.len());
    assert!(forward.is_disjoint(&reverse));
}

#[test]
fn link_key_scans_preserve_duplicates_cardinality_and_round_trips() {
    let mut storage = SqliteStorageAdapter::open_in_memory().expect("sqlite opens");
    let shared_source = CollectionId::new("src/集合");
    let shared_id = EntityId::new("source/一");
    let target = CollectionId::new("target/集合");
    let links = [
        Link {
            source_collection: shared_source.clone(),
            source_id: shared_id.clone(),
            target_collection: target.clone(),
            target_id: EntityId::new("one/1"),
            link_type: "owns/typed".into(),
            metadata: json!({}),
        },
        Link {
            source_collection: shared_source.clone(),
            source_id: shared_id.clone(),
            target_collection: target.clone(),
            target_id: EntityId::new("two/2"),
            link_type: "owns".into(),
            metadata: json!({}),
        },
    ];
    for link in &links {
        storage.put_link(link).expect("typed link stores");
    }

    assert_eq!(
        storage
            .list_outbound_links(&shared_source, &shared_id, None)
            .expect("outbound scan")
            .len(),
        2
    );
    assert_eq!(
        storage
            .list_outbound_links(&shared_source, &shared_id, Some("owns"))
            .expect("cardinality scan")
            .len(),
        1
    );
    for link in &links {
        assert_eq!(
            storage
                .get_link(
                    &link.source_collection,
                    &link.source_id,
                    &link.link_type,
                    &link.target_collection,
                    &link.target_id,
                )
                .expect("exact duplicate lookup"),
            Some(link.clone())
        );
        assert_eq!(
            storage
                .list_inbound_links(
                    &link.target_collection,
                    &link.target_id,
                    Some(&link.link_type)
                )
                .expect("reverse scan"),
            vec![link.clone()]
        );
    }
}
