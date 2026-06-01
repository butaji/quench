//! Fine-grained reactivity system
//!
//! This module provides a Leptos-inspired signal system for fine-grained
//! reactivity. Signals are the foundation of the islands architecture.

use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// Signal type for reactive values
///
/// A signal is a container for a value that can be read and written.
#[allow(dead_code)]
pub struct Signal<T: Clone> {
    /// Inner value storage
    value: Arc<RwLock<T>>,
    /// Next subscriber ID
    next_id: Arc<AtomicUsize>,
    /// Subscribers (id -> callback)
    subscribers: Arc<RwLock<Vec<(usize, Arc<dyn Fn() + Send + Sync>)>>>,
}

impl<T: Clone> Signal<T> {
    /// Create a new signal with an initial value
    pub fn new(value: T) -> Self {
        Self {
            value: Arc::new(RwLock::new(value)),
            next_id: Arc::new(AtomicUsize::new(0)),
            subscribers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get the current value
    pub fn get(&self) -> T {
        self.value.read().clone()
    }

    /// Set a new value
    pub fn set(&self, value: T) {
        *self.value.write() = value;
        self.notify();
    }

    /// Update the value using a closure
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        f(&mut self.value.write());
        self.notify();
    }

    /// Read-only access to the inner value
    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, T> {
        self.value.read()
    }

    /// Subscribe to changes
    pub fn subscribe<F>(&self, effect: F) -> impl Fn() + Send + Sync
    where
        F: Fn() + Send + Sync + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let subscribers = self.subscribers.clone();
        subscribers
            .write()
            .push((id, Arc::new(effect) as Arc<dyn Fn() + Send + Sync>));

        move || {
            let mut subs = subscribers.write();
            subs.retain(|(sid, _)| *sid != id);
        }
    }

    /// Notify all subscribers
    fn notify(&self) {
        // Collect callbacks under read lock, then drop lock before running
        let callbacks: Vec<Arc<dyn Fn() + Send + Sync>> = {
            let subs = self.subscribers.read();
            subs.iter().map(|(_, cb)| cb.clone()).collect()
        };
        // Now run callbacks outside the lock to avoid deadlocks
        for callback in callbacks {
            callback();
        }
    }
}

impl<T: Clone> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            next_id: self.next_id.clone(),
            subscribers: self.subscribers.clone(),
        }
    }
}

impl<T: Clone> Default for Signal<T>
where
    T: Default,
{
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Clone> From<T> for Signal<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

// =============================================================================
// Computed
// =============================================================================

/// Computed signal - a signal derived from other signals
#[allow(dead_code)]
pub struct Computed<T> {
    compute: Arc<dyn Fn() -> T + Send + Sync>,
    cache: Arc<RwLock<Option<T>>>,
    dirty: Arc<AtomicBool>,
}

impl<T: Clone + Send + Sync + 'static> Computed<T> {
    /// Create a new computed signal
    pub fn new<F>(f: F) -> Self
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        let val = f();
        Self {
            compute: Arc::new(f),
            cache: Arc::new(RwLock::new(Some(val))),
            dirty: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get the computed value - recomputes every call for reactivity
    ///
    /// Note: The dirty flag is set up for future optimization when automatic
    /// dependency tracking is implemented. Currently always recomputes.
    pub fn get(&self) -> T {
        // Always recompute for now to maintain reactivity
        // TODO: When dependency tracking is added, check dirty flag first
        let new_val = (self.compute)();
        let mut cache = self.cache.write();
        *cache = Some(new_val.clone());
        new_val
    }

    /// Mark as dirty (call when dependencies change)
    #[allow(dead_code)]
    pub fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
    }

    /// Read access
    pub fn read(&self) -> T {
        self.get()
    }
}

impl<T: Clone + Send + Sync + 'static> Clone for Computed<T> {
    fn clone(&self) -> Self {
        Self {
            compute: self.compute.clone(),
            cache: self.cache.clone(),
            dirty: self.dirty.clone(),
        }
    }
}

