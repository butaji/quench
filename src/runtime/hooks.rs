//! Preact hooks implementation
//!
//! This module provides React/Preact-compatible hooks for use in
//! components. Hooks must be called in the same order on every render.

use std::sync::Arc;
use parking_lot::RwLock;

// Re-export signals
pub use super::signals::Signal;

/// State hook result
pub type UseStateResult<T> = (T, Box<dyn Fn(T) + Send + Sync>);

/// useState hook
///
/// Creates a reactive state value that persists across renders.
/// When the setter is called, the component will re-render.
///
/// # Example
/// ```ignore
/// let (count, setCount) = use_state(|| 0);
/// setCount(count + 1);
/// ```
pub fn use_state<T, F>(initial: F) -> UseStateResult<T>
where
    T: Clone + Send + Sync + 'static,
    F: FnOnce() -> T + Clone + Send + Sync + 'static,
{
    let initial_value = initial();
    
    let state = Arc::new(RwLock::new(initial_value));
    let state_clone = state.clone();
    
    let getter: T = state.read().clone();
    
    let setter: Box<dyn Fn(T) + Send + Sync> = Box::new(move |new_value: T| {
        *state_clone.write() = new_value;
        // In a full implementation, this would trigger a re-render
    });
    
    (getter, setter)
}

/// useRef hook
///
/// Creates a mutable reference that persists across renders
/// but does NOT trigger a re-render when changed.
///
/// # Example
/// ```ignore
/// let timer_ref = use_ref(|| None::<TimeoutId>);
/// timer_ref.set(Some(setTimeout(...)));
/// ```
pub struct Ref<T> {
    inner: Arc<RwLock<Option<T>>>,
}

impl<T: Clone> Ref<T> {
    /// Get current value
    pub fn get(&self) -> Option<T> {
        self.inner.read().clone()
    }

    /// Get mutable reference
    pub fn get_mut(&mut self) -> Option<T> {
        self.inner.read().clone()
    }

    /// Set value
    pub fn set(&mut self, value: T) {
        *self.inner.write() = Some(value);
    }
}

impl<T: Clone> Clone for Ref<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// useRef hook
pub fn use_ref<T, F>(initial: F) -> Ref<T>
where
    T: Clone + 'static,
    F: FnOnce() -> T,
{
    Ref {
        inner: Arc::new(RwLock::new(Some(initial()))),
    }
}

/// useRef with default value
pub fn use_ref_default<T: Clone + Default>() -> Ref<T> {
    Ref {
        inner: Arc::new(RwLock::new(None::<T>)),
    }
}

/// Memoized value
pub struct Memo<T> {
    value: Arc<RwLock<Option<T>>>,
    deps_hash: usize,
}

impl<T: Clone> Memo<T> {
    /// Get the memoized value
    pub fn get(&self) -> Option<T> {
        self.value.read().clone()
    }
}

/// Compute hash of dependencies
fn hash_deps(deps: &[impl std::hash::Hash + Sized]) -> usize {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    for dep in deps {
        dep.hash(&mut hasher);
    }
    hasher.finish() as usize
}

/// useMemo hook
///
/// Memoizes an expensive computation. The value is only recomputed
/// when dependencies change.
///
/// # Example
/// ```ignore
/// let expensive = use_memo(|| compute_expensive_value(a, b), [a, b]);
/// ```
pub fn use_memo<T, F, D>(factory: F, deps: &[D]) -> T
where
    T: Clone + 'static,
    F: FnOnce() -> T,
    D: std::hash::Hash + Sized + 'static,
{
    // Simple implementation: always recompute
    // Full implementation would cache and check deps
    let _hash = hash_deps(deps);
    factory()
}

/// Callback memoization
pub struct Callback<F> {
    inner: Arc<F>,
    deps_hash: usize,
}

impl<F> Callback<F> {
    /// Get the callback
    pub fn get(&self) -> &F {
        &*self.inner
    }
}

/// useCallback hook
///
/// Memoizes a callback so it maintains referential equality
/// unless its dependencies change.
///
/// # Example
/// ```ignore
/// let handleClick = use_callback(|e: MouseEvent| {
///     console.log!("Clicked!");
/// }, []); // empty deps = never recreated
/// ```
pub fn use_callback<F, D>(callback: F, _deps: &[D]) -> F
where
    F: Clone + 'static,
    D: std::hash::Hash + Sized + 'static,
{
    // Simple implementation: always return the callback
    // Full implementation would cache and check deps
    callback
}

/// Reducer result
pub type ReducerResult<S, A> = (S, Box<dyn Fn(A) + Send + Sync>);

