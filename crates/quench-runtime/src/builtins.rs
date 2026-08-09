use std::rc::Rc;

use crate::{ops::Builtin, value::Value};

pub(crate) fn property(builtin: Builtin, key: &str) -> Value {
    if matches!(builtin, Builtin::Array) && key == "isArray" {
        return Value::Builtin(Builtin::ArrayIsArray);
    }
    Value::Undefined
}

pub(crate) fn array(arguments: &[Value]) -> Value {
    if arguments.len() == 1 {
        if let Value::Number(length) = arguments[0] {
            if length >= 0.0 && length.fract() == 0.0 {
                return Value::Array(Rc::new(vec![Value::Undefined; length as usize]));
            }
        }
    }
    Value::Array(Rc::new(arguments.to_vec()))
}
