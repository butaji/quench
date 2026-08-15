//! Map and Set collections — canonical JS semantics for insertion-order collections.

pub mod map;
pub mod set;
include!("collections_set_methods.rs");

use crate::{execute::VmError, ops::Builtin, value::Value};

pub(crate) fn execute_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    if is_set_relation(builtin) {
        return Some(set::set_relation(builtin, receiver, arguments));
    }
    if let Some(result) = execute_weak(builtin, receiver, arguments) {
        return Some(result);
    }
    execute_core(builtin, receiver, arguments)
}

fn execute_core(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    use Builtin::*;
    if let Some(result) = execute_iterator_next(builtin, receiver) {
        return Some(result);
    }
    match builtin {
        IteratorConcat => Some(iterator_concat(arguments)),
        IteratorFrom => Some(iterator_from(arguments)),
        IteratorReturn => Some(iterator::return_(receiver, arguments)),
        IteratorPrototypeConstructorGetter => Some(Ok(Value::Builtin(Builtin::Iterator))),
        IteratorPrototypeConstructorSetter => {
            Some(iterator_accessor_setter(receiver, arguments, "constructor"))
        }
        IteratorPrototypeToStringTagGetter => Some(Ok(Value::String("Iterator".into()))),
        IteratorPrototypeToStringTagSetter => Some(iterator_accessor_setter(
            receiver,
            arguments,
            "Symbol.toStringTag",
        )),
        IteratorToArray => Some(iterator_to_array(receiver)),
        Map => Some(constructor_requires_new("Map")),
        MapGroupBy => Some(map::map_group_by(arguments)),
        MapGetOrInsert => Some(map::map_get_or_insert(receiver, arguments)),
        MapGetOrInsertComputed => Some(map::map_get_or_insert_computed(receiver, arguments)),
        Set => Some(constructor_requires_new("Set")),
        MapSet => Some(map::map_set(receiver, arguments)),
        MapSizeGetter => Some(map::map_size(receiver)),
        MapGet => Some(map::map_get(receiver, arguments)),
        MapHas => Some(map::map_has(receiver, arguments)),
        MapDelete => Some(map::map_delete(receiver, arguments)),
        MapClear => Some(map::map_clear(receiver)),
        MapForEach => Some(map::map_for_each(receiver, arguments)),
        SetAdd => Some(set::set_add(receiver, arguments)),
        SetSizeGetter => Some(set::set_size(receiver)),
        SetHas => Some(set::set_has(receiver, arguments)),
        SetDelete => Some(set::set_delete(receiver, arguments)),
        SetClear => Some(set::set_clear(receiver)),
        SetForEach => Some(set::set_for_each(receiver, arguments)),
        MapIterator | MapEntries => Some(iterator::from_map(receiver)),
        MapKeys => Some(iterator::from_map_keys(receiver)),
        MapValues => Some(iterator::from_map_values(receiver)),
        SetIterator => Some(iterator::from_set(receiver)),
        SetEntries => Some(iterator::from_set_entries(receiver)),
        SetSpeciesGetter | MapSpeciesGetter | SpeciesGetter => Some(set::set_species(receiver)),
        _ => None,
    }
}

fn iterator_accessor_setter(
    receiver: Option<&Value>,
    arguments: &[Value],
    key: &str,
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Iterator accessor called on non-object",
        ));
    };
    if !crate::value::is_object(receiver) {
        return Err(crate::value::error::throw_type_error(
            "Iterator accessor called on non-object",
        ));
    }
    if matches!(receiver, Value::Builtin(Builtin::IteratorPrototype)) {
        return Err(crate::value::error::throw_type_error(
            "Cannot set Iterator prototype accessor",
        ));
    }
    let value = arguments.first().cloned().unwrap_or(Value::Undefined);
    crate::builtins::set_property(receiver.clone(), key, value);
    Ok(Value::Undefined)
}