/// useReducer hook
///
/// Manages complex state with a reducer pattern.
///
/// # Example
/// ```ignore
/// fn reducer(state: i32, action: &str) -> i32 {
///     match action {
///         "increment" => state + 1,
///         "decrement" => state - 1,
///         _ => state,
///     }
/// }
///
/// let (state, dispatch) = use_reducer(reducer, 0);
/// dispatch("increment");
/// ```
pub fn use_reducer<S, A, R>(reducer: R, initial: S) -> ReducerResult<S, A>
where
    S: Clone + Send + Sync + 'static,
    A: Send + Sync + 'static,
    R: Fn(S, A) -> S + Clone + Send + Sync + 'static,
{
    let state = Arc::new(RwLock::new(initial));
    let state_clone = state.clone();
    let reducer_clone = reducer.clone();
    
    let dispatch: Box<dyn Fn(A) + Send + Sync> = Box::new(move |action: A| {
        let current = state_clone.read().clone();
        let new_state = reducer_clone(current, action);
        *state_clone.write() = new_state;
        // In a full implementation, this would trigger a re-render
    });
    
    let getter: S = state.read().clone();
    (getter, dispatch)
}

/// Effect cleanup function
pub type EffectCleanup = Box<dyn Fn() + Send + Sync>;

/// Effect callback
pub type EffectCallback = Box<dyn FnOnce() -> Option<EffectCleanup> + Send + Sync + 'static>;

/// useEffect hook
///
/// Runs a side effect after render.
///
/// # Example
/// ```ignore
/// use_effect(|| {
///     console.log!("Component mounted");
///     Some(|| console.log!("Component unmounted"))
/// }, []); // empty deps = run once
///
/// use_effect(|| {
///     document.title = format!("Count: {}", count);
/// }, [count]); // run when count changes
/// ```
pub fn use_effect<F, D>(_callback: F, _deps: D)
where
    F: FnOnce() -> Option<EffectCleanup> + Send + Sync + 'static,
    D: AsRef<[usize]> + 'static,
{
    // SSR: effects are not run synchronously
    // Client-side: would schedule effect execution after paint
}

/// useLayoutEffect hook
///
/// Same as useEffect but runs synchronously before browser paint.
pub fn use_layout_effect<F, D>(_callback: F, _deps: D)
where
    F: FnOnce() -> Option<EffectCleanup> + Send + Sync + 'static,
    D: AsRef<[usize]> + 'static,
{
    // SSR: effects are not run
    // Client-side: would schedule synchronous effect execution
}

/// Context value wrapper
pub struct Context<T> {
    value: Arc<dyn std::any::Any + Send + Sync>,
    default_value: Arc<dyn std::any::Any + Send + Sync>,
    _marker: std::marker::PhantomData<T>,
}

impl<T: Clone> Clone for Context<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            default_value: self.default_value.clone(),
            _marker: self._marker,
        }
    }
}

impl<T: Clone + Send + Sync + 'static> Context<T> {
    /// Create a new context
    pub fn new(value: T) -> Self {
        let default = value.clone();
        Self {
            value: Arc::new(value),
            default_value: Arc::new(default),
            _marker: std::marker::PhantomData,
        }
    }

    /// Get the context value
    pub fn get(&self) -> &T {
        self.value
            .downcast_ref::<T>()
            .or_else(|| self.default_value.downcast_ref::<T>())
            .expect("Context type mismatch")
    }
}

/// createContext - creates a context with a default value
pub fn create_context<T: Clone + Send + Sync + 'static>(default_value: T) -> Context<T> {
    Context::new(default_value)
}

/// useContext hook
///
/// Reads a value from the nearest context provider.
///
/// # Example
/// ```ignore
/// let theme = use_context(&ThemeContext);
/// ```
pub fn use_context<T>(_context: &Context<T>) -> T
where
    T: Clone + 'static,
{
    // TODO: Implement proper context propagation
    // This requires a component context that tracks providers
    // For now, return the default value
    unimplemented!("useContext requires context provider setup")
}

/// useDebugValue hook
///
/// Display custom label for custom hooks in React DevTools.
pub fn use_debug_value<T>(_value: T) {
    // No-op in production builds
    // In dev mode with React DevTools integration, this would display the value
}

/// useDebugValue with formatter
pub fn use_debug_value_formatted<T, F>(_value: T, _format: F)
where
    F: FnOnce(&T),
{
    // No-op
}

/// State signal wrapper
pub struct StateSignal<T: Clone> {
    inner: Signal<T>,
}

impl<T: Clone> StateSignal<T> {
    /// Get current value
    pub fn get(&self) -> T {
        self.inner.get()
    }

    /// Set new value
    pub fn set(&self, value: T) {
        self.inner.set(value);
    }

    /// Update with a function
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        self.inner.update(f);
    }
}

/// useSyncExternalStore hook (for subscribing to external stores)
pub fn use_sync_external_store<T, S>(_subscribe: S, _get_snapshot: fn(&S) -> T, _get_server_snapshot: fn() -> T) -> T
where
    T: Clone + 'static,
    S: 'static,
{
    // TODO: Implement proper external store subscription
    todo!("useSyncExternalStore requires external store implementation")
}

/// useId hook (for generating unique IDs)
pub fn use_id() -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("rts-{:x}", id)
}
