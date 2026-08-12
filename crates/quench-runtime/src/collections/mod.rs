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
    use Builtin::*;
    if is_set_relation(builtin) {
        return Some(set::set_relation(builtin, receiver, arguments));
    }
    if let Some(result) = execute_weak(builtin, receiver, arguments) {
        return Some(result);
    }
    match builtin {
        Map => Some(constructor_receiver(receiver).and_then(|_| map::map_new(arguments))),
        MapGroupBy => Some(map::map_group_by(arguments)),
        MapGetOrInsert => Some(map::map_get_or_insert(receiver, arguments)),
        MapGetOrInsertComputed => Some(map::map_get_or_insert_computed(receiver, arguments)),
        Set => Some(constructor_receiver(receiver).and_then(|_| set::set_new(arguments))),
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
        IteratorNext => Some(Ok(iterator::next(receiver))),
        IteratorSelf => Some(iterator_self(receiver)),
        _ => None,
    }
}

fn iterator_self(receiver: Option<&Value>) -> Result<Value, VmError> {
    receiver
        .filter(|value| matches!(value, Value::Iterator(_)))
        .cloned()
        .ok_or_else(|| crate::value::error::throw_type_error("not an iterator"))
}

fn execute_weak(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    Some(match builtin {
        Builtin::WeakMap => {
            constructor_receiver(receiver).and_then(|_| map::weak_map_new(arguments))
        }
        Builtin::WeakMapSet => map::weak_map_set(receiver, arguments),
        Builtin::WeakMapGet => map::weak_map_get(receiver, arguments),
        Builtin::WeakMapHas => map::weak_map_has(receiver, arguments),
        Builtin::WeakMapDelete => map::weak_map_delete(receiver, arguments),
        Builtin::WeakMapGetOrInsert | Builtin::WeakMapGetOrInsertComputed => {
            weak_map_extended(builtin, receiver, arguments)?
        }
        Builtin::WeakSet => {
            constructor_receiver(receiver).and_then(|_| set::weak_set_new(arguments))
        }
        Builtin::WeakSetAdd => set::weak_set_add(receiver, arguments),
        Builtin::WeakSetHas => set::weak_set_has(receiver, arguments),
        Builtin::WeakSetDelete => set::weak_set_delete(receiver, arguments),
        _ => return None,
    })
}

fn constructor_receiver(receiver: Option<&Value>) -> Result<(), VmError> {
    receiver.map(|_| ()).ok_or_else(crate::vm::not_callable)
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
