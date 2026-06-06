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

/// Batch multiple signal updates together.
/// Currently a placeholder; runs `f` immediately.
#[allow(dead_code)]
pub fn batch<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
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
#[path = "signals_test.rs"]
mod tests;
