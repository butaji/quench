pub(crate) fn from_map(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Map iterator called on incompatible receiver",
        ));
    };
    Ok(make_map(Rc::clone(data), 0))
}

pub(crate) fn from_map_keys(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Map iterator called on incompatible receiver",
        ));
    };
    Ok(make_map(Rc::clone(data), 1))
}

pub(crate) fn from_map_values(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Map(data)) =
        receiver.filter(|value| matches!(value, Value::Map(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Map iterator called on incompatible receiver",
        ));
    };
    Ok(make_map(Rc::clone(data), 2))
}

pub(crate) fn from_set(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Set(data)) =
        receiver.filter(|value| matches!(value, Value::Set(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Set iterator called on incompatible receiver",
        ));
    };
    Ok(make_set(Rc::clone(data), 0))
}

pub(crate) fn from_set_entries(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Set(data)) =
        receiver.filter(|value| matches!(value, Value::Set(data) if !data.weak))
    else {
        return Err(crate::value::error::throw_type_error(
            "Set iterator called on incompatible receiver",
        ));
    };
    Ok(make_set(Rc::clone(data), 1))
}

pub(crate) fn next(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    let Some(iterator @ Value::Iterator(data)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Iterator.prototype.next called on incompatible receiver",
        ));
    };
    if is_helper_iter(data) {
        if *data.executing.borrow() || *data.in_return.borrow() {
            return Err(crate::value::error::throw_type_error(
                "Iterator is already executing",
            ));
        }
        *data.executing.borrow_mut() = true;
        let stepped = step_value(iterator);
        *data.executing.borrow_mut() = false;
        return match stepped {
            Ok(Some(value)) => Ok(result(value, false)),
            Ok(None) => Ok(result(Value::Undefined, true)),
            Err(error) => Err(error),
        };
    }
    match step_value(iterator) {
        Ok(Some(value)) => Ok(result(value, false)),
        Ok(None) => Ok(result(Value::Undefined, true)),
        Err(error) => Err(error),
    }
}

fn is_helper_iter(data: &crate::value::IteratorData) -> bool {
    matches!(
        &*data.state.borrow(),
        IteratorState::Mapped { .. }
            | IteratorState::Filtered { .. }
            | IteratorState::FlatMapped { .. }
            | IteratorState::Dropped { .. }
            | IteratorState::Take { .. }
            | IteratorState::Zip { .. }
            | IteratorState::Concat { .. }
    )
}

pub(crate) fn next_set(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    branded_next(receiver, "Set")
}

pub(crate) fn next_map(receiver: Option<&Value>) -> Result<Value, crate::execute::VmError> {
    branded_next(receiver, "Map")
}

fn branded_next(receiver: Option<&Value>, brand: &str) -> Result<Value, crate::execute::VmError> {
    let branded = matches!(receiver, Some(Value::Iterator(data)) if {
        let state = data.state.borrow();
        match brand {
            "Set" => matches!(&*state, IteratorState::Set { .. }),
            _ => matches!(&*state, IteratorState::Map { .. }),
        }
    });
    if !branded {
        return Err(crate::value::error::throw_type_error(
            "Iterator next called on incompatible receiver",
        ));
    }
    next(receiver)
}

pub(crate) fn prototype_of(value: &Value) -> Value {
    let Value::Iterator(data) = value else {
        return Value::Builtin(crate::ops::Builtin::ArrayIteratorPrototype);
    };
    Value::Builtin(builtin_for(data))
}

pub(crate) fn builtin_for(data: &crate::value::IteratorData) -> crate::ops::Builtin {
    match &*data.state.borrow() {
        IteratorState::RegExpString { .. } => crate::ops::Builtin::RegExpStringIteratorPrototype,
        IteratorState::String { .. } => crate::ops::Builtin::StringIteratorPrototype,
        IteratorState::Set { .. } => crate::ops::Builtin::SetIteratorPrototype,
        IteratorState::Map { .. } => crate::ops::Builtin::MapIteratorPrototype,
        IteratorState::Native { .. } | IteratorState::Protocol { .. } => {
            crate::ops::Builtin::ArrayIteratorPrototype
        }
        IteratorState::Mapped { .. } => crate::ops::Builtin::ArrayIteratorPrototype,
        IteratorState::Filtered { .. } => crate::ops::Builtin::ArrayIteratorPrototype,
        IteratorState::FlatMapped { .. } => crate::ops::Builtin::ArrayIteratorPrototype,
        IteratorState::Dropped { .. } => crate::ops::Builtin::ArrayIteratorPrototype,
        IteratorState::Take { .. } => crate::ops::Builtin::ArrayIteratorPrototype,
        IteratorState::Concat { .. } => crate::ops::Builtin::IteratorPrototype,
        IteratorState::Zip { .. } => crate::ops::Builtin::IteratorPrototype,
    }
}

