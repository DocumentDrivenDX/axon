//! Tests for DatabaseRouter: (tenant_id, database_name) -> storage adapter handle.
//!
//! Covers caching, cross-tenant isolation, eviction, and factory error propagation.

#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axon_core::auth::TenantId;
use axon_core::error::AxonError;
use axon_core::id::{CollectionId, EntityId};
use axon_core::types::Entity;
use axon_storage::{MemoryStorageAdapter, StorageAdapter};
use axon_server::database_router::{DatabaseAdapterFactory, DatabaseRouter, MemoryAdapterFactory};
use serde_json::json;

// ── Test 1: resolve_same_pair_returns_same_arc ───────────────────────────────

#[test]
fn resolve_same_pair_returns_same_arc() {
    let factory = Arc::new(MemoryAdapterFactory) as Arc<dyn DatabaseAdapterFactory + Send + Sync>;
    let router = DatabaseRouter::new(factory);
    let tenant = TenantId::new("t-aaa");

    let h1 = router.resolve(tenant.clone(), "orders").unwrap();
    let h2 = router.resolve(tenant.clone(), "orders").unwrap();

    assert!(
        Arc::ptr_eq(&h1, &h2),
        "same (tenant, database) pair should return the same Arc"
    );
}

// ── Test 2: different_pairs_return_different_arcs ────────────────────────────

#[test]
fn different_pairs_return_different_arcs() {
    let factory = Arc::new(MemoryAdapterFactory) as Arc<dyn DatabaseAdapterFactory + Send + Sync>;
    let router = DatabaseRouter::new(factory);
    let tenant_a = TenantId::new("t-aaa");
    let tenant_b = TenantId::new("t-bbb");

    let h_a = router.resolve(tenant_a, "orders").unwrap();
    let h_b = router.resolve(tenant_b, "orders").unwrap();

    assert!(
        !Arc::ptr_eq(&h_a, &h_b),
        "different tenants with the same database name should get different adapters"
    );
}

// ── Test 3: evict_drops_cache ────────────────────────────────────────────────

#[test]
fn evict_drops_cache() {
    let factory = Arc::new(MemoryAdapterFactory) as Arc<dyn DatabaseAdapterFactory + Send + Sync>;
    let router = DatabaseRouter::new(factory);
    let tenant = TenantId::new("t-ccc");

    let handle_1 = router.resolve(tenant.clone(), "orders").unwrap();
    router.evict(tenant.clone(), "orders");
    let handle_2 = router.resolve(tenant.clone(), "orders").unwrap();

    assert!(
        !Arc::ptr_eq(&handle_1, &handle_2),
        "after eviction, resolve should return a fresh adapter"
    );
}

// ── Test 4: cross_tenant_isolation ───────────────────────────────────────────
//
// A StorageAdapter wrapper backed by Arc<Mutex<MemoryStorageAdapter>>.
// This lets the test hold a concrete reference to the inner adapter for
// writing, while the router caches the same underlying storage behind a
// Arc<dyn StorageAdapter>.
struct LockableAdapter(Arc<Mutex<MemoryStorageAdapter>>);

impl LockableAdapter {
    fn new() -> Self {
        LockableAdapter(Arc::new(Mutex::new(MemoryStorageAdapter::default())))
    }

    /// Return a cloned Arc to the inner Mutex, allowing the test to write
    /// directly to the underlying MemoryStorageAdapter.
    fn shared(&self) -> Arc<Mutex<MemoryStorageAdapter>> {
        Arc::clone(&self.0)
    }
}

impl StorageAdapter for LockableAdapter {
    fn get(
        &self,
        collection: &CollectionId,
        id: &EntityId,
    ) -> Result<Option<Entity>, AxonError> {
        self.0.lock().unwrap().get(collection, id)
    }

    fn put(&mut self, entity: Entity) -> Result<(), AxonError> {
        self.0.lock().unwrap().put(entity)
    }

    fn delete(
        &mut self,
        collection: &CollectionId,
        id: &EntityId,
    ) -> Result<(), AxonError> {
        self.0.lock().unwrap().delete(collection, id)
    }

    fn count(&self, collection: &CollectionId) -> Result<usize, AxonError> {
        self.0.lock().unwrap().count(collection)
    }

    fn range_scan(
        &self,
        collection: &CollectionId,
        start: Option<&EntityId>,
        end: Option<&EntityId>,
        limit: Option<usize>,
    ) -> Result<Vec<Entity>, AxonError> {
        self.0.lock().unwrap().range_scan(collection, start, end, limit)
    }

