//! GraphQL subscription support for change feeds (US-050, FEAT-015).
//!
//! Provides a change feed broker that distributes entity change events
//! to GraphQL WebSocket subscribers. Backed by the audit log.
//!
//! Subscriptions can filter by collection and/or specific fields.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axon_core::id::CollectionId;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// A change event emitted to subscribers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEvent {
    /// The ID of the audit entry that produced this event.
    ///
    /// Clients use this as a resume point for `since_audit_id` on reconnect.
    /// Must be populated on both replayed and live events.
    pub audit_id: String,
    /// The collection that changed.
    pub collection: String,
    /// The entity ID that changed.
    pub entity_id: String,
    /// The operation: "create", "update", or "delete".
    pub operation: String,
    /// Entity data after the change (None for deletes).
    pub data: Option<serde_json::Value>,
    /// Entity data before the change (None for creates).
    pub previous_data: Option<serde_json::Value>,
    /// Entity version after the change.
    pub version: u64,
    /// Timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Actor who made the change.
    pub actor: String,
}

/// Subscription filter criteria.
#[derive(Debug, Clone, Default)]
pub struct SubscriptionFilter {
    /// If set, only receive events for this collection.
    pub collection: Option<CollectionId>,
    /// If set, only receive events that touch these fields.
    pub fields: Vec<String>,
}

impl SubscriptionFilter {
    /// Check if an event matches this filter.
    pub fn matches(&self, event: &ChangeEvent) -> bool {
        if let Some(col) = &self.collection {
            if event.collection != col.as_str() {
                return false;
            }
        }

        if !self.fields.is_empty() {
            // Check if any of the filtered fields are present in the data.
            if let Some(data) = &event.data {
                if !self.fields.iter().any(|f| data.get(f).is_some()) {
                    return false;
                }
            }
        }

        true
    }
}

/// Unique identifier for a subscription.
pub type SubscriptionId = u64;

/// A registered subscriber.
struct Subscriber {
    filter: SubscriptionFilter,
    /// Buffered events waiting to be consumed.
    events: Vec<ChangeEvent>,
    /// Whether this subscription has been closed.
    closed: bool,
}

/// Change feed broker that distributes events to subscribers.
///
/// Thread-safe via internal `Mutex`. Subscribers register with a filter
/// and poll for events. When a collection is dropped, all subscriptions
/// for that collection are closed.
#[derive(Default, Clone)]
pub struct ChangeFeedBroker {
    inner: Arc<Mutex<BrokerInner>>,
}

#[derive(Default)]
struct BrokerInner {
    next_id: SubscriptionId,
    subscribers: HashMap<SubscriptionId, Subscriber>,
}

impl ChangeFeedBroker {
    /// Create a new broker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe to change events with the given filter.
    ///
    /// Returns a subscription ID that can be used to poll for events
    /// or unsubscribe.
    pub fn subscribe(&self, filter: SubscriptionFilter) -> SubscriptionId {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.next_id += 1;
        let id = inner.next_id;
        inner.subscribers.insert(
            id,
            Subscriber {
                filter,
                events: Vec::new(),
                closed: false,
            },
        );
        id
    }

    /// Unsubscribe and remove a subscription.
    pub fn unsubscribe(&self, id: SubscriptionId) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.subscribers.remove(&id);
    }

    /// Publish a change event to all matching subscribers.
    pub fn publish(&self, event: ChangeEvent) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for sub in inner.subscribers.values_mut() {
            if !sub.closed && sub.filter.matches(&event) {
                sub.events.push(event.clone());
            }
        }
    }

    /// Poll for events on a subscription.
    ///
    /// Returns all buffered events and clears the buffer.
    /// Returns `None` if the subscription has been closed or doesn't exist.
    pub fn poll(&self, id: SubscriptionId) -> Option<Vec<ChangeEvent>> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let sub = inner.subscribers.get_mut(&id)?;
        if sub.closed {
            return None;
        }
        let events = std::mem::take(&mut sub.events);
        Some(events)
    }

    /// Close all subscriptions for a collection (e.g., when collection is dropped).
    pub fn close_collection(&self, collection: &CollectionId) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for sub in inner.subscribers.values_mut() {
            if let Some(col) = &sub.filter.collection {
                if col == collection {
                    sub.closed = true;
                }
            }
        }
    }

    /// Number of active (non-closed) subscriptions.
    pub fn active_count(&self) -> usize {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.subscribers.values().filter(|s| !s.closed).count()
    }

    /// Check if a subscription is still active.
    pub fn is_active(&self, id: SubscriptionId) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.subscribers.get(&id).is_some_and(|s| !s.closed)
    }
}

