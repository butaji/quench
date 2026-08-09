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
        MapSet => Some(Ok(map::map_set(receiver, arguments))),
        MapGet => Some(Ok(map::map_get(receiver, arguments))),
        MapHas => Some(Ok(map::map_has(receiver, arguments))),
        MapDelete => Some(Ok(map::map_delete(receiver, arguments))),
        MapClear => Some(Ok(map::map_clear(receiver))),
        MapForEach => Some(map::map_for_each(receiver, arguments)),
        SetAdd => Some(Ok(set::set_add(receiver, arguments))),
        SetHas => Some(Ok(set::set_has(receiver, arguments))),
        SetDelete => Some(Ok(set::set_delete(receiver, arguments))),
        SetClear => Some(Ok(set::set_clear(receiver))),
        SetForEach => Some(set::set_for_each(receiver, arguments)),
        _ => None,
    }
}
