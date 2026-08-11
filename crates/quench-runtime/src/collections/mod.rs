//! Map and Set collections — canonical JS semantics for insertion-order collections.

pub mod map;
pub mod set;

use crate::{execute::VmError, ops::Builtin, value::Value};

pub(crate) fn execute_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    use Builtin::*;
    match builtin {
        Map => Some(Ok(map::map_new(arguments))),
        Set => Some(Ok(set::set_new(arguments))),
        MapSet => Some(map::map_set(receiver, arguments)),
        MapGet => Some(map::map_get(receiver, arguments)),
        MapHas => Some(map::map_has(receiver, arguments)),
        MapDelete => Some(map::map_delete(receiver, arguments)),
        MapClear => Some(map::map_clear(receiver)),
        MapForEach => Some(map::map_for_each(receiver, arguments)),
        SetAdd => Some(set::set_add(receiver, arguments)),
        SetHas => Some(set::set_has(receiver, arguments)),
        SetDelete => Some(set::set_delete(receiver, arguments)),
        SetClear => Some(set::set_clear(receiver)),
        SetForEach => Some(set::set_for_each(receiver, arguments)),
        MapIterator => Some(iterator::from_map(receiver)),
        MapEntries => Some(iterator::from_map(receiver)),
        MapKeys => Some(iterator::from_map_keys(receiver)),
        MapValues => Some(iterator::from_map_values(receiver)),
        SetIterator => Some(iterator::from_set(receiver)),
        WeakMap => Some(Ok(map::weak_map_new(arguments))),
        WeakMapSet => Some(map::map_set(receiver, arguments)),
        WeakMapGet => Some(map::map_get(receiver, arguments)),
        WeakMapHas => Some(map::map_has(receiver, arguments)),
        WeakMapDelete => Some(map::map_delete(receiver, arguments)),
        WeakSet => Some(set::weak_set_new(arguments)),
        WeakSetAdd => Some(set::weak_set_add(receiver, arguments)),
        WeakSetHas => Some(set::weak_set_has(receiver, arguments)),
        WeakSetDelete => Some(set::weak_set_delete(receiver, arguments)),
        IteratorNext => Some(Ok(iterator::next(receiver))),
        _ => None,
    }
}

pub(crate) mod iterator;
