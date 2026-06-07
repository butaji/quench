//! Memo hooks

use std::hash::Hash;
use std::sync::RwLock;

/// Memoized value with dependency tracking
#[allow(dead_code)]
pub struct Memoized<T> {
    value: std::sync::Arc<RwLock<Option<T>>>,
    deps_hash: std::sync::Arc<RwLock<u64>>,
}

impl<T: Clone> Memoized<T> {
    #[allow(dead_code)]
    pub fn get(&self) -> Option<T> {
        self.value.read().unwrap().clone()
    }
}

/// use_memo hook - returns memoized value based on dependencies.
/// In SSR without lifecycle, computes fresh each time.
#[allow(dead_code)]
pub fn use_memo<T, F, D>(factory: F, _deps: &[D]) -> T
where
    T: Clone + 'static,
    F: FnOnce() -> T,
    D: Hash + Sized + 'static,
{
    factory()
}

/// use_callback hook - returns memoized callback
///
/// Returns the same callback reference if deps haven't changed.
#[allow(dead_code)]
pub fn use_callback<F, D>(callback: F, _deps: &[D]) -> F
where
    F: Clone + 'static,
    D: Hash + Sized + 'static,
{
    // SSR: return callback unchanged
    callback
}

#[allow(dead_code)]
pub struct ReducerResult<S, A> {
    pub state: S,
    pub dispatch: Box<dyn Fn(A) + Send + Sync>,
}

#[allow(dead_code)]
pub fn use_reducer<S, A, R>(reducer: R, initial: S) -> ReducerResult<S, A>
where
    S: Clone + Send + Sync + 'static,
    A: Send + Sync + 'static,
    R: Fn(S, A) -> S + Clone + Send + Sync + 'static,
{
    let state = std::sync::Arc::new(std::sync::RwLock::new(initial));
    let state_clone = state.clone();
    let reducer_clone = reducer.clone();

    let dispatch = Box::new(move |action: A| {
        let current = state_clone.read().unwrap().clone();
        let new_state = reducer_clone(current, action);
        *state_clone.write().unwrap() = new_state;
    }) as Box<dyn Fn(A) + Send + Sync>;

    let state_value = state.read().unwrap().clone();
    ReducerResult {
        state: state_value,
        dispatch,
    }
}