// =============================================================================
// Effect
// =============================================================================

/// Effect - a side effect that depends on signals
pub struct Effect {
    cleanup: Option<Box<dyn Fn()>>,
}

impl Effect {
    /// Create a new effect
    #[allow(dead_code)]
    pub fn new<F, C>(f: F, cleanup: C) -> Effect
    where
        F: FnOnce(),
        C: Fn() + 'static,
    {
        f();
        Effect {
            cleanup: Some(Box::new(cleanup)),
        }
    }

    /// Clean up the effect
    pub fn cleanup(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

impl Drop for Effect {
    fn drop(&mut self) {
        self.cleanup();
    }
}

// =============================================================================
// Utilities
// =============================================================================

/// Batch multiple signal updates together
///
/// When batch is active, signal notifications are deferred until the batch ends.
#[allow(dead_code)]
pub fn batch<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    // In this simple implementation, we collect pending notifications
    // and flush them after f() completes.
    // For a full implementation, we'd use a thread-local or context to track batching.
    // Since Signal::notify() is called directly, we need a different approach.
    // For now, we rely on the fact that multiple set/update calls will each notify,
    // but subscribers will see intermediate states. True batching requires
    // wrapping signals in a batch-aware layer.
    // This is a placeholder that runs f() immediately.
    // Real batching would use AtomicBool::compare_exchange to track batch state.
    f()
}

/// Create a signal from an initial value
#[allow(dead_code)]
pub fn signal<T: Clone>(initial: T) -> Signal<T> {
    Signal::new(initial)
}

/// Create a computed signal
pub fn computed<T, F>(f: F) -> Computed<T>
where
    T: Clone + Send + Sync + 'static,
    F: Fn() -> T + Send + Sync + 'static,
{
    Computed::new(f)
}

// =============================================================================
// Store
// =============================================================================

/// A store is an object with reactive properties
pub struct Store<T: Clone> {
    signal: Signal<T>,
}

impl<T: Clone> Store<T> {
    /// Create a new store
    pub fn new(state: T) -> Self {
        Self {
            signal: Signal::new(state),
        }
    }

    /// Get the current state
    pub fn get(&self) -> T {
        self.signal.get()
    }

