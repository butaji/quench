use super::{descriptor, execute_special};
use crate::{
    execute::{execute_builtin_with_receiver, VmError},
    ops::Builtin,
    value::{FunctionValue, ObjectData, Value},
};
use std::{cell::RefCell, rc::Rc};

#[test]
fn binding_cell_property_mutates_without_escaping() {
    let cell = Rc::new(RefCell::new(Value::Number(1.0)));
    let binding = Value::BindingCell(Rc::clone(&cell));
    let metadata = Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), binding.clone()),
        ("writable".to_string(), Value::Boolean(true)),
    ])));
    let object = Value::Object(Rc::new(ObjectData::new(vec![
        ("x".to_string(), binding),
        (crate::builtins::descriptor_key("x"), metadata),
    ])));
    let updated = crate::builtins::set_property(object.clone(), "x", Value::Number(2.0));
    assert!(matches!((&object, &updated), (Value::Object(a), Value::Object(b)) if Rc::ptr_eq(a, b)));
    assert_eq!(*cell.borrow(), Value::Number(2.0));
    let result = descriptor(Some(&updated), Some(&Value::String("x".to_string()))).unwrap();
    assert_eq!(crate::execute::get_property(&result, "value"), Value::Number(2.0));
}

#[test]
fn static_has_own_uses_first_argument_as_target() {
    let result = execute_special(
        Builtin::ObjectHasOwnProperty,
        None,
        &[Value::Builtin(Builtin::Object), Value::String("hasOwn".to_string())],
    )
    .unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn has_own_throws_on_nullish_target() {
    let error = execute_builtin_with_receiver(
        Builtin::ObjectHasOwnProperty,
        &[Value::Null, Value::String("x".to_string())],
        None,
    )
    .unwrap_err();
    assert!(matches!(error, VmError::Thrown(_)));
}
