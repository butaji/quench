#[cfg(test)]
mod tests {
    use super::{drain_microtasks, new_promise, promise_then, reject_promise, resolve_promise};
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
}
