pub(crate) fn from_map(receiver: Option<&Value>) -> Value {
    let Some(Value::Map(data)) = receiver else {
        return empty();
    };
    let values = data
        .keys
        .iter()
        .zip(&data.values)
        .map(|(key, value)| Value::array(vec![key.clone(), value.clone()]))
        .collect();
    make(values)
}

pub(crate) fn from_set(receiver: Option<&Value>) -> Value {
    let Some(Value::Set(data)) = receiver else {
        return empty();
    };
    make(data.values.iter().cloned().collect())
}

pub(crate) fn next(receiver: Option<&Value>) -> Value {
    let Some(iterator @ Value::Iterator(_)) = receiver else {
        return result(Value::Undefined, true);
    };
    match step_value(iterator) {
        Ok(Some(value)) => result(value, false),
        Ok(None) => result(Value::Undefined, true),
        Err(_) => result(Value::Undefined, true),
    }
}

pub(crate) fn property(key: &str) -> Value {
    match key {
        "next" => Value::Builtin(crate::ops::Builtin::IteratorNext),
        _ => Value::Undefined,
    }
}

pub(crate) fn make(values: Vec<Value>) -> Value {
    Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Native {
            values,
            index: 0,
            done: false,
        }),
    }))
}

fn make_protocol(iterator: Value, next: Value) -> Value {
    Value::Iterator(Rc::new(IteratorData {
        state: RefCell::new(IteratorState::Protocol {
            iterator,
            next,
            done: false,
        }),
    }))
}

fn empty() -> Value {
    make(Vec::new())
}

fn result(value: Value, done: bool) -> Value {
    Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value),
        ("done".to_string(), Value::Boolean(done)),
    ])))
}