pub(crate) fn property(key: &str) -> Value {
    match key {
        "next" => Value::Builtin(crate::ops::Builtin::IteratorNext),
        "toArray" => Value::Builtin(crate::ops::Builtin::IteratorToArray),
        "map" => Value::Builtin(crate::ops::Builtin::IteratorMap),
        "filter" => Value::Builtin(crate::ops::Builtin::IteratorFilter),
        "some" => Value::Builtin(crate::ops::Builtin::IteratorSome),
        "every" => Value::Builtin(crate::ops::Builtin::IteratorEvery),
        "flatMap" => Value::Builtin(crate::ops::Builtin::IteratorFlatMap),
        "drop" => Value::Builtin(crate::ops::Builtin::IteratorDrop),
        "take" => Value::Builtin(crate::ops::Builtin::IteratorTake),
        "reduce" => Value::Builtin(crate::ops::Builtin::IteratorReduce),
        "find" => Value::Builtin(crate::ops::Builtin::IteratorFind),
        "forEach" => Value::Builtin(crate::ops::Builtin::IteratorForEach),
        "Symbol.iterator" => Value::Builtin(crate::ops::Builtin::IteratorSelf),
        _ => Value::Undefined,
    }
}

pub(crate) fn property_for(value: &Value, key: &str) -> Value {
    if key == "next" {
        return next_for(value);
    }
    if key == "return"
        && matches!(
            value,
            Value::Iterator(data)
                if matches!(
                    &*data.state.borrow(),
                    IteratorState::Protocol { .. }
                        | IteratorState::Mapped { .. }
                        | IteratorState::Filtered { .. }
                        | IteratorState::Concat { .. }
                        | IteratorState::Zip { .. }
                )
        )
    {
        return Value::Builtin(crate::ops::Builtin::IteratorReturn);
    }
    if key != "Symbol.toStringTag" {
        return property(key);
    }
    let Value::Iterator(data) = value else {
        return Value::Undefined;
    };
    let tag = match &*data.state.borrow() {
        IteratorState::Native { .. } => "Array Iterator",
        IteratorState::RegExpString { .. } => "RegExp String Iterator",
        IteratorState::String { .. } => "String Iterator",
        IteratorState::Set { .. } => "Set Iterator",
        IteratorState::Map { .. } => "Map Iterator",
        IteratorState::Protocol { .. } => "Iterator",
        IteratorState::Mapped { .. } => "Iterator",
        IteratorState::Filtered { .. } => "Iterator",
        IteratorState::FlatMapped { .. } => "Iterator",
        IteratorState::Dropped { .. } => "Iterator",
        IteratorState::Take { .. } => "Iterator",
        IteratorState::Concat { .. } => "Iterator",
        IteratorState::Zip { .. } => "Iterator",
    };
    Value::String(tag.to_string())
}

fn next_for(value: &Value) -> Value {
    use crate::ops::Builtin;
    let Value::Iterator(data) = value else {
        return property("next");
    };
    let builtin = match &*data.state.borrow() {
        IteratorState::RegExpString { .. } => Builtin::RegExpStringIteratorNext,
        IteratorState::String { .. } => Builtin::StringIteratorNext,
        IteratorState::Set { .. } => Builtin::SetIteratorNext,
        IteratorState::Map { .. } => Builtin::MapIteratorNext,
        IteratorState::Native { .. } | IteratorState::Protocol { .. } => Builtin::IteratorNext,
        IteratorState::Mapped { .. } => Builtin::IteratorNext,
        IteratorState::Filtered { .. } => Builtin::IteratorNext,
        IteratorState::FlatMapped { .. } => Builtin::IteratorNext,
        IteratorState::Dropped { .. } => Builtin::IteratorNext,
        IteratorState::Take { .. } => Builtin::IteratorNext,
        IteratorState::Concat { .. } => Builtin::IteratorNext,
        IteratorState::Zip { .. } => Builtin::IteratorNext,
    };
    Value::Builtin(builtin)
}

