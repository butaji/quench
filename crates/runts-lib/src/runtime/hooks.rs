//! Preact-compatible hooks implementation
//!
//! This module provides React/Preact-compatible hooks for use in
//! components. Hooks must be called in the same order on every render.
//!
//! Note: Some types have `#[allow(dead_code)]` because they are provided
//! for API compatibility and may be used in future client-side hydration.

use std::sync::Arc;
use std::marker::PhantomData;
use parking_lot::RwLock;
#[allow(unused_imports)]
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

// Re-export signals
pub use super::signals::Signal;
pub use super::signals::Computed;
pub use super::signals::signal;
pub use super::signals::computed;
pub use super::signals::batch;

/// State hook result type
#[allow(dead_code)]
pub type UseStateResult<T> = (T, Arc<dyn Fn(T) + Send + Sync>);

/// useState hook
///
/// Creates a reactive state value that persists across renders.
pub fn use_state<T, F>(initial: F) -> UseStateResult<T>
where
    T: Clone + Send + Sync + 'static,
    F: FnOnce() -> T + Clone + Send + Sync + 'static,
{
    let initial_value = initial();
    
    let state = Arc::new(RwLock::new(initial_value));
    let state_clone = state.clone();
    
    let setter: Arc<dyn Fn(T) + Send + Sync> = Arc::new(move |new_value: T| {
        *state_clone.write() = new_value;
    });
    
    let getter: T = state.read().clone();
    (getter, setter)
}

/// Ref wrapper type
#[allow(dead_code)]
pub struct Ref<T> {
    inner: Arc<RwLock<Option<T>>>,
}

impl<T: Clone> Ref<T> {
    /// Get current value
    pub fn get(&self) -> Option<T> {
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

/// Memoized value wrapper
#[allow(dead_code)]
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
#[allow(dead_code)]
fn hash_deps<T: Hash>(deps: &[T]) -> usize {
    let mut hasher = DefaultHasher::new();
    for dep in deps {
        dep.hash(&mut hasher);
    }
    hasher.finish() as usize
}

/// useMemo hook
pub fn use_memo<T, F, D>(factory: F, _deps: &[D]) -> T
where
    T: Clone + 'static,
    F: FnOnce() -> T,
    D: Hash + 'static,
{
    factory()
}

/// Callback memoization wrapper
#[allow(dead_code)]
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
pub fn use_callback<F, D>(callback: F, _deps: &[D]) -> F
where
    F: Clone + 'static,
    D: Hash + 'static,
{
    callback
}

/// Reducer result type
#[allow(dead_code)]
pub type ReducerResult<S, A> = (S, Arc<dyn Fn(A) + Send + Sync>);

/// useReducer hook
pub fn use_reducer<S, A, R>(reducer: R, initial: S) -> ReducerResult<S, A>
where
    S: Clone + Send + Sync + 'static,
    A: Send + Sync + 'static,
    R: Fn(S, A) -> S + Clone + Send + Sync + 'static,
{
    let state = Arc::new(RwLock::new(initial));
    let state_clone = state.clone();
    let reducer_clone = reducer.clone();
    
    let dispatch: Arc<dyn Fn(A) + Send + Sync> = Arc::new(move |action: A| {
        let current = state_clone.read().clone();
        let new_state = reducer_clone(current, action);
        *state_clone.write() = new_state;
    });
    
    let getter: S = state.read().clone();
    (getter, dispatch)
}

/// Effect cleanup function type
#[allow(dead_code)]
pub type EffectCleanup = Box<dyn Fn() + Send + Sync>;

/// Effect callback type
#[allow(dead_code)]
pub type EffectCallback = Box<dyn FnOnce() -> Option<EffectCleanup> + Send + Sync + 'static>;

/// useEffect hook
pub fn use_effect<F, D>(_callback: F, _deps: D)
where
    F: FnOnce() -> Option<EffectCleanup> + Send + Sync + 'static,
    D: AsRef<[usize]> + 'static,
{
    // SSR: effects are not run synchronously
}

/// useLayoutEffect hook
pub fn use_layout_effect<F, D>(_callback: F, _deps: D)
where
    F: FnOnce() -> Option<EffectCleanup> + Send + Sync + 'static,
    D: AsRef<[usize]> + 'static,
{
    // SSR: effects are not run
}

/// Context value wrapper
#[allow(dead_code)]
pub struct Context<T: Send + Sync> {
    value: Arc<dyn std::any::Any + Send + Sync>,
    default_value: Arc<dyn std::any::Any + Send + Sync>,
    _marker: PhantomData<T>,
}

impl<T: Clone + Send + Sync> Clone for Context<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            default_value: self.default_value.clone(),
            _marker: PhantomData,
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
            _marker: PhantomData,
        }
    }

    /// Get the context value
    pub fn get(&self) -> Option<T> {
        self.value
            .downcast_ref::<T>()
            .cloned()
            .or_else(|| self.default_value.downcast_ref::<T>().cloned())
    }
}

/// createContext - creates a context with a default value
pub fn create_context<T: Clone + Send + Sync + 'static>(default_value: T) -> Context<T> {
    Context::new(default_value)
}

/// useContext hook
pub fn use_context<T: Clone + Send + Sync + 'static>(_context: &Context<T>) -> Option<T> {
    _context.get()
}

/// useDebugValue hook
pub fn use_debug_value<T>(_value: T) {
    // No-op in production builds
}

/// useId hook (for generating unique IDs)
pub fn use_id() -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("rts-{:x}", id)
}
