//! Signals implementation

#[derive(Clone)]
pub struct Signal {
    inner: std::rc::Rc<std::cell::RefCell<f64>>,
}

impl Signal {
    pub fn new(val: f64) -> Self {
        Self {
            inner: std::rc::Rc::new(std::cell::RefCell::new(val)),
        }
    }
    pub fn get(&self) -> f64 {
        *self.inner.borrow()
    }
    pub fn set(&self, val: f64) {
        *self.inner.borrow_mut() = val;
    }
}

#[derive(Clone)]
pub struct Computed {
    inner: std::rc::Rc<std::cell::RefCell<Option<f64>>>,
}

impl Computed {
    pub fn new(_f: impl Fn() -> f64) -> Self {
        Self {
            inner: std::rc::Rc::new(std::cell::RefCell::new(None)),
        }
    }
    pub fn get(&self) -> f64 {
        self.inner.borrow().unwrap_or(0.0)
    }
}
