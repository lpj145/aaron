pub mod error;
pub mod subscriber;
pub mod topic;

pub use error::EventHubError;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
pub use subscriber::Subscriber;
use tokio::sync::RwLock;
use topic::Topic;

/// Default buffer capacity for each subscriber queue.
pub const DEFAULT_EVENT_CAPACITY: usize = 128;

/// A high-performance, strongly-typed in-memory Pub/Sub event bus powered by lockless `crossfire` channels.
///
/// Any type implementing `Clone + Send + Sync + Unpin + 'static` can be published and subscribed to without
/// string topic keys or manual serialization.
///
/// Designed with fine-grained per-topic concurrency to ensure zero global lock contention during event dispatch.
///
/// # Example
///
/// ```rust
/// use aaron_core::EventHub;
///
/// #[derive(Clone, Debug, PartialEq)]
/// struct NodeAlert {
///     message: String,
/// }
///
/// # async fn doc_example() -> Result<(), aaron_core::BoxError> {
/// let hub = EventHub::new();
///
/// // Subscribe to NodeAlert events
/// let mut sub = hub.subscribe::<NodeAlert>().await;
///
/// // Publish an event
/// hub.publish(NodeAlert {
///     message: "CPU 95%".to_string(),
/// }).await;
///
/// // Receive event
/// let event = sub.recv().await?;
/// assert_eq!(event.message, "CPU 95%");
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct EventHub {
    topics: Arc<RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>>,
    default_capacity: usize,
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHub {
    /// Creates a new `EventHub` with the default buffer capacity (128 items per subscriber).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_EVENT_CAPACITY)
    }

    /// Creates a new `EventHub` with a custom default capacity per subscriber queue.
    pub fn with_capacity(default_capacity: usize) -> Self {
        Self {
            topics: Arc::new(RwLock::new(HashMap::new())),
            default_capacity,
        }
    }

    /// Subscribes to events of type `E` using the default queue capacity.
    pub async fn subscribe<E: Clone + Send + Sync + Unpin + 'static>(&self) -> Subscriber<E> {
        self.subscribe_with_capacity::<E>(self.default_capacity)
            .await
    }

    /// Subscribes to events of type `E` with a custom queue capacity.
    pub async fn subscribe_with_capacity<E: Clone + Send + Sync + Unpin + 'static>(
        &self,
        capacity: usize,
    ) -> Subscriber<E> {
        let type_id = TypeId::of::<E>();
        let (tx, rx) = crossfire::mpsc::bounded_async::<E>(capacity);

        let topic = self.get_or_create_topic::<E>(type_id).await;
        topic.add_subscriber(tx).await;

        Subscriber::new(rx)
    }

    /// Publishes an event of type `E` to all active subscribers of this event type.
    ///
    /// Returns the number of subscribers the event was delivered to.
    /// Dropped or disconnected subscribers are automatically cleaned up without holding global table locks.
    pub async fn publish<E: Clone + Send + Sync + Unpin + 'static>(&self, event: E) -> usize {
        let type_id = TypeId::of::<E>();

        // 1. Obtain topic handle under read lock and release table lock immediately
        let maybe_topic = {
            let read_guard = self.topics.read().await;
            read_guard.get(&type_id).cloned()
        };

        // 2. Dispatch to subscribers without holding the global topics table lock
        if let Some(entry) = maybe_topic
            && let Ok(topic) = entry.downcast::<Topic<E>>()
        {
            return topic.publish(&event).await;
        }

        0
    }

    /// Returns the number of active subscribers for event type `E`.
    pub async fn subscriber_count<E: Send + Sync + Unpin + 'static>(&self) -> usize {
        let type_id = TypeId::of::<E>();
        let maybe_topic = {
            let read_guard = self.topics.read().await;
            read_guard.get(&type_id).cloned()
        };

        if let Some(entry) = maybe_topic
            && let Ok(topic) = entry.downcast::<Topic<E>>()
        {
            topic.subscriber_count().await
        } else {
            0
        }
    }

    /// Clears all subscribers for event type `E`.
    pub async fn clear<E: 'static>(&self) {
        let type_id = TypeId::of::<E>();
        let mut write_guard = self.topics.write().await;
        write_guard.remove(&type_id);
    }

    /// Clears all topics and subscribers across all event types.
    pub async fn clear_all(&self) {
        let mut write_guard = self.topics.write().await;
        write_guard.clear();
    }

    /// Helper to get or insert a topic with double-checked locking.
    async fn get_or_create_topic<E: Send + Sync + Unpin + 'static>(
        &self,
        type_id: TypeId,
    ) -> Arc<Topic<E>> {
        {
            let read_guard = self.topics.read().await;
            if let Some(entry) = read_guard.get(&type_id)
                && let Ok(topic) = entry.clone().downcast::<Topic<E>>()
            {
                return topic;
            }
        }

        let mut write_guard = self.topics.write().await;
        if let Some(entry) = write_guard.get(&type_id)
            && let Ok(topic) = entry.clone().downcast::<Topic<E>>()
        {
            return topic;
        }

        let new_topic = Arc::new(Topic::<E>::new());
        write_guard.insert(type_id, new_topic.clone());
        new_topic
    }
}