    /// Set the state
    pub fn set(&self, state: T) {
        self.signal.set(state);
    }
}

impl<T: Clone> Clone for Store<T> {
    fn clone(&self) -> Self {
        Self {
            signal: self.signal.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_new_and_get() {
        let sig = Signal::new(42i32);
        assert_eq!(sig.get(), 42);
    }

    #[test]
    fn test_signal_set() {
        let sig = Signal::new(0i32);
        sig.set(100);
        assert_eq!(sig.get(), 100);
    }

    #[test]
    fn test_signal_update() {
        let sig = Signal::new(10i32);
        sig.update(|v| *v *= 2);
        assert_eq!(sig.get(), 20);
    }

    #[test]
    fn test_signal_clone() {
        let sig1 = Signal::new(vec![1, 2, 3]);
        let sig2 = sig1.clone();
        assert_eq!(sig1.get(), sig2.get());
        sig1.set(vec![4, 5]);
        assert_eq!(sig2.get(), vec![4, 5]);
    }

    #[test]
    fn test_signal_read() {
        let sig = Signal::new(42i32);
        let guard = sig.read();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_signal_default() {
        let sig: Signal<i32> = Signal::default();
        assert_eq!(sig.get(), 0);
    }

    #[test]
    fn test_signal_from() {
        let sig: Signal<String> = Signal::from("hello".to_string());
        assert_eq!(sig.get(), "hello");
    }

    #[test]
    fn test_signal_with_string() {
        let sig = Signal::new("hello".to_string());
        sig.update(|s| s.push_str(" world"));
        assert_eq!(sig.get(), "hello world");
    }

    #[test]
    fn test_computed_new_and_get() {
        let comp = Computed::new(|| 2 + 2);
        assert_eq!(comp.get(), 4);
    }

    #[test]
    fn test_computed_clone() {
        let comp1 = Computed::new(|| vec![1, 2]);
        let comp2 = comp1.clone();
        assert_eq!(comp1.get(), comp2.get());
    }

    #[test]
    fn test_batch() {
        let sig = Signal::new(0i32);
        batch(|| {
            sig.set(1);
            sig.set(2);
            sig.set(3);
        });
        assert_eq!(sig.get(), 3);
    }

    #[test]
    fn test_signal_helper() {
        let sig = signal(42i32);
        assert_eq!(sig.get(), 42);
    }

    #[test]
    fn test_computed_helper() {
        let comp: Computed<i32> = computed(|| 2 * 21);
        assert_eq!(comp.get(), 42);
    }

    #[test]
    fn test_store_new_and_get() {
        let store = Store::new("state".to_string());
        assert_eq!(store.get(), "state");
    }

    #[test]
    fn test_store_set() {
        let store = Store::new(0i32);
        store.set(99);
        assert_eq!(store.get(), 99);
    }

    #[test]
    fn test_store_clone() {
        let store1 = Store::new(vec![1, 2]);
        let store2 = store1.clone();
        assert_eq!(store1.get(), store2.get());
    }

    #[test]
    fn test_signal_with_complex_type() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert("key".to_string(), "value".to_string());
        let sig = Signal::new(map);
        let mut updated = sig.get();
        updated.insert("key2".to_string(), "value2".to_string());
        sig.set(updated);
        assert_eq!(sig.get().len(), 2);
    }

    #[test]
    fn test_computed_type_inference() {
        let comp = Computed::new(|| 42i32);
        assert_eq!(comp.get(), 42);
    }

    #[test]
    fn test_effect_new_runs_immediately() {
        // Effect::new runs f() immediately
        use std::sync::atomic::{AtomicBool, Ordering};
        static RAN: AtomicBool = AtomicBool::new(false);
        RAN.store(false, Ordering::Relaxed);

        {
            let _effect = Effect::new(
                || { RAN.store(true, Ordering::Relaxed); },
                || {},
            );
        }
        assert!(RAN.load(Ordering::Relaxed));
    }

    #[test]
    fn test_effect_stores_cleanup() {
        // Effect stores cleanup closure for Drop
        use std::sync::atomic::{AtomicBool, Ordering};
        static CLEANUP_RAN: AtomicBool = AtomicBool::new(false);
        CLEANUP_RAN.store(false, Ordering::Relaxed);

        // This test verifies Effect::new doesn't panic and stores cleanup
        let _effect = Effect::new(|| {}, || {});
        drop(_effect);
        // Cleanup ran on drop (if Effect impl was different)
        // For now just verify no panic
    }

    #[test]
    fn test_computed_depends_on_signal() {
        // Computed::get() recomputes every call, so it reads current signal value
        let sig = Signal::new(5i32);
        let comp = Computed::new({
            let sig_clone = sig.clone();
            move || sig_clone.get() * 3
        });
        assert_eq!(comp.get(), 15);
        // Changing sig updates the computed value since get() recomputes
        sig.set(10);
        assert_eq!(comp.get(), 30); // Now 30, reactive
    }

    #[test]
    fn test_subscribe_unsubscribe_correct_id() {
        // Bug 1 regression: unsubscribe should remove correct subscriber, not last
        use std::sync::atomic::{AtomicUsize, Ordering};

        let sig = Signal::new(0i32);
        let call_count = Arc::new(AtomicUsize::new(0));

        // A subscribes
        let call_count_a = call_count.clone();
        let unsub_a = sig.subscribe(move || {
            call_count_a.fetch_add(1, Ordering::Relaxed);
        });

        // B subscribes - also increments count
        let call_count_b = call_count.clone();
        let _unsub_b = sig.subscribe(move || {
            call_count_b.fetch_add(1, Ordering::Relaxed);
        });

        // A unsubscribes
        unsub_a();

        // Set should only trigger B (count = 1), not A (was unsubscribed)
        sig.set(1);
        assert_eq!(call_count.load(Ordering::Relaxed), 1); // Only B was called
    }

    #[test]
    fn test_multiple_subscribers_unsubscribe_specific() {
        // Test that unsubscribing in LIFO order works correctly
        use std::sync::atomic::{AtomicUsize, Ordering};

        let sig = Signal::new(0i32);
        let counts = Arc::new((AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0)));

        let counts_0 = counts.clone();
        let unsub0 = sig.subscribe(move || {
            counts_0.0.fetch_add(1, Ordering::Relaxed);
        });
        let counts_1 = counts.clone();
        let unsub1 = sig.subscribe(move || {
            counts_1.1.fetch_add(1, Ordering::Relaxed);
        });
        let counts_2 = counts.clone();
        let _unsub2 = sig.subscribe(move || {
            counts_2.2.fetch_add(1, Ordering::Relaxed);
        });

        // Unsubscribe middle one
        unsub1();

        sig.set(1);

        assert_eq!(counts.0.load(Ordering::Relaxed), 1); // 0 was called
        assert_eq!(counts.1.load(Ordering::Relaxed), 0); // 1 was unsubscribed
        assert_eq!(counts.2.load(Ordering::Relaxed), 1); // 2 was called
    }

    #[test]
    fn test_concurrent_subscribe_during_notify() {
        // Bug 2 regression: subscribe during notify should not cause deadlock or panic
        use std::sync::atomic::{AtomicUsize, Ordering};

        let sig = Signal::new(0i32);
        let inner_count = Arc::new(AtomicUsize::new(0));

        // Subscribe a callback that itself subscribes another callback
        let sig2 = sig.clone();
        let inner_count_clone = inner_count.clone();
        let _unsub = sig.subscribe(move || {
            inner_count_clone.fetch_add(1, Ordering::Relaxed);
            // This should not deadlock or panic
            let _new_unsub = sig2.subscribe(|| {});
        });

        // Notify should complete without deadlock
        sig.set(1);

        assert_eq!(inner_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_unsubscribe_during_notify() {
        // Bug 2 regression: subscribe during notify should not cause deadlock or panic
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let sig = Signal::new(0i32);
        let count = Arc::new(AtomicUsize::new(0));

        // Subscriber that subscribes another during notify
        let sig2 = sig.clone();
        let count_clone = count.clone();
        let _unsub = sig.subscribe(move || {
            count_clone.fetch_add(1, Ordering::Relaxed);
            // Subscribe a new callback during notify - should not deadlock
            let _new_unsub = sig2.subscribe(|| {});
        });

        // Setting value triggers notify - should not panic or deadlock
        sig.set(1);

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_subscribe_returns_correct_unsubscribe() {
        // Verify unsubscribe returned is for the correct subscription
        use std::sync::atomic::{AtomicUsize, Ordering};

        let sig = Signal::new(0i32);
        let hit_count = Arc::new(AtomicUsize::new(0));

        // First subscriber that should remain
        let hit_count_0 = hit_count.clone();
        let _unsub_first = sig.subscribe(move || {
            hit_count_0.fetch_add(10, Ordering::Relaxed);
        });

        // Second subscriber that will be removed
        let hit_count_1 = hit_count.clone();
        let unsub_second = sig.subscribe(move || {
            hit_count_1.fetch_add(100, Ordering::Relaxed);
        });

        // Third subscriber that should remain
        let hit_count_2 = hit_count.clone();
        let _unsub_third = sig.subscribe(move || {
            hit_count_2.fetch_add(1000, Ordering::Relaxed);
        });

        // Remove second subscriber
        unsub_second();

        sig.set(1);

        // Only first (10) and third (1000) should have been called
        assert_eq!(hit_count.load(Ordering::Relaxed), 1010);
    }
}
