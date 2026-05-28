use super::*;

pub fn use_memo<T, F, D>(_factory: F, _deps: &[D]) -> T
where T: Clone + 'static, F: FnOnce() -> T, D: std::hash::Hash + Sized { todo!() }

pub fn use_callback<F, D>(callback: F, _deps: &[D]) -> F
where F: 'static, D: std::hash::Hash + Sized { callback }

pub fn use_reducer<S, A, R>(_reducer: R, _initial: S) -> ReducerResult<S, A>
where S: 'static + Clone, A: 'static, R: Fn(S, A) -> S { ReducerResult { state: _initial, dispatch: Rc::new(|_| {}) } }
