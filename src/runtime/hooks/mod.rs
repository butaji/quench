//! Preact hooks runtime

pub mod state;
pub mod effect;
pub mod memo;
pub mod context;

pub use state::*;
pub use effect::*;
pub use memo::*;
pub use context::*;

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    static HOOK_STATE: RefCell<HookState> = RefCell::new(HookState::default());
}

#[derive(Default)]
struct HookState {
    index: usize,
    entries: Vec<Rc<RefCell<Option<Rc<dyn Any + Send + Sync>>>>>,
    effect_queue: Vec<QueuedEffect>,
    context_providers: HashMap<usize, Rc<RefCell<Option<Rc<dyn Any + Send + Sync>>>>>,
}

struct HookEntry { kind: HookKind, hash: usize, value: Rc<RefCell<Option<Rc<dyn Any + Send + Sync>>>> }
struct QueuedEffect { callback: Rc<dyn Fn()>, deps: Vec<usize> }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HookKind { State, Ref, Memo, Callback, Reducer, Effect, LayoutEffect, Context }

pub fn reset_hook_index() { HOOK_STATE.with(|s| { s.borrow_mut().index = 0; }); }
pub fn flush_effects() { HOOK_STATE.with(|s| { let mut state = s.borrow_mut(); for effect in state.effect_queue.drain(..) { (effect.callback)(); } }); }

fn next_hook_index() -> usize { HOOK_STATE.with(|s| { let mut state = s.borrow_mut(); let idx = state.index; state.index += 1; idx }) }

fn with_hook_state<T, F>(f: F) -> T where F: FnOnce(&mut HookState) -> T {
    HOOK_STATE.with(|s| { let mut state = s.borrow_mut(); f(&mut state) })
}

fn init_hook<T: Any + Send + Sync>(value: T, kind: HookKind, hash: usize) -> usize {
    with_hook_state(|state| {
        let idx = state.index;
        state.index += 1;
        while state.entries.len() <= idx { state.entries.push(Rc::new(RefCell::new(None))); }
        let entry = Rc::new(RefCell::new(Some(Rc::new(value) as Rc<dyn Any + Send + Sync>)));
        state.entries[idx] = entry;
        idx
    })
}

fn read_hook<T: Clone + Any + Send + Sync + 'static>(idx: usize) -> T {
    HOOK_STATE.with(|s| {
        let state = s.borrow();
        if idx < state.entries.len() { if let Some(ref cell) = *state.entries[idx].borrow() { if let Some(v) = cell.downcast_ref::<T>() { return v.clone(); } } }
        panic!("Hook at index {} not initialized", idx)
    })
}

fn write_hook<T: Any + Send + Sync>(idx: usize, value: T) {
    HOOK_STATE.with(|s| {
        let mut state = s.borrow_mut();
        if idx < state.entries.len() { *state.entries[idx].borrow_mut() = Some(Rc::new(value) as Rc<dyn Any + Send + Sync>); }
    });
}

fn hash_deps(deps: &[impl std::hash::Hash + Sized]) -> usize {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut h = DefaultHasher::new();
    for d in deps { d.hash(&mut h); }
    h.finish() as usize
}

#[derive(Clone)]
pub struct Ref<T> { inner: Rc<RefCell<Option<T>>> }
impl<T> Ref<T> { pub fn new(value: T) -> Self { Self { inner: Rc::new(RefCell::new(Some(value))) } } pub fn get(&self) -> T where T: Clone { self.inner.borrow().as_ref().cloned().unwrap_or_default() } pub fn set(&self, value: T) { *self.inner.borrow_mut() = Some(value); } }

pub struct UseStateResult<T> { pub value: T, pub set_value: Rc<dyn Fn(T)> }
impl<T: 'static + Clone + Send + Sync> UseStateResult<T> {
    pub fn new(value: T) -> Self { let value = Rc::new(std::sync::Mutex::new(value)); let value_clone = Rc::clone(&value); Self { value: value.lock().unwrap().clone(), set_value: Rc::new(move |v| { *value_clone.lock().unwrap() = v; }) } }
}

pub struct ReducerResult<S, A> { pub state: S, pub dispatch: Rc<dyn Fn(A)> }
