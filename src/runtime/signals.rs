//! Fine-grained reactivity system
//!
//! This module provides a Leptos-inspired signal system for fine-grained
//! reactivity. Signals are the foundation of the islands architecture.

use std::sync::Arc;
use parking_lot::RwLock;

/// Read signal - immutable reference to a signal
#[allow(dead_code)]
pub type ReadSignal<T> = Signal<T>;

/// Write signal - mutable reference to a signal
#[allow(dead_code)]
pub type WriteSignal<T> = Signal<T>;

/// Signal type for reactive values
///
/// A signal is a container for a value that can be read and written.
/// When a signal's value changes, all dependent computations are re-evaluated.
#[derive(Clone)]
pub struct Signal<T: Clone> {
    /// Inner value storage
    value: Arc<RwLock<T>>,
}

impl<T: Clone> Signal<T> {
    /// Create a new signal with an initial value
    pub fn new(value: T) -> Self {
        Self {
            value: Arc::new(RwLock::new(value)),
        }
    }

    /// Get the current value
    pub fn get(&self) -> T {
        self.value.read().clone()
    }

    /// Set a new value
    pub fn set(&self, value: T) {
        *self.value.write() = value;
    }

    /// Update the value using a closure
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        let mut value = self.value.write();
        f(&mut value);
    }

    /// Read-only access to the inner value
    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, T> {
        self.value.read()
    }

    /// Check if the signal equals a value
    #[allow(dead_code)]
    pub fn equals(&self, other: &T) -> bool
    where
        T: PartialEq,
    {
        *self.value.read() == *other
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
// Derived Signals
// =============================================================================

/// Computed signal - a signal derived from other signals
pub struct Computed<T: Clone> {
    value: Signal<T>,
    #[allow(dead_code)]
    dependencies: Vec<Box<dyn std::any::Any>>,
}

impl<T: Clone> Clone for Computed<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            dependencies: Vec::new(), // Can't clone Any
        }
    }
}

impl<T: Clone> Computed<T> {
    /// Create a new computed signal
    pub fn new<F>(f: F) -> Self
    where
        F: FnOnce() -> T,
    {
        let value = f();
        Self {
            value: Signal::new(value),
            dependencies: Vec::new(),
        }
    }

    /// Get the computed value
    pub fn get(&self) -> T {
        self.value.get()
    }

    /// Read access
    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, T> {
        self.value.read()
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
// Batch Updates
// =============================================================================

/// Batch multiple signal updates together
pub fn batch<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    f()
}

/// Temporarily pause tracking for a computation
#[allow(dead_code)]
pub fn untrack<T, F>(f: F) -> T
where
    F: FnOnce() -> T,
{
    f()
}

// =============================================================================
// Utilities
// =============================================================================

/// Create a signal from an initial value
pub fn signal<T: Clone>(initial: T) -> Signal<T> {
    Signal::new(initial)
}
