//! Fine-grained reactivity system
//!
//! Leptos-inspired signals with automatic dependency tracking.
//! Signals are the foundation of the islands architecture.
//!
//! # Example
//! ```ignore
//! let count = Signal::new(0);
//! let doubled = Computed::new({
//!     let count = count.clone();
//!     move || count.get() * 2
//! });
//!
//! Effect::new(move || {
//!     println!("Count: {}, Doubled: {}", count.get(), doubled.get());
//! });
//!
//! count.set(5); // Effect re-runs automatically
//! ```

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use parking_lot::RwLock;

// =============================================================================
// Effect Stack (thread-local tracking for auto-subscription)
// =============================================================================

thread_local! {
    static EFFECT_STACK: RefCell<Vec<usize>> = RefCell::new(Vec::new());
}

fn with_effect_stack_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut Vec<usize>) -> R,
{
    EFFECT_STACK.with(|stack| f(&mut *stack.borrow_mut()))
}

fn with_effect_stack_ref<F, R>(f: F) -> R
where
    F: FnOnce(&Vec<usize>) -> R,
{
    EFFECT_STACK.with(|stack| f(&*stack.borrow()))
}

fn current_effect_id() -> Option<usize> {
    with_effect_stack_ref(|stack| stack.last().copied())
}

fn push_effect(id: usize) {
    with_effect_stack_mut(|stack| stack.push(id));
}

fn pop_effect() {
    with_effect_stack_mut(|stack| {
        stack.pop();
    });
}

// =============================================================================
// Global Effect Registry (stores Arc closures by ID)
// =============================================================================

static NEXT_EFFECT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn alloc_effect_id() -> usize {
    NEXT_EFFECT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

// =============================================================================
// Batch / Untrack
// =============================================================================

thread_local! {
    static BATCH_DEPTH: RefCell<u32> = RefCell::new(0);
    static PENDING_EFFECTS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// Batch multiple signal updates together.
/// Effects are deferred until the batch completes.
pub fn batch<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    BATCH_DEPTH.with(|depth| *depth.borrow_mut() += 1);
    let result = f();
    BATCH_DEPTH.with(|depth| {
        let mut d = depth.borrow_mut();
        *d -= 1;
        if *d == 0 {
            // Flush pending effects
            let pending: Vec<usize> = PENDING_EFFECTS.with(|pe| {
                let set = pe.borrow().clone();
                pe.borrow_mut().clear();
                set.into_iter().collect()
            });
            for id in pending {
                trigger_effect(id);
            }
        }
    });
    result
}

/// Check if currently inside a batch.
fn is_batched() -> bool {
    BATCH_DEPTH.with(|depth| *depth.borrow() > 0)
}

/// Schedule an effect to run (or defer if batched).
fn schedule_effect(id: usize) {
    if is_batched() {
        PENDING_EFFECTS.with(|pe| {
            pe.borrow_mut().insert(id);
        });
    } else {
        trigger_effect(id);
    }
}

/// Temporarily disable tracking for a computation.
pub fn untrack<T, F>(f: F) -> T
where
    F: FnOnce() -> T,
{
    let saved = with_effect_stack_mut(|stack| {
        let s = stack.clone();
        stack.clear();
        s
    });
    let result = f();
    with_effect_stack_mut(|stack| {
        *stack = saved;
    });
    result
}

// =============================================================================
// Effect callback type — shareable, cloneable
// =============================================================================

type EffectCallback = Arc<dyn Fn() + Send + Sync>;

/// Global map of active effect callbacks by ID.
static EFFECT_CALLBACKS: RwLock<Vec<Option<EffectCallback>>> = RwLock::new(Vec::new());

fn register_effect_callback(id: usize, callback: EffectCallback) {
    let mut callbacks = EFFECT_CALLBACKS.write();
    if id >= callbacks.len() {
        callbacks.resize_with(id + 1, || None);
    }
    callbacks[id] = Some(callback);
}

fn get_effect_callback(id: usize) -> Option<EffectCallback> {
    EFFECT_CALLBACKS.read().get(id)?.clone()
}

fn remove_effect_callback(id: usize) {
    let mut callbacks = EFFECT_CALLBACKS.write();
    if let Some(slot) = callbacks.get_mut(id) {
        *slot = None;
    }
}

fn trigger_effect(id: usize) {
    if let Some(cb) = get_effect_callback(id) {
        push_effect(id);
        cb();
        pop_effect();
    }
}

// =============================================================================
// Signal
// =============================================================================

/// Read signal - immutable reference to a signal
pub type ReadSignal<T> = Signal<T>;

/// Write signal - mutable reference to a signal
pub type WriteSignal<T> = Signal<T>;

/// Signal type for reactive values.
///
/// A signal is a container for a value that can be read and written.
/// When a signal's value changes, all dependent computations are re-evaluated.
#[derive(Clone)]
pub struct Signal<T: Clone> {
    /// Inner value storage
    value: Arc<RwLock<T>>,
    /// Subscriber effect IDs
    subscribers: Arc<RwLock<HashSet<usize>>>,
}

impl<T: Clone + fmt::Debug> fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Signal").field("value", &self.value.read()).finish()
    }
}

