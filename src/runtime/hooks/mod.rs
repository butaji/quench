//! Preact hooks runtime

pub mod context;
pub mod effect;
pub mod memo;
pub mod state;


#[allow(dead_code)]
#[derive(Clone)]
pub struct Ref<T: Clone> {
    inner: std::rc::Rc<std::cell::RefCell<Option<T>>>,
}

#[allow(dead_code)]
impl<T: Clone> Ref<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: std::rc::Rc::new(std::cell::RefCell::new(Some(value))),
        }
    }
    pub fn get(&self) -> Option<T> {
        self.inner.borrow().clone()
    }

    pub fn expect(&self, msg: &str) -> T {
        self.inner.borrow().clone().expect(msg)
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
pub fn reset_hook_index() {}
#[allow(dead_code)]
pub fn flush_effects() {}
