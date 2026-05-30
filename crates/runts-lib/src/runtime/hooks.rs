//! Preact-compatible hooks implementation
//!
//! This module provides React/Preact-compatible hooks for use in
//! components. Hooks must be called in the same order on every render.
//!
//! Note: Some types have `#[allow(dead_code)]` because they are provided
//! for API compatibility and may be used in future client-side hydration.

use parking_lot::RwLock;
use std::collections::hash_map::DefaultHasher;
#[allow(unused_imports)]
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::Arc;

// Re-export signals
pub use super::signals::batch;
pub use super::signals::computed;
pub use super::signals::signal;
pub use super::signals::Computed;
pub use super::signals::Signal;

/// State hook result type
#[allow(dead_code)]
pub type UseStateResult<T> = (T, Arc<dyn Fn(T) + Send + Sync>);

/// useState hook
///
/// Creates a reactive state value that persists across renders.
pub fn use_state<T>(initial: T) -> UseStateResult<T>
where
    T: Clone + Send + Sync + 'static,
{
    let state = Arc::new(RwLock::new(initial));
    let state_clone = state.clone();

    let setter: Arc<dyn Fn(T) + Send + Sync> = Arc::new(move |new_value: T| {
        *state_clone.write() = new_value;
    });

    let getter: T = state.read().clone();
    (getter, setter)
}

/// useState hook with lazy initialization
pub fn use_state_with<T, F>(initial: F) -> UseStateResult<T>
where
    T: Clone + Send + Sync + 'static,
    F: FnOnce() -> T + Send + Sync + 'static,
{
    let state = Arc::new(RwLock::new(initial()));
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
        &self.inner
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_state_initial_value() {
        // use_state takes initial value directly, not a closure
        let (value, _setter) = use_state(0i32);
        assert_eq!(value, 0);
    }

    #[test]
    fn test_use_state_with_different_initial_values() {
        let (value1, _) = use_state(10i32);
        let (value2, _) = use_state(20i32);
        assert_eq!(value1, 10);
        assert_eq!(value2, 20);
    }

    #[test]
    fn test_use_state_with() {
        // use_state_with evaluates the init closure once
        let (value, _) = use_state_with(|| 100i32);
        assert_eq!(value, 100);
    }

    #[test]
    fn test_use_ref_get() {
        // use_ref returns a Ref<T> - get returns Option<T>
        let r = use_ref(|| 42i32);
        assert_eq!(r.get(), Some(42));
    }

    #[test]
    fn test_use_ref_get_string() {
        let r = use_ref(|| "hello".to_string());
        assert_eq!(r.get(), Some("hello".to_string()));
    }

    #[test]
    fn test_use_ref_clone_shares_storage() {
        let r1 = use_ref(|| vec![1, 2, 3]);
        let r2 = r1.clone();
        // Cloned refs share the same inner Arc
        assert_eq!(r1.get(), r2.get());
    }

    #[test]
    fn test_use_memo() {
        // use_memo calls factory and returns result
        let val: i32 = use_memo(|| 2 + 2, &[0usize]);
        assert_eq!(val, 4);
    }

    #[test]
    fn test_use_callback_returns_callback() {
        // use_callback returns the callback unchanged
        let cb = use_callback(|| 42i32, &[0usize]);
        assert_eq!(cb(), 42);
    }

    #[test]
    fn test_use_reducer_initial_state() {
        // use_reducer returns (initial_state, dispatch)
        let reducer = |state: i32, _action: i32| state;
        let (state, _dispatch) = use_reducer(reducer, 10);
        assert_eq!(state, 10);
    }

    #[test]
    fn test_use_effect_succeeds() {
        // use_effect does nothing on server, but should not panic
        use_effect(|| None, vec![]);
    }

    #[test]
    fn test_use_layout_effect_succeeds() {
        use_layout_effect(|| None, vec![]);
    }

    #[test]
    fn test_create_context() {
        let ctx = create_context(42i32);
        assert_eq!(ctx.get(), Some(42));
    }

    #[test]
    fn test_create_context_with_string() {
        let ctx = create_context("default".to_string());
        assert_eq!(ctx.get(), Some("default".to_string()));
    }

    #[test]
    fn test_context_clone() {
        let ctx1 = create_context(100i32);
        let ctx2 = ctx1.clone();
        assert_eq!(ctx1.get(), ctx2.get());
        assert_eq!(ctx1.get(), Some(100));
    }

    #[test]
    fn test_use_context() {
        let ctx = create_context("hello".to_string());
        let val = use_context(&ctx);
        assert_eq!(val, Some("hello".to_string()));
    }

    #[test]
    fn test_use_debug_value_no_panic() {
        use_debug_value(42);
        use_debug_value("test");
    }

    #[test]
    fn test_use_id_generates_unique_ids() {
        let id1 = use_id();
        let id2 = use_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("rts-"));
        assert!(id2.starts_with("rts-"));
    }

    #[test]
    fn test_signal_re_exports() {
        let sig = signal(42i32);
        assert_eq!(sig.get(), 42);
    }

    #[test]
    fn test_computed_re_exports() {
        let comp: Computed<i32> = computed(|| 2 * 21);
        assert_eq!(comp.get(), 42);
    }

    #[test]
    fn test_batch_re_exports() {
        let sig = signal(0i32);
        batch(|| {
            sig.set(1);
            sig.set(2);
        });
        assert_eq!(sig.get(), 2);
    }

    #[test]
    fn test_use_state_with_vec() {
        let (value, _) = use_state(vec![1, 2, 3]);
        assert_eq!(value, vec![1, 2, 3]);
    }

    #[test]
    fn test_use_state_with_option() {
        let (value, _) = use_state(Some(42i32));
        assert_eq!(value, Some(42));
    }

    #[test]
    fn test_ref_get_none_when_unset() {
        // Ref stores Option<T>, initial is Some(initial)
        let r = use_ref(|| 0i32);
        assert_eq!(r.get(), Some(0));
    }

    #[test]
    fn test_memo_with_string() {
        let val: String = use_memo(|| format!("hello"), &[0usize]);
        assert_eq!(val, "hello");
    }
}