impl<T: Clone> Signal<T> {
    /// Create a new signal with an initial value.
    pub fn new(value: T) -> Self {
        Self {
            value: Arc::new(RwLock::new(value)),
            subscribers: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Get the current value.
    ///
    /// If called inside an effect, automatically subscribes the effect
    /// to this signal.
    pub fn get(&self) -> T {
        if let Some(effect_id) = current_effect_id() {
            self.subscribers.write().insert(effect_id);
        }
        self.value.read().clone()
    }

    /// Peek the value without subscribing (no tracking).
    pub fn peek(&self) -> T {
        self.value.read().clone()
    }

    /// Set a new value.
    ///
    /// Notifies all subscribers after the value changes.
    pub fn set(&self, value: T) {
        *self.value.write() = value;
        self.notify();
    }

    /// Update the value using a closure.
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        let mut value = self.value.write();
        f(&mut value);
        drop(value); // release lock before notifying
        self.notify();
    }

    /// Read-only access to the inner value (lock guard).
    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, T> {
        self.value.read()
    }

    /// Check if the signal equals a value.
    #[allow(dead_code)]
    pub fn equals(&self, other: &T) -> bool
    where
        T: PartialEq,
    {
        *self.value.read() == *other
    }

    /// Notify all subscribers.
    fn notify(&self) {
        let subs: Vec<usize> = self.subscribers.read().iter().copied().collect();
        for id in subs {
            schedule_effect(id);
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

/// Computed signal - a signal derived from other signals.
///
/// Automatically re-computes when dependencies change.
#[derive(Clone)]
pub struct Computed<T: Clone> {
    value: Signal<T>,
    #[allow(dead_code)]
    compute_fn: Arc<dyn Fn() -> T + Send + Sync>,
}

impl<T: Clone + Send + Sync + 'static> Computed<T> {
    /// Create a new computed signal.
    ///
    /// The computation function is called immediately and whenever
    /// any signal accessed during its execution changes.
    pub fn new<F>(f: F) -> Self
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        let initial = f();
        let signal = Signal::new(initial);
        let f = Arc::new(f);

        // Set up an effect that re-computes when dependencies change.
        // We intentionally leak the Effect so it lives for the program duration.
        // In a real implementation, Computed would store and drop it on disposal.
        {
            let signal = signal.clone();
            let f = f.clone();
            let _effect = Effect::new(move || {
                let new_value = f();
                signal.set(new_value);
            });
            std::mem::forget(_effect);
        }

        Self {
            value: signal,
            compute_fn: f,
        }
    }

    /// Get the computed value.
    pub fn get(&self) -> T {
        self.value.get()
    }

    /// Peek without subscribing.
    pub fn peek(&self) -> T {
        self.value.peek()
    }

    /// Read access.
    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, T> {
        self.value.read()
    }
}

// =============================================================================
// Effect
// =============================================================================

/// Effect - a side effect that depends on signals.
///
/// Automatically subscribes to any signals read during execution.
/// Re-runs when those signals change.
pub struct Effect {
    id: usize,
    #[allow(dead_code)]
    cleanup: Option<Box<dyn Fn()>>,
}

impl Effect {
    /// Create and run a new effect immediately.
    ///
    /// # Example
    /// ```ignore
    /// Effect::new(move || {
    ///     println!("Value: {}", signal.get());
    /// });
    /// ```
    pub fn new<F>(f: F) -> Effect
    where
        F: Fn() + Send + Sync + 'static,
    {
        let id = alloc_effect_id();
        let f: EffectCallback = Arc::new(f);

        register_effect_callback(id, f.clone());

        // Run immediately (and subscribe to signals)
        push_effect(id);
        f();
        pop_effect();

        Effect { id, cleanup: None }
    }

    /// Create an effect with cleanup.
    #[allow(dead_code)]
    pub fn new_with_cleanup<F, C>(f: F, cleanup: C) -> Effect
    where
        F: Fn() + Send + Sync + 'static,
        C: Fn() + Send + Sync + 'static,
    {
        let mut effect = Self::new(f);
        effect.cleanup = Some(Box::new(cleanup));
        effect
    }
}

impl Drop for Effect {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
        remove_effect_callback(self.id);
    }
}

// =============================================================================
// Utilities
// =============================================================================

/// Create a signal from an initial value.
pub fn signal<T: Clone>(initial: T) -> Signal<T> {
    Signal::new(initial)
}

/// Create a computed signal.
pub fn computed<T, F>(f: F) -> Computed<T>
where
    T: Clone + Send + Sync + 'static,
    F: Fn() -> T + Send + Sync + 'static,
{
    Computed::new(f)
}

/// Create an effect.
pub fn effect<F>(f: F) -> Effect
where
    F: Fn() + Send + Sync + 'static,
{
    Effect::new(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_signal_basic() {
        let s = Signal::new(10);
        assert_eq!(s.get(), 10);
        s.set(20);
        assert_eq!(s.get(), 20);
    }

    #[test]
    fn test_signal_effect() {
        let s = Signal::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        let _effect = {
            let s = s.clone();
            let counter = counter.clone();
            Effect::new(move || {
                let _ = s.get(); // subscribe
                counter.fetch_add(1, Ordering::SeqCst);
            })
        };

        // Effect runs immediately
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        s.set(5);
        assert_eq!(counter.load(Ordering::SeqCst), 2);

        s.set(10);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn test_computed() {
        let a = Signal::new(2);
        let b = Signal::new(3);
        let sum = Computed::new({
            let a = a.clone();
            let b = b.clone();
            move || a.get() + b.get()
        });

        assert_eq!(sum.get(), 5);

        a.set(10);
        assert_eq!(sum.get(), 13);

        b.set(7);
        assert_eq!(sum.get(), 17);
    }

    #[test]
    fn test_batch() {
        let s = Signal::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        let _effect = {
            let s = s.clone();
            let counter = counter.clone();
            Effect::new(move || {
                let _ = s.get();
                counter.fetch_add(1, Ordering::SeqCst);
            })
        };

        assert_eq!(counter.load(Ordering::SeqCst), 1);

        batch(|| {
            s.set(1);
            s.set(2);
            s.set(3);
            // Effect should NOT run inside batch
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        });

        // Effect runs once after batch
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_untrack() {
        let s = Signal::new(0);
        let counter = Arc::new(AtomicUsize::new(0));

        let _effect = {
            let s = s.clone();
            let counter = counter.clone();
            Effect::new(move || {
                // This read is tracked
                let _val = s.get();
                // This read is not tracked
                let _ = untrack(|| s.get());
                counter.fetch_add(1, Ordering::SeqCst);
            })
        };

        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Even though we set, the effect should only re-run if the
        // tracked value changes. Since untrack doesn't subscribe,
        // the effect only sees the first tracked read.
        s.set(5);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
