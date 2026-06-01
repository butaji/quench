use super::*;
use std::sync::{Arc, RwLock};

/// Hook state storage - persists across renders within same component tree
thread_local! {
    static HOOK_STATES: RwLock<Vec<Box<dyn std::any::Any + Send + Sync>>> = RwLock::new(Vec::new());
}

/// Reset hook state for new render cycle
#[allow(dead_code)]
pub fn reset_hook_index() {
    HOOK_STATES.with(|s| s.write().unwrap().clear());
}

/// Flush pending effects (no-op in SSR)
#[allow(dead_code)]
pub fn flush_effects() {}

#[allow(dead_code)]
pub struct UseStateResult<T: Clone> {
    pub value: T,
    pub set_value: Arc<dyn Fn(T) + Send + Sync>,
}

impl<T: Clone + Send + Sync + 'static> UseStateResult<T> {
    #[allow(dead_code)]
    pub fn new(initial: T) -> Self {
        let storage = Arc::new(RwLock::new(initial));
        let storage_clone = storage.clone();
        let setter = Arc::new(move |new_val: T| {
            *storage_clone.write().unwrap() = new_val;
        }) as Arc<dyn Fn(T) + Send + Sync>;
        let value = storage.read().unwrap().clone();
        UseStateResult {
            value,
            set_value: setter,
        }
    }
}

#[allow(dead_code)]
pub fn use_state<T: Clone + Send + Sync + 'static>(initial: T) -> UseStateResult<T> {
    use_state_with(|| initial)
}

#[allow(dead_code)]
pub fn use_state_with<T, F>(initial: F) -> UseStateResult<T>
where
    T: Clone + Send + Sync + 'static,
    F: FnOnce() -> T,
{
    UseStateResult::new(initial())
}