    fn compare_and_swap(
        &mut self,
        entity: Entity,
        expected_version: u64,
    ) -> Result<Entity, AxonError> {
        self.0.lock().unwrap().compare_and_swap(entity, expected_version)
    }

    fn create_if_absent(
        &mut self,
        entity: Entity,
        expected_absent_version: u64,
    ) -> Result<Entity, AxonError> {
        self.0.lock().unwrap().create_if_absent(entity, expected_absent_version)
    }
}

/// Factory used by cross_tenant_isolation to track the Arc<Mutex<MemoryStorageAdapter>>
/// behind each created LockableAdapter so the test can write to them directly.
#[allow(clippy::type_complexity)]
struct IsolationFactory {
    adapters: Mutex<HashMap<(TenantId, String), Arc<Mutex<MemoryStorageAdapter>>>>,
}

impl IsolationFactory {
    fn new() -> Self {
        IsolationFactory {
            adapters: Mutex::new(HashMap::new()),
        }
    }

    fn get_shared(
        &self,
        tenant_id: &TenantId,
        database_name: &str,
    ) -> Option<Arc<Mutex<MemoryStorageAdapter>>> {
        self.adapters
            .lock()
            .unwrap()
            .get(&(tenant_id.clone(), database_name.to_string()))
            .cloned()
    }
}

impl DatabaseAdapterFactory for IsolationFactory {
    fn create_adapter(
        &self,
        tenant_id: TenantId,
        database_name: &str,
    ) -> Result<Arc<dyn StorageAdapter + Send + Sync>, AxonError> {
        let adapter = LockableAdapter::new();
        let shared = adapter.shared();
        self.adapters
            .lock()
            .unwrap()
            .insert((tenant_id, database_name.to_string()), shared);
        Ok(Arc::new(adapter))
    }
}

#[test]
fn cross_tenant_isolation() {
    let factory = Arc::new(IsolationFactory::new());
    let dyn_factory: Arc<dyn DatabaseAdapterFactory + Send + Sync> = factory.clone();
    let router = DatabaseRouter::new(dyn_factory);

    let tenant_a = TenantId::new("t-iso-a");
    let tenant_b = TenantId::new("t-iso-b");
    let collection = CollectionId::new("orders");
    let entity_id = EntityId::new("order-001");

    // Populate the cache for both tenants.
    router.resolve(tenant_a.clone(), "orders").unwrap();
    router.resolve(tenant_b.clone(), "orders").unwrap();

    // Write to tenant_a's adapter through the factory's shared reference.
    let inner_a = factory
        .get_shared(&tenant_a, "orders")
        .expect("adapter for tenant_a should exist");
    let entity = Entity::new(
        collection.clone(),
        entity_id.clone(),
        json!({"status": "open"}),
    );
    inner_a.lock().unwrap().put(entity).unwrap();

    // Reading via tenant_b's adapter must return None — no cross-tenant leak.
    let arc_b = router.resolve(tenant_b, "orders").unwrap();
    let result = arc_b.get(&collection, &entity_id).unwrap();
    assert!(
        result.is_none(),
        "entity written to tenant_a's adapter must not appear in tenant_b's adapter"
    );
}

// ── Test 5: factory_error_propagates ─────────────────────────────────────────

struct FailingFactory;

impl DatabaseAdapterFactory for FailingFactory {
    fn create_adapter(
        &self,
        _tenant_id: TenantId,
        _database_name: &str,
    ) -> Result<Arc<dyn StorageAdapter + Send + Sync>, AxonError> {
        Err(AxonError::Storage("factory failed".into()))
    }
}

#[test]
fn factory_error_propagates() {
    let factory = Arc::new(FailingFactory) as Arc<dyn DatabaseAdapterFactory + Send + Sync>;
    let router = DatabaseRouter::new(factory);

    let result = router.resolve(TenantId::new("t-fail"), "db");
    assert!(result.is_err(), "factory error should propagate through resolve");
    // unwrap_err() requires T: Debug; use .err().unwrap() instead since
    // Arc<dyn StorageAdapter + Send + Sync> is not Debug.
    match result.err().unwrap() {
        AxonError::Storage(msg) => assert_eq!(msg, "factory failed"),
        other => panic!("unexpected error: {other:?}"),
    }
}
