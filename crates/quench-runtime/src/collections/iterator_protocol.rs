use super::IteratorData;
use crate::value::Value;

thread_local! {
    static UPDATE_PROTOCOL_RECEIVER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) struct ReceiverUpdateGuard(bool);

impl ReceiverUpdateGuard {
    pub(crate) fn install() -> Self {
        Self(UPDATE_PROTOCOL_RECEIVER.with(|flag| flag.replace(true)))
    }
}

impl Drop for ReceiverUpdateGuard {
    fn drop(&mut self) {
        UPDATE_PROTOCOL_RECEIVER.with(|flag| flag.set(self.0));
    }
}

pub(crate) fn should_update_protocol_receiver() -> bool {
    UPDATE_PROTOCOL_RECEIVER.with(std::cell::Cell::get)
}

pub(super) fn call_next(
    data: &IteratorData,
    next: &Value,
    iterator: &Value,
) -> Result<Value, crate::execute::VmError> {
    if !super::should_update_protocol_receiver() {
        return super::call(next, iterator);
    }
    let (result, updated) = crate::functions::execute_target_with_receiver(next, iterator, &[])?;
    if !same_identity(iterator, &updated) {
        update_receiver(data, updated);
    }
    Ok(result)
}

fn same_identity(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Array(left), Value::Array(right)) => std::rc::Rc::ptr_eq(left, right),
        _ => left == right,
    }
}

fn update_receiver(data: &IteratorData, receiver: Value) {
    let mut state = data.state.borrow_mut();
    if let crate::value::IteratorState::Protocol { iterator, .. } = &mut *state {
        *iterator = receiver;
    }
}