// -- Broadcast-based broker for async subscriptions --------------------------

/// Default capacity for the broadcast channel.
const DEFAULT_BROADCAST_CAPACITY: usize = 256;

/// Async broadcast broker for GraphQL subscriptions.
///
/// Wraps a `tokio::sync::broadcast` channel so that each subscriber gets an
/// independent `tokio_stream::wrappers::BroadcastStream`. Suitable for use
/// inside `SubscriptionFieldFuture` resolvers which need an async `Stream`.
#[derive(Clone)]
pub struct BroadcastBroker {
    sender: broadcast::Sender<ChangeEvent>,
}

impl Default for BroadcastBroker {
    fn default() -> Self {
        Self::new(DEFAULT_BROADCAST_CAPACITY)
    }
}

impl BroadcastBroker {
    /// Create a new broadcast broker with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish a change event to all active subscribers.
    ///
    /// Returns `Ok(receiver_count)` on success, or `Err(event)` if there
    /// are currently no active receivers (the event is lost).
    #[allow(clippy::result_large_err)]
    pub fn publish(&self, event: ChangeEvent) -> Result<usize, ChangeEvent> {
        self.sender.send(event).map_err(|e| e.0)
    }

    /// Subscribe and get a `broadcast::Receiver` for change events.
    pub fn subscribe(&self) -> broadcast::Receiver<ChangeEvent> {
        self.sender.subscribe()
    }

    /// Number of active receivers on the broadcast channel.
    pub fn receiver_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_event(collection: &str, entity_id: &str, op: &str) -> ChangeEvent {
        ChangeEvent {
            audit_id: format!("audit-{entity_id}"),
            collection: collection.into(),
            entity_id: entity_id.into(),
            operation: op.into(),
            data: Some(json!({"title": "test"})),
            previous_data: None,
            version: 1,
            timestamp_ms: 1000,
            actor: "agent".into(),
        }
    }

