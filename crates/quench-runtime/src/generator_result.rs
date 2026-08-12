pub(crate) fn iterator_result(value: Value, done: bool) -> Value {
    Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value),
        ("done".to_string(), Value::Boolean(done)),
    ])))
}