pub(crate) fn make(values: Vec<Value>) -> Value {
    Value::Iterator(Rc::new(IteratorData::new(IteratorState::Native {
        values,
        receiver: None,
        typed_receiver: None,
        typed_keys: false,
        entries: false,
        keys: false,
        index: 0,
        done: false,
    })))
}

pub(crate) fn make_array(data: Rc<crate::value::ArrayData>) -> Value {
    Value::Iterator(Rc::new(IteratorData::new(IteratorState::Native {
        values: Vec::new(),
        receiver: Some(data),
        typed_receiver: None,
        typed_keys: false,
        entries: false,
        keys: false,
        index: 0,
        done: false,
    })))
}

pub(crate) fn make_array_entries(data: Rc<crate::value::ArrayData>) -> Value {
    Value::Iterator(Rc::new(IteratorData::new(IteratorState::Native {
        values: Vec::new(),
        receiver: Some(data),
        typed_receiver: None,
        typed_keys: false,
        entries: true,
        keys: false,
        index: 0,
        done: false,
    })))
}

pub(crate) fn make_typed(value: Value) -> Value {
    Value::Iterator(Rc::new(IteratorData::new(IteratorState::Native {
        values: Vec::new(),
        receiver: None,
        typed_receiver: Some(value),
        typed_keys: false,
        entries: false,
        keys: false,
        index: 0,
        done: false,
    })))
}

pub(crate) fn make_typed_entries(value: Value) -> Value {
    Value::Iterator(Rc::new(IteratorData::new(IteratorState::Native {
        values: Vec::new(),
        receiver: None,
        typed_receiver: Some(value),
        typed_keys: false,
        entries: true,
        keys: false,
        index: 0,
        done: false,
    })))
}

pub(crate) fn make_typed_keys(value: Value) -> Value {
    Value::Iterator(Rc::new(IteratorData::new(IteratorState::Native {
        values: Vec::new(),
        receiver: None,
        typed_receiver: Some(value),
        typed_keys: true,
        entries: false,
        keys: false,
        index: 0,
        done: false,
    })))
}

pub(crate) fn make_array_keys(data: Rc<crate::value::ArrayData>) -> Value {
    Value::Iterator(Rc::new(IteratorData::new(IteratorState::Native {
        values: Vec::new(),
        receiver: Some(data),
        typed_receiver: None,
        typed_keys: false,
        entries: false,
        keys: true,
        index: 0,
        done: false,
    })))
}

pub(crate) fn make_string(input: Vec<u16>) -> Value {
    Value::Iterator(Rc::new(IteratorData::new(IteratorState::String {
        input,
        index: 0,
        done: false,
    })))
}

pub(crate) fn make_regexp_string(
    regexp: Value,
    input: String,
    global: bool,
    unicode: bool,
) -> Value {
    Value::Iterator(Rc::new(IteratorData::new(IteratorState::RegExpString {
        regexp,
        input,
        global,
        unicode,
        done: false,
    })))
}

fn make_set(data: Rc<crate::value::SetData>, kind: u8) -> Value {
    Value::Iterator(Rc::new(IteratorData::new(IteratorState::Set {
        data,
        index: 0,
        kind,
        done: false,
    })))
}

fn make_map(data: Rc<crate::value::MapData>, kind: u8) -> Value {
    Value::Iterator(Rc::new(IteratorData::new(IteratorState::Map {
        data,
        index: 0,
        kind,
        done: false,
    })))
}

pub(crate) fn result(value: Value, done: bool) -> Value {
    Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value),
        ("done".to_string(), Value::Boolean(done)),
    ])))
}
use super::step_value;
use crate::value::{IteratorData, IteratorState, Value};
use std::rc::Rc;
