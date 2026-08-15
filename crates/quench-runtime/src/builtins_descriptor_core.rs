pub(crate) fn non_enumerable_descriptor(value: &Value) -> Value {
    Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), value.clone()),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}

pub(crate) fn same_value_zero(left: &Value, right: &Value) -> bool {
    if let (Value::Number(left), Value::Number(right)) = (left, right) {
        return left.is_nan() && right.is_nan() || left == right;
    }
    same_value(Some(left), Some(right))
}

fn same_value_objects(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Array(left), Value::Array(right)) => Rc::ptr_eq(left, right),
        (Value::Object(left), Value::Object(right)) => Rc::ptr_eq(left, right),
        (Value::ObjectAlias(left), Value::Object(right))
        | (Value::Object(right), Value::ObjectAlias(left)) => left
            .0
            .borrow()
            .upgrade()
            .is_some_and(|left| Rc::ptr_eq(&left, right)),
        (Value::ArrayBuffer(left), Value::ArrayBuffer(right)) => Rc::ptr_eq(left, right),
        (Value::DataView(left), Value::DataView(right)) => Rc::ptr_eq(left, right),
        (Value::Float32Array(left), Value::Float32Array(right)) => Rc::ptr_eq(left, right),
        (Value::Float64Array(left), Value::Float64Array(right)) => Rc::ptr_eq(left, right),
        (Value::Int16Array(left), Value::Int16Array(right)) => Rc::ptr_eq(left, right),
        (Value::Int8Array(left), Value::Int8Array(right)) => Rc::ptr_eq(left, right),
        (Value::Int32Array(left), Value::Int32Array(right)) => Rc::ptr_eq(left, right),
        (Value::Uint16Array(left), Value::Uint16Array(right)) => Rc::ptr_eq(left, right),
        (Value::Uint8Array(left), Value::Uint8Array(right)) => Rc::ptr_eq(left, right),
        (Value::Uint8ClampedArray(left), Value::Uint8ClampedArray(right)) => {
            Rc::ptr_eq(left, right)
        }
        (Value::Uint32Array(left), Value::Uint32Array(right)) => Rc::ptr_eq(left, right),
        (Value::Function(left), Value::Function(right)) => Rc::ptr_eq(left, right),
        (Value::Generator(left), Value::Generator(right)) => Rc::ptr_eq(left, right),
        (Value::BoundFunction(left), Value::BoundFunction(right)) => Rc::ptr_eq(left, right),
        (Value::StringUnits(_), Value::String(_)) | (Value::String(_), Value::StringUnits(_)) => {
            crate::strings::units_equal(left, right)
        }
        _ => left == right,
    }
}
