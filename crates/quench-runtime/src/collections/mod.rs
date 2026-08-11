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
        MapIterator => Some(Ok(iterator::from_map(receiver))),
        SetIterator => Some(Ok(iterator::from_set(receiver))),
        IteratorNext => Some(Ok(iterator::next(receiver))),
        _ => None,
    }
}

pub(crate) mod iterator;
