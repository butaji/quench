//! Promise built-in module

mod callbacks;
mod constructor;
mod helpers;
mod instance_methods;
mod microtask;
mod static_methods;

// Re-export public APIs
pub(crate) use callbacks::{process_callbacks_sync, queue_callback_on_promise};
pub use constructor::{create_promise_constructor, register_promise};
pub use helpers::{
    create_callback_promise, create_promise_proto, create_resolved_promise,
    create_rejected_promise, get_promise_proto, set_promise_proto,
};
pub use microtask::{execute_pending_microtasks, queue_microtask_impl};
pub use static_methods::{
    promise_all_impl, promise_race_impl, promise_reject_impl_static,
    promise_resolve_impl_static,
};

// Re-export instance method for internal use
pub use instance_methods::promise_then_impl;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::object::PromiseObjectData;
    use crate::value::object::PromiseState;

    #[test]
    fn test_promise_state_pending() {
        let data = PromiseObjectData::new();
        assert_eq!(data.state, PromiseState::Pending);
    }

    #[test]
    fn test_promise_fulfill() {
        let mut data = PromiseObjectData::new();
        data.fulfill(crate::value::Value::Number(42.0));
        assert_eq!(data.state, PromiseState::Fulfilled);
        assert_eq!(data.result, crate::value::Value::Number(42.0));
    }

    #[test]
    fn test_promise_reject() {
        let mut data = PromiseObjectData::new();
        data.reject(crate::value::Value::String("error".to_string()));
        assert_eq!(data.state, PromiseState::Rejected);
        assert_eq!(data.result, crate::value::Value::String("error".to_string()));
    }

    #[test]
    fn test_create_resolved_promise() {
        let promise = create_resolved_promise(crate::value::Value::Number(42.0));
        let obj = promise.borrow();
        if let Some(ref data) = obj.promise_data {
            assert_eq!(data.state, PromiseState::Fulfilled);
            assert_eq!(data.result, crate::value::Value::Number(42.0));
        }
    }

    #[test]
    fn test_create_rejected_promise() {
        let promise = create_rejected_promise(crate::value::Value::String("error".to_string())).unwrap();
        let obj = promise.borrow();
        if let Some(ref data) = obj.promise_data {
            assert_eq!(data.state, PromiseState::Rejected);
            assert_eq!(data.result, crate::value::Value::String("error".to_string()));
        }
    }
}
