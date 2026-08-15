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

fn execute_iterator_next(
    builtin: Builtin,
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    use Builtin::*;
    Some(match builtin {
        IteratorNext => iterator::next(receiver),
        SetIteratorNext => iterator::next_set(receiver),
        MapIteratorNext => iterator::next_map(receiver),
        IteratorSelf | AsyncIteratorSelf => iterator_self(receiver),
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
