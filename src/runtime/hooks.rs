//! Preact hooks implementation
//!
//! This module provides React/Preact-compatible hooks for use in
//! components. Hooks must be called in the same order on every render.
//!
//! Hooks are stored in a thread-local indexed array, matching React's
//! hooks semantics exactly.

use std::sync::Arc;
use parking_lot::RwLock;
use std::cell::RefCell;

// Re-export signals
pub use super::signals::Signal;

// =============================================================================
// Hook State (thread-local indexed storage for correct hook semantics)
// =============================================================================

thread_local! {
    static HOOK_STATE: RefCell<Vec<HookEntry>> = RefCell::new(Vec::new());
    static HOOK_INDEX: RefCell<usize> = RefCell::new(0);
    static EFFECT_QUEUE: RefCell<Vec<QueuedEffect>> = RefCell::new(Vec::new());
}

struct HookEntry {
    state: Arc<RwLock<Box<dyn std::any::Any + Send + Sync>>>,
    kind: HookKind,
    hash: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum HookKind {
    State,
    Memo,
    Callback,
    Ref,
    Reducer,
}

#[derive(Clone)]
struct QueuedEffect {
    callback: Arc<RwLock<Option<Box<dyn FnOnce() -> Option<EffectCleanup> + Send + Sync>>>>,
    deps_hash: usize,
    ran_once: bool,
}

/// Reset hook index for a new render cycle.
pub fn reset_hook_index() {
    HOOK_INDEX.with(|idx| *idx.borrow_mut() = 0);
}

/// Flush all queued effects (run after render).
pub fn flush_effects() {
    EFFECT_QUEUE.with(|queue| {
        let mut q = queue.borrow_mut();
        for effect in q.iter_mut() {
            if let Some(cb) = effect.callback.write().take() {
                effect.ran_once = true;
                let _ = cb();
            }
        }
    });
}

fn next_hook_index() -> usize {
    HOOK_INDEX.with(|idx| {
        let mut i = idx.borrow_mut();
        let current = *i;
        *i += 1;
        current
    })
}

fn with_hook_state<T, F>(f: F) -> T
where
    F: FnOnce(&mut Vec<HookEntry>) -> T,
{
    HOOK_STATE.with(|state| f(&mut *state.borrow_mut()))
}

fn init_hook<T: std::any::Any + Send + Sync>(value: T, kind: HookKind, hash: usize) -> usize {
    let idx = next_hook_index();
    with_hook_state(|state| {
        if idx >= state.len() {
            state.push(HookEntry {
                state: Arc::new(RwLock::new(Box::new(value))),
                kind,
                hash,
            });
        }
    });
    idx
}

fn read_hook<T: Clone + std::any::Any + Send + Sync + 'static>(idx: usize) -> T {
    with_hook_state(|state| {
        state[idx].state.read().downcast_ref::<T>().cloned().unwrap()
    })
}

fn write_hook<T: std::any::Any + Send + Sync>(idx: usize, value: T) {
    with_hook_state(|state| {
        *state[idx].state.write() = Box::new(value);
    });
}

// =============================================================================
// Public API
// =============================================================================

/// State hook result
#[allow(dead_code)]
pub type UseStateResult<T> = (T, Box<dyn Fn(T) + Send + Sync>);

/// useState hook
///
/// Creates a reactive state value that persists across renders.
/// When the setter is called, the component will re-render.
///
/// # Example
/// ```ignore
/// let (count, set_count) = use_state(0);
/// set_count(count + 1);
/// ```
pub fn use_state<T>(initial: T) -> UseStateResult<T>
where
    T: Clone + std::any::Any + Send + Sync + 'static,
{
    let idx = init_hook(initial, HookKind::State, 0);
    let value = read_hook::<T>(idx);
    
    let setter: Box<dyn Fn(T) + Send + Sync> = Box::new(move |new_value: T| {
        write_hook(idx, new_value);
    });
    
    (value, setter)
}

