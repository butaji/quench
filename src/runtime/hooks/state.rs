use super::*;

pub fn use_state<T: Clone + 'static + Send + Sync>(initial: T) -> UseStateResult<T> {
    let idx = next_hook_index();
    UseStateResult::new(initial)
}

pub fn use_state_with<T, F>(initial: F) -> UseStateResult<T>
where T: Clone + 'static + Send + Sync, F: FnOnce() -> T {
    let idx = next_hook_index();
    UseStateResult::new(initial())
}