    #[test]
    fn subscribe_and_receive_events() {
        let broker = ChangeFeedBroker::new();
        let id = broker.subscribe(SubscriptionFilter::default());

        broker.publish(make_event("tasks", "t-001", "create"));

        let events = broker
            .poll(id)
            .expect("active subscription should return a poll buffer");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].entity_id, "t-001");
    }

    #[test]
    fn poll_clears_buffer() {
        let broker = ChangeFeedBroker::new();
        let id = broker.subscribe(SubscriptionFilter::default());

        broker.publish(make_event("tasks", "t-001", "create"));
        let events = broker
            .poll(id)
            .expect("active subscription should return a poll buffer");
        assert_eq!(events.len(), 1);

        // Second poll returns empty.
        let events = broker
            .poll(id)
            .expect("active subscription should return a poll buffer");
        assert!(events.is_empty());
    }

    #[test]
    fn filter_by_collection() {
        let broker = ChangeFeedBroker::new();
        let id = broker.subscribe(SubscriptionFilter {
            collection: Some(CollectionId::new("tasks")),
            ..SubscriptionFilter::default()
        });

        broker.publish(make_event("tasks", "t-001", "create"));
        broker.publish(make_event("users", "u-001", "create"));

        let events = broker
            .poll(id)
            .expect("active subscription should return a poll buffer");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].collection, "tasks");
    }

    #[test]
    fn filter_by_fields() {
        let broker = ChangeFeedBroker::new();
        let id = broker.subscribe(SubscriptionFilter {
            fields: vec!["title".into()],
            ..SubscriptionFilter::default()
        });

        broker.publish(ChangeEvent {
            data: Some(json!({"title": "yes"})),
            ..make_event("tasks", "t-001", "create")
        });
        broker.publish(ChangeEvent {
            data: Some(json!({"other": "no"})),
            ..make_event("tasks", "t-002", "create")
        });

        let events = broker
            .poll(id)
            .expect("active subscription should return a poll buffer");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].entity_id, "t-001");
    }

    #[test]
    fn unsubscribe_removes_subscription() {
        let broker = ChangeFeedBroker::new();
        let id = broker.subscribe(SubscriptionFilter::default());
        broker.unsubscribe(id);

        assert!(broker.poll(id).is_none());
    }

    #[test]
    fn close_collection_closes_matching_subscriptions() {
        let broker = ChangeFeedBroker::new();
        let id1 = broker.subscribe(SubscriptionFilter {
            collection: Some(CollectionId::new("tasks")),
            ..SubscriptionFilter::default()
        });
        let id2 = broker.subscribe(SubscriptionFilter {
            collection: Some(CollectionId::new("users")),
            ..SubscriptionFilter::default()
        });

        broker.close_collection(&CollectionId::new("tasks"));

        assert!(!broker.is_active(id1));
        assert!(broker.is_active(id2));
        assert!(broker.poll(id1).is_none());
    }

    #[test]
    fn active_count_tracks_subscriptions() {
        let broker = ChangeFeedBroker::new();
        assert_eq!(broker.active_count(), 0);

        let id1 = broker.subscribe(SubscriptionFilter::default());
        let _id2 = broker.subscribe(SubscriptionFilter::default());
        assert_eq!(broker.active_count(), 2);

        broker.unsubscribe(id1);
        assert_eq!(broker.active_count(), 1);
    }

    #[test]
    fn multiple_subscribers_receive_same_event() {
        let broker = ChangeFeedBroker::new();
        let id1 = broker.subscribe(SubscriptionFilter::default());
        let id2 = broker.subscribe(SubscriptionFilter::default());

        broker.publish(make_event("tasks", "t-001", "create"));

        assert_eq!(
            broker
                .poll(id1)
                .expect("first active subscription should receive the event")
                .len(),
            1
        );
        assert_eq!(
            broker
                .poll(id2)
                .expect("second active subscription should receive the event")
                .len(),
            1
        );
    }

    #[test]
    fn change_event_serialization() {
        let event = make_event("tasks", "t-001", "update");
        let json = serde_json::to_string(&event).expect("change event should serialize");
        let parsed: ChangeEvent =
            serde_json::from_str(&json).expect("change event JSON should deserialize");
        assert_eq!(parsed.collection, "tasks");
        assert_eq!(parsed.operation, "update");
        assert_eq!(parsed.audit_id, "audit-t-001");
    }

    #[test]
    fn audit_id_serializes_in_websocket_payload() {
        let event = make_event("tasks", "t-001", "create");
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            json.contains("\"audit_id\":\"audit-t-001\""),
            "expected audit_id in serialized payload, got: {json}"
        );
    }

    #[test]
    fn published_event_carries_audit_id_to_subscriber() {
        let broker = ChangeFeedBroker::new();
        let id = broker.subscribe(SubscriptionFilter::default());

        let mut event = make_event("tasks", "t-001", "create");
        event.audit_id = "audit-42".into();
        broker.publish(event);

        let events = broker.poll(id).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].audit_id, "audit-42");
    }

    // -- BroadcastBroker tests -----------------------------------------------

    #[tokio::test]
    async fn broadcast_broker_publish_and_receive() {
        let broker = BroadcastBroker::default();
        let mut rx = broker.subscribe();

        let event = make_event("tasks", "t-001", "create");
        let sent = broker
            .publish(event.clone())
            .expect("publish should succeed with active receiver");
        assert_eq!(sent, 1);

        let received = rx.recv().await.expect("should receive event");
        assert_eq!(received.entity_id, "t-001");
    }

    #[tokio::test]
    async fn broadcast_broker_multiple_receivers() {
        let broker = BroadcastBroker::default();
        let mut rx1 = broker.subscribe();
        let mut rx2 = broker.subscribe();

        assert_eq!(broker.receiver_count(), 2);

        broker
            .publish(make_event("tasks", "t-001", "create"))
            .expect("publish should succeed");

        let e1 = rx1.recv().await.expect("rx1 should receive event");
        let e2 = rx2.recv().await.expect("rx2 should receive event");
        assert_eq!(e1.entity_id, "t-001");
        assert_eq!(e2.entity_id, "t-001");
    }

    #[test]
    fn broadcast_broker_publish_no_receivers() {
        let broker = BroadcastBroker::default();
        // No subscribers -- event is lost.
        let result = broker.publish(make_event("tasks", "t-001", "create"));
        assert!(
            result.is_err(),
            "publish with no receivers should return Err"
        );
    }
}
