//! Event Bus - Central event distribution system

use super::{Event, EventFilter, EventId};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use crossbeam_channel::{bounded, Sender, Receiver};

/// Subscription ID
pub type SubscriptionId = u64;

/// Event handler callback type
pub type EventHandler = Box<dyn Fn(&Event) + Send + Sync>;

/// Subscription handle
pub struct Subscription {
    id: SubscriptionId,
    filter: EventFilter,
    handler: EventHandler,
}

/// Central event bus
pub struct EventBus {
    /// Subscriptions
    subscriptions: RwLock<HashMap<SubscriptionId, Arc<Subscription>>>,
    /// Subscription counter
    next_sub_id: std::sync::atomic::AtomicU64,
    /// Event history (circular buffer)
    history: RwLock<Vec<Event>>,
    /// Max history size
    max_history: usize,
    /// Broadcast channel for async subscribers
    broadcast_tx: Sender<Event>,
    /// Broadcast receiver (clone for each async subscriber)
    broadcast_rx: Receiver<Event>,
}

impl EventBus {
    /// Create new event bus
    pub fn new(max_history: usize) -> Self {
        let (tx, rx) = bounded(1000);
        
        Self {
            subscriptions: RwLock::new(HashMap::new()),
            next_sub_id: std::sync::atomic::AtomicU64::new(1),
            history: RwLock::new(Vec::with_capacity(max_history)),
            max_history,
            broadcast_tx: tx,
            broadcast_rx: rx,
        }
    }

    /// Subscribe to events with filter and handler
    pub fn subscribe(&self, filter: EventFilter, handler: EventHandler) -> SubscriptionId {
        let id = self.next_sub_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        
        let sub = Arc::new(Subscription {
            id,
            filter,
            handler,
        });

        self.subscriptions.write().insert(id, sub);
        id
    }

    /// Unsubscribe
    pub fn unsubscribe(&self, sub_id: SubscriptionId) -> bool {
        self.subscriptions.write().remove(&sub_id).is_some()
    }

    /// Publish an event
    pub fn publish(&self, event: Event) {
        // Store in history
        {
            let mut history = self.history.write();
            if history.len() >= self.max_history {
                history.remove(0);
            }
            history.push(event.clone());
        }

        // Notify subscribers
        let subs = self.subscriptions.read();
        for sub in subs.values() {
            if sub.filter.matches(&event) {
                (sub.handler)(&event);
            }
        }

        // Broadcast for async subscribers
        let _ = self.broadcast_tx.try_send(event);
    }

    /// Get broadcast receiver (for async subscribers)
    pub fn receiver(&self) -> Receiver<Event> {
        self.broadcast_rx.clone()
    }

    /// Get event history
    pub fn history(&self) -> Vec<Event> {
        self.history.read().clone()
    }

    /// Get recent events matching filter
    pub fn recent(&self, filter: &EventFilter, limit: usize) -> Vec<Event> {
        self.history
            .read()
            .iter()
            .rev()
            .filter(|e| filter.matches(e))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get event by ID
    pub fn get(&self, id: EventId) -> Option<Event> {
        self.history.read().iter().find(|e| e.id == id).cloned()
    }

    /// Clear history
    pub fn clear_history(&self) {
        self.history.write().clear();
    }

    /// Get subscriber count
    pub fn subscriber_count(&self) -> usize {
        self.subscriptions.read().len()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(10000)
    }
}

/// Global event bus instance
static GLOBAL_BUS: std::sync::OnceLock<EventBus> = std::sync::OnceLock::new();

/// Get global event bus
pub fn global() -> &'static EventBus {
    GLOBAL_BUS.get_or_init(EventBus::default)
}

/// Publish event to global bus
pub fn emit(event: Event) {
    global().publish(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{EventCategory, EventSeverity};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_publish_subscribe() {
        let bus = EventBus::new(100);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let sub_id = bus.subscribe(
            EventFilter::category(EventCategory::Vm),
            Box::new(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }),
        );

        let event = Event::new(
            EventCategory::Vm,
            EventSeverity::Info,
            "test",
            "Test event",
        );
        bus.publish(event);

        assert_eq!(counter.load(Ordering::SeqCst), 1);
        
        bus.unsubscribe(sub_id);
    }

    #[test]
    fn test_filter() {
        let bus = EventBus::new(100);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        bus.subscribe(
            EventFilter::category(EventCategory::Storage),
            Box::new(move |_| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }),
        );

        // VM event should not trigger storage subscriber
        let vm_event = Event::new(
            EventCategory::Vm,
            EventSeverity::Info,
            "test",
            "VM event",
        );
        bus.publish(vm_event);
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Storage event should trigger
        let storage_event = Event::new(
            EventCategory::Storage,
            EventSeverity::Info,
            "test",
            "Storage event",
        );
        bus.publish(storage_event);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
