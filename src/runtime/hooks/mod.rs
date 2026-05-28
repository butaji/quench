//! Preact hooks runtime

pub mod context;
pub mod effect;
pub mod memo;
pub mod state;

pub use context::*;
pub use effect::*;
pub use memo::*;
pub use state::*;

#[derive(Clone)]
pub struct Ref<T: Clone> {
    inner: std::rc::Rc<std::cell::RefCell<Option<T>>>,
}

impl<T: Clone> Ref<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: std::rc::Rc::new(std::cell::RefCell::new(Some(value))),
        }
    }
    pub fn get(&self) -> T {
        self.inner
            .borrow()
            .clone()
            .unwrap_or_else(|| panic!("Ref not initialized"))
    }
}

#[derive(Clone)]
pub struct UseStateResult<T: Clone> {
    pub value: T,
    pub set_value: std::sync::Arc<dyn Fn(T) + Send + Sync>,
}

impl<T: Clone + Send + Sync + 'static> UseStateResult<T> {
    pub fn new(value: T) -> Self {
        let setter = std::sync::Arc::new(move |_new_val: T| {});
        Self {
            value,
            set_value: setter,
        }
    }
}

pub fn reset_hook_index() {}
pub fn flush_effects() {}
