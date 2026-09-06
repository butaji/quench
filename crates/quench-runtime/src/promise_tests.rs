#[cfg(test)]
mod tests {
    use super::{
        drain_microtasks, drain_microtasks_all, has_pending_unhandled_rejections, new_promise,
        promise_then, reject_promise, resolve_promise, take_unhandled_rejections,
    };
    use crate::ops::Builtin;
    use crate::value::{PromiseState, Value};

    fn promise_data(value: &Value) -> &std::rc::Rc<crate::value::PromiseData> {
        match value {
            Value::Promise(promise) => promise,
            _ => panic!("expected promise"),
        }
    }

    #[test]
    fn resolve_sets_result_once() {
        let promise = new_promise();
        let data = promise_data(&promise);

        resolve_promise(data, Value::Number(1.0));
        reject_promise(data, Value::Number(2.0));

        assert_eq!(
            data.state.borrow().clone(),
            PromiseState::Fulfilled(Value::Number(1.0))
        );
        assert_eq!(*data.result.borrow(), Some(Value::Number(1.0)));
    }

    #[test]
    fn then_actions_are_consumed_after_settlement() {
        let promise = new_promise();
        let data = promise_data(&promise).clone();

        promise_then(Some(&promise), &[Value::String(String::from("ok"))]).unwrap();
        assert_eq!(data.then_actions.borrow().len(), 1);

        resolve_promise(&data, Value::Boolean(true));
        drain_microtasks();

        assert!(data.then_actions.borrow().is_empty());
        assert_eq!(
            data.state.borrow().clone(),
            PromiseState::Fulfilled(Value::Boolean(true))
        );
    }

    #[test]
    fn rejection_handler_removes_synchronously_queued_unhandled_entry() {
        take_unhandled_rejections();
        let promise = new_promise();
        let data = promise_data(&promise);
        reject_promise(data, Value::String("boom".into()));
        assert!(has_pending_unhandled_rejections());
        promise_then(
            Some(&promise),
            &[Value::Undefined, Value::Builtin(Builtin::Boolean)],
        )
        .expect("rejection handler installs");
        assert!(!has_pending_unhandled_rejections());
        assert!(take_unhandled_rejections().is_empty());
        drain_microtasks_all();
    }
    #[test]
    fn promise_prototype_is_cached_per_realm() {
        let first = crate::vm::VmContext::isolated();
        let second = crate::vm::VmContext::isolated();
        let first_prototype = crate::vm::with_realm(first.realm(), || {
            let promise = new_promise();
            crate::builtins::object::get_prototype_of(Some(&promise)).unwrap()
        })
        .expect("first realm remains registered");
        let second_prototype = crate::vm::with_realm(second.realm(), || {
            let promise = new_promise();
            crate::builtins::object::get_prototype_of(Some(&promise)).unwrap()
        })
        .expect("second realm remains registered");

        assert_ne!(first_prototype, second_prototype);
        assert_eq!(
            crate::vm::with_realm(first.realm(), || {
                crate::vm::realm_intrinsic(Builtin::PromisePrototype)
            }),
            Some(first_prototype)
        );
        assert_eq!(
            crate::vm::with_realm(second.realm(), || {
                crate::vm::realm_intrinsic(Builtin::PromisePrototype)
            }),
            Some(second_prototype)
        );
    }
}