fn iterator_to_array(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Iterator receiver required",
        ));
    };
    let iterator = iterator::open(receiver.clone())?;
    Ok(Value::array(iterator::collect(&iterator)?))
}

fn iterator_from(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(value) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "Iterator.from requires an argument",
        ));
    };
    if matches!(value, Value::String(_) | Value::StringUnits(_)) {
        return iterator::open(value.clone());
    }
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error(
            "value is not iterable",
        ));
    }
    let method = crate::execute::get_property_result(value, "Symbol.iterator")?;
    if crate::conversion::is_callable(&method) {
        return iterator::open(value.clone());
    }
    if !matches!(method, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "value is not iterable",
        ));
    }
    let next = crate::execute::get_property_result(value, "next")?;
    Ok(Value::Iterator(std::rc::Rc::new(
        crate::value::IteratorData {
            state: std::cell::RefCell::new(crate::value::IteratorState::Protocol {
                iterator: value.clone(),
                next,
                done: false,
                executing: false,
            }),
        },
    )))
}

fn iterator_concat(arguments: &[Value]) -> Result<Value, VmError> {
    let mut items = Vec::with_capacity(arguments.len());
    for value in arguments {
        if !crate::value::is_object(value) {
            return Err(crate::value::error::throw_type_error(
                "Iterator.concat item is not an object",
            ));
        }
        let method = crate::execute::get_property_result(value, "Symbol.iterator")?;
        if !crate::conversion::is_callable(&method) {
            return Err(crate::value::error::throw_type_error(
                "Iterator.concat item is not iterable",
            ));
        }
        items.push((value.clone(), method));
    }
    Ok(Value::Iterator(std::rc::Rc::new(
        crate::value::IteratorData {
            state: std::cell::RefCell::new(crate::value::IteratorState::Concat {
                items,
                index: 0,
                current: None,
                done: false,
            }),
        },
    )))
}

fn execute_iterator_next(
    builtin: Builtin,
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    use Builtin::*;
    Some(match builtin {
        IteratorNext => iterator::next(receiver),
        SetIteratorNext => iterator::next_set(receiver),
        MapIteratorNext => iterator::next_map(receiver),
        IteratorSelf => iterator_self(receiver),
        _ => return None,
    })
}

fn constructor_requires_new(name: &str) -> Result<Value, VmError> {
    Err(crate::value::error::throw_type_error(&format!(
        "Constructor {name} requires 'new'"
    )))
}

fn iterator_self(receiver: Option<&Value>) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn execute_weak(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    Some(match builtin {
        Builtin::WeakMap => Err(crate::value::error::throw_type_error(
            "Constructor WeakMap requires 'new'",
        )),
        Builtin::WeakMapSet => map::weak_map_set(receiver, arguments),
        Builtin::WeakMapGet => map::weak_map_get(receiver, arguments),
        Builtin::WeakMapHas => map::weak_map_has(receiver, arguments),
        Builtin::WeakMapDelete => map::weak_map_delete(receiver, arguments),
        Builtin::WeakMapGetOrInsert | Builtin::WeakMapGetOrInsertComputed => {
            weak_map_extended(builtin, receiver, arguments)?
        }
        Builtin::WeakSet => Err(crate::value::error::throw_type_error(
            "Constructor WeakSet requires 'new'",
        )),
        Builtin::WeakSetAdd => set::weak_set_add(receiver, arguments),
        Builtin::WeakSetHas => set::weak_set_has(receiver, arguments),
        Builtin::WeakSetDelete => set::weak_set_delete(receiver, arguments),
        _ => return None,
    })
}

fn weak_map_extended(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    Some(match builtin {
        Builtin::WeakMapGetOrInsert => map::weak_map_get_or_insert(receiver, arguments),
        Builtin::WeakMapGetOrInsertComputed => {
            map::weak_map_get_or_insert_computed(receiver, arguments)
        }
        _ => return None,
    })
}

pub(crate) mod iterator;