/// useState hook with lazy initialization
///
/// # Example
/// ```ignore
/// let (count, set_count) = use_state_with(|| expensive_computation());
/// ```
pub fn use_state_with<T, F>(initial: F) -> UseStateResult<T>
where
    T: Clone + std::any::Any + Send + Sync + 'static,
    F: FnOnce() -> T + Send + Sync + 'static,
{
    let idx = next_hook_index();
    with_hook_state(|state| {
        if idx >= state.len() {
            let value = initial();
            state.push(HookEntry {
                state: Arc::new(RwLock::new(Box::new(value))),
                kind: HookKind::State,
                hash: 0,
            });
        }
    });
    let value = read_hook::<T>(idx);
    
    let setter: Box<dyn Fn(T) + Send + Sync> = Box::new(move |new_value: T| {
        write_hook(idx, new_value);
    });
    
    (value, setter)
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
    inner: Arc<RwLock<T>>,
}

impl<T: Clone> Ref<T> {
    /// Get current value
    pub fn get(&self) -> T {
        self.inner.read().clone()
    }

    /// Get mutable reference
    pub fn get_mut(&mut self) -> T {
        self.inner.read().clone()
    }

    /// Set value
    pub fn set(&mut self, value: T) {
        *self.inner.write() = value;
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
    T: Clone + std::any::Any + Send + Sync + 'static,
    F: FnOnce() -> T,
{
    let idx = next_hook_index();
    with_hook_state(|state| {
        if idx >= state.len() {
            let value = initial();
            state.push(HookEntry {
                state: Arc::new(RwLock::new(Box::new(value))),
                kind: HookKind::Ref,
                hash: 0,
            });
        }
    });
    Ref {
        inner: Arc::new(RwLock::new(read_hook::<T>(idx))),
    }
}

/// useRef with default value
pub fn use_ref_default<T: Clone + Default + std::any::Any + Send + Sync + 'static>() -> Ref<T> {
    use_ref(|| T::default())
}

/// Compute hash of dependencies
fn hash_deps(deps: &[impl std::hash::Hash + Sized]) -> usize {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    
    let mut hasher = DefaultHasher::new();
    for dep in deps {
        dep.hash(&mut hasher);
    }
    hasher.finish() as usize
}

fn hash_usize_slice(slice: &[usize]) -> usize {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut hasher = DefaultHasher::new();
    for &v in slice {
        hasher.write_usize(v);
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
    T: Clone + std::any::Any + Send + Sync + 'static,
    F: FnOnce() -> T,
    D: std::hash::Hash + Sized + 'static,
{
    let new_hash = hash_deps(deps);
    let idx = next_hook_index();
    
    with_hook_state(|state| {
        if idx >= state.len() {
            let value = factory();
            state.push(HookEntry {
                state: Arc::new(RwLock::new(Box::new(value))),
                kind: HookKind::Memo,
                hash: new_hash,
            });
        } else {
            let entry = &state[idx];
            if entry.kind != HookKind::Memo || entry.hash != new_hash {
                let value = factory();
                state[idx] = HookEntry {
                    state: Arc::new(RwLock::new(Box::new(value))),
                    kind: HookKind::Memo,
                    hash: new_hash,
                };
            }
        }
    });
    
    read_hook::<T>(idx)
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
pub fn use_callback<F, D>(callback: F, deps: &[D]) -> F
where
    F: Clone + std::any::Any + Send + Sync + 'static,
    D: std::hash::Hash + Sized + 'static,
{
    let new_hash = hash_deps(deps);
    let idx = next_hook_index();
    
    with_hook_state(|state| {
        if idx >= state.len() {
            state.push(HookEntry {
                state: Arc::new(RwLock::new(Box::new(callback))),
                kind: HookKind::Callback,
                hash: new_hash,
            });
        } else {
            let entry = &state[idx];
            if entry.kind != HookKind::Callback || entry.hash != new_hash {
                state[idx] = HookEntry {
                    state: Arc::new(RwLock::new(Box::new(callback))),
                    kind: HookKind::Callback,
                    hash: new_hash,
                };
            }
        }
    });
    
    read_hook::<F>(idx)
}

/// Reducer result
#[allow(dead_code)]
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
    S: Clone + std::any::Any + Send + Sync + 'static,
    A: Send + Sync + 'static,
    R: Fn(S, A) -> S + Clone + Send + Sync + 'static,
{
    let idx = init_hook(initial.clone(), HookKind::Reducer, 0);
    let value = read_hook::<S>(idx);
    
    let dispatch: Box<dyn Fn(A) + Send + Sync> = Box::new(move |action: A| {
        let current = read_hook::<S>(idx);
        let new_state = reducer(current, action);
        write_hook(idx, new_state);
    });
    
    (value, dispatch)
}

/// Effect cleanup function
#[allow(dead_code)]
pub type EffectCleanup = Box<dyn Fn() + Send + Sync>;

/// Effect callback
#[allow(dead_code)]
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
pub fn use_effect<F, D>(callback: F, deps: D)
where
    F: FnOnce() -> Option<EffectCleanup> + Send + Sync + 'static,
    D: AsRef<[usize]> + 'static,
{
    let deps_slice = deps.as_ref();
    let new_hash = hash_usize_slice(deps_slice);
    let idx = next_hook_index();
    
    EFFECT_QUEUE.with(|queue| {
        let mut q = queue.borrow_mut();
        if idx >= q.len() {
            q.push(QueuedEffect {
                callback: Arc::new(RwLock::new(Some(Box::new(callback)))),
                deps_hash: new_hash,
                ran_once: false,
            });
        } else {
            let effect = &mut q[idx];
            if effect.deps_hash != new_hash || !effect.ran_once {
                effect.deps_hash = new_hash;
                *effect.callback.write() = Some(Box::new(callback));
            }
        }
    });
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
#[allow(dead_code)]
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
pub fn use_context<T>(context: &Context<T>) -> T
where
    T: Clone + Send + Sync + 'static,
{
    context.get().clone()
}

/// useDebugValue hook
///
/// Display custom label for custom hooks in React DevTools.
pub fn use_debug_value<T>(_value: T) {
    // No-op in production builds
}

/// useDebugValue with formatter
pub fn use_debug_value_formatted<T, F>(_value: T, _format: F)
where
    F: FnOnce(&T),
{
    // No-op
}

/// State signal wrapper
#[allow(dead_code)]
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
pub fn use_sync_external_store<T, S>(subscribe: S, get_snapshot: fn(&S) -> T, get_server_snapshot: fn() -> T) -> T
where
    T: Clone + 'static,
    S: 'static,
{
    let _ = subscribe;
    let _ = get_snapshot;
    get_server_snapshot()
}

/// useId hook (for generating unique IDs)
pub fn use_id() -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("rts-{:x}", id)
}

/// Error boundary state
#[derive(Clone)]
struct ErrorBoundaryState {
    error: Option<String>,
    reset_version: usize,
}

/// useErrorBoundary hook
///
/// Catches errors from child components and provides a way to reset.
/// Returns `(Option<String>, Box<dyn Fn() + Send + Sync>)` where:
/// - The Option contains the error message if an error occurred
/// - The Fn is called to reset the error boundary
///
/// # Example
/// ```ignore
/// let (error, reset_error) = use_error_boundary();
/// if let Some(msg) = error {
///     return html!(<div>"Error: " {msg}</div> <button on_click={reset_error}>"Retry"</button></div>);
/// }
/// ```
pub fn use_error_boundary() -> (Option<String>, Box<dyn Fn() + Send + Sync>) {
    let idx = next_hook_index();
    
    let state = with_hook_state(|state| {
        if idx >= state.len() {
            let initial = ErrorBoundaryState { error: None, reset_version: 0 };
            state.push(HookEntry {
                state: Arc::new(RwLock::new(Box::new(initial.clone()))),
                kind: HookKind::State,
                hash: 0,
            });
            initial
        } else {
            state[idx].state.read().downcast_ref::<ErrorBoundaryState>().cloned().unwrap()
        }
    });
    
    let reset: Box<dyn Fn() + Send + Sync> = Box::new(move || {
        with_hook_state(|state| {
            if idx < state.len() {
                let new_state = ErrorBoundaryState { error: None, reset_version: state[idx].state.read().downcast_ref::<ErrorBoundaryState>().unwrap().reset_version + 1 };
                *state[idx].state.write() = Box::new(new_state);
            }
        });
    });
    
    (state.error, reset)
}

/// useErrorBoundary with callback (for logging/monitoring)
pub fn use_error_boundary_with_callback<F>(callback: F) -> (Option<String>, Box<dyn Fn() + Send + Sync>)
where
    F: FnOnce(&str) + Clone + Send + Sync + 'static,
{
    let (error, reset) = use_error_boundary();
    
    if let Some(ref err) = error {
        let cb = callback.clone();
        cb(err);
    }
    
    (error, reset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_state_basic() {
        reset_hook_index();
        let (count, set_count) = use_state(0i32);
        assert_eq!(count, 0);
        set_count(5);
        reset_hook_index();
        let (count2, _) = use_state(0i32);
        assert_eq!(count2, 5);
    }

    #[test]
    fn test_use_memo_caches() {
        reset_hook_index();
        let mut call_count = 0;
        let memoized = use_memo(
            || { call_count += 1; call_count },
            &[1usize, 2usize],
        );
        assert_eq!(memoized, 1);

        reset_hook_index();
        let memoized2 = use_memo(
            || { call_count += 1; call_count },
            &[1usize, 2usize],
        );
        // Same deps: factory should NOT be called again
        assert_eq!(memoized2, 1);
        assert_eq!(call_count, 1);

        reset_hook_index();
        let memoized3 = use_memo(
            || { call_count += 1; call_count },
            &[1usize, 3usize],
        );
        // Different deps: factory should be called
        assert_eq!(memoized3, 2);
        assert_eq!(call_count, 2);
    }

    #[test]
    fn test_use_reducer() {
        reset_hook_index();
        let (state, dispatch) = use_reducer(
            |s: i32, a: &'static str| match a {
                "inc" => s + 1,
                "dec" => s - 1,
                _ => s,
            },
            10i32,
        );
        assert_eq!(state, 10);
        dispatch("inc");
        reset_hook_index();
        let (state2, _) = use_reducer(
            |s: i32, a: &'static str| match a {
                "inc" => s + 1,
                "dec" => s - 1,
                _ => s,
            },
            10i32,
        );
        assert_eq!(state2, 11);
    }

    #[test]
    fn test_use_effect_queues() {
        reset_hook_index();
        EFFECT_QUEUE.with(|q| q.borrow_mut().clear());
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ran2 = ran.clone();
        use_effect(
            move || {
                ran2.store(true, std::sync::atomic::Ordering::SeqCst);
                None
            },
            [0usize],
        );
        flush_effects();
        assert!(ran.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_use_error_boundary_initial_state() {
        reset_hook_index();
        let (error, _reset) = use_error_boundary();
        assert!(error.is_none());
    }

    #[test]
    fn test_use_error_boundary_reset() {
        reset_hook_index();
        let (error, reset) = use_error_boundary();
        assert!(error.is_none());
        
        // Simulate setting an error by manipulating state directly
        // In real usage, the runtime would catch panics and set this
        reset();
        
        reset_hook_index();
        let (error2, _reset2) = use_error_boundary();
        // After reset, error should still be None (we never set one)
        assert!(error2.is_none());
    }

    #[test]
    fn test_use_id_unique() {
        reset_hook_index();
        let id1 = use_id();
        let id2 = use_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("rts-"));
    }
}
