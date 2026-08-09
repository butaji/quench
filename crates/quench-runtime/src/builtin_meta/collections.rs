//! Map/Set builtin metadata.

use crate::{ops::Builtin, value::Value};

/// Returns the prototype property value for Map/Set builtins.
pub fn collections_property(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    match (builtin, key) {
        (MapPrototype, "set") => Some(Value::Builtin(MapSet)),
        (MapPrototype, "get") => Some(Value::Builtin(MapGet)),
        (MapPrototype, "has") => Some(Value::Builtin(MapHas)),
        (MapPrototype, "delete") => Some(Value::Builtin(MapDelete)),
        (MapPrototype, "clear") => Some(Value::Builtin(MapClear)),
        (MapPrototype, "forEach") => Some(Value::Builtin(MapForEach)),
        (SetPrototype, "add") => Some(Value::Builtin(SetAdd)),
        (SetPrototype, "has") => Some(Value::Builtin(SetHas)),
        (SetPrototype, "delete") => Some(Value::Builtin(SetDelete)),
        (SetPrototype, "clear") => Some(Value::Builtin(SetClear)),
        (SetPrototype, "forEach") => Some(Value::Builtin(SetForEach)),
        _ => None,
    }
}

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::MapSet => Some("Map.prototype.set"),
        Builtin::MapGet => Some("Map.prototype.get"),
        Builtin::MapHas => Some("Map.prototype.has"),
        Builtin::MapDelete => Some("Map.prototype.delete"),
        Builtin::MapClear => Some("Map.prototype.clear"),
        Builtin::MapForEach => Some("Map.prototype.forEach"),
        Builtin::SetAdd => Some("Set.prototype.add"),
        Builtin::SetHas => Some("Set.prototype.has"),
        Builtin::SetDelete => Some("Set.prototype.delete"),
        Builtin::SetClear => Some("Set.prototype.clear"),
        Builtin::SetForEach => Some("Set.prototype.forEach"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::MapSet => Some(1.0),
        Builtin::MapGet | Builtin::MapHas | Builtin::MapDelete => Some(1.0),
        Builtin::MapClear | Builtin::MapForEach => Some(0.0),
        Builtin::SetAdd | Builtin::SetHas | Builtin::SetDelete => Some(1.0),
        Builtin::SetClear | Builtin::SetForEach => Some(0.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::MapSet => Some("set"),
        Builtin::MapGet => Some("get"),
        Builtin::MapHas => Some("has"),
        Builtin::MapDelete => Some("delete"),
        Builtin::MapClear => Some("clear"),
        Builtin::MapForEach => Some("forEach"),
        Builtin::SetAdd => Some("add"),
        Builtin::SetHas => Some("has"),
        Builtin::SetDelete => Some("delete"),
        Builtin::SetClear => Some("clear"),
        Builtin::SetForEach => Some("forEach"),
        _ => None,
    }
}
