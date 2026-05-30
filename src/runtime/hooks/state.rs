use super::*;

#[allow(dead_code)]
pub fn use_state<T: Clone + 'static + Send + Sync>(initial: T) -> UseStateResult<T> {
    UseStateResult::new(initial)
}

#[allow(dead_code)]
pub fn use_state_with<T, F>(initial: F) -> UseStateResult<T>
where
    T: Clone + 'static + Send + Sync,
    F: FnOnce() -> T,
{
    UseStateResult::new(initial())
}
