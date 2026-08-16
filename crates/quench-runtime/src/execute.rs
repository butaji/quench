//! VM helpers for executing residual operations.
pub use crate::vm::{
    copy_register, execute as run_vm, execute_builtin_with_receiver, execute_in_place,
    execute_with_context, execute_with_registers, get_property, get_property_result, is_truthy,
    read_register, write_value, VmError,
};

pub fn call(
    function: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, VmError> {
    crate::functions::execute_target(function, receiver, arguments)
}

pub fn set_property(
    target: crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> crate::value::Value {
    crate::builtins::set_property(target, key, value)
}

/// Set an object's prototype through the runtime's ordinary object semantics.
pub fn set_prototype_of(
    target: &crate::value::Value,
    prototype: &crate::value::Value,
) -> Result<crate::value::Value, VmError> {
    crate::builtins::object::set_prototype_of(&[target.clone(), prototype.clone()])
}

/// Set an own property on a callable value without invoking inherited
/// setters. Hosts use this for compatibility metadata such as `super_`.
pub fn set_callable_property(
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> Result<(), VmError> {
    match target {
        crate::value::Value::Function(function) => {
            function
                .properties
                .borrow_mut()
                .retain(|(name, _)| name != key);
            function
                .properties
                .borrow_mut()
                .push((key.to_owned(), value));
            Ok(())
        }
        crate::value::Value::BoundFunction(function) => {
            function
                .properties
                .borrow_mut()
                .retain(|(name, _)| name != key);
            function
                .properties
                .borrow_mut()
                .push((key.to_owned(), value));
            Ok(())
        }
        _ => Err(VmError::NotCallable),
    }
}

/// Publish a host-side replacement for an identity-bearing JavaScript value.
/// Hosts use this when a callback mutates an object through a receiver but
/// the value representation requires replacement rather than interior
/// mutation.
pub fn replace_value(old: &crate::value::Value, new: &crate::value::Value) {
    crate::locals::replace_value(old, new);
}
pub(crate) use crate::vm::{
    execute_completion_in_place, execute_completion_step_in_place, not_callable,
};
