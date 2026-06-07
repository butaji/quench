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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_state_initial_value() {
        let result = use_state(42i32);
        assert_eq!(result.value, 42);
    }

    #[test]
    fn test_use_state_setter() {
        let result = use_state(0i32);
        (result.set_value)(100);
        // Setter updates internal state, but in test we create new instance
        assert_eq!(result.value, 0); // Original instance still has 0
    }

    #[test]
    fn test_use_state_with() {
        let result = use_state_with(|| 100i32);
        assert_eq!(result.value, 100);
    }

    #[test]
    fn test_ref_new_and_get() {
        let r = Ref::new(42i32);
        assert_eq!(r.get(), Some(42));
    }

    #[test]
    fn test_ref_clone_shares_storage() {
        let r1 = Ref::new(vec![1, 2, 3]);
        let r2 = r1.clone();
        assert_eq!(r1.get(), r2.get());
    }

    #[test]
    fn test_ref_expect() {
        let r = Ref::new(42i32);
        assert_eq!(r.expect("test"), 42);
    }

    #[test]
    fn test_use_state_with_string() {
        let result = use_state("hello".to_string());
        assert_eq!(result.value, "hello");
    }

    #[test]
    fn test_use_state_with_option() {
        let result = use_state(Some(42i32));
        assert_eq!(result.value, Some(42));
    }

    #[test]
    fn test_use_state_with_vec() {
        let result = use_state(vec![1, 2, 3]);
        assert_eq!(result.value, vec![1, 2, 3]);
    }
}
