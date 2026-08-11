//! Map/Set builtin metadata.

use crate::{ops::Builtin, value::Value};

/// Returns the prototype property value for Map/Set builtins.
pub fn collections_property(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    if builtin == WeakSetPrototype {
        return weak_set_property(key);
    }
    match (builtin, key) {
        (MapPrototype, "set") => Some(Value::Builtin(MapSet)),
        (MapPrototype, "get") => Some(Value::Builtin(MapGet)),
        (MapPrototype, "has") => Some(Value::Builtin(MapHas)),
        (MapPrototype, "delete") => Some(Value::Builtin(MapDelete)),
        (MapPrototype, "clear") => Some(Value::Builtin(MapClear)),
        (MapPrototype, "forEach") => Some(Value::Builtin(MapForEach)),
        (MapPrototype, "entries") => Some(Value::Builtin(MapEntries)),
        (MapPrototype, "keys") => Some(Value::Builtin(MapKeys)),
        (MapPrototype, "values") => Some(Value::Builtin(MapValues)),
        (SetPrototype, "add") => Some(Value::Builtin(SetAdd)),
        (SetPrototype, "has") => Some(Value::Builtin(SetHas)),
        (SetPrototype, "delete") => Some(Value::Builtin(SetDelete)),
        (SetPrototype, "clear") => Some(Value::Builtin(SetClear)),
        (SetPrototype, "forEach") => Some(Value::Builtin(SetForEach)),
        (SetPrototype, "keys") => Some(Value::Builtin(SetKeys)),
        (SetPrototype, "difference") => Some(Value::Builtin(SetDifference)),
        (SetPrototype, "intersection") => Some(Value::Builtin(SetIntersection)),
        (SetPrototype, "symmetricDifference") => Some(Value::Builtin(SetSymmetricDifference)),
        (SetPrototype, "union") => Some(Value::Builtin(SetUnion)),
        (SetPrototype, "isDisjointFrom") => Some(Value::Builtin(SetIsDisjointFrom)),
        (SetPrototype, "isSubsetOf") => Some(Value::Builtin(SetIsSubsetOf)),
        (SetPrototype, "isSupersetOf") => Some(Value::Builtin(SetIsSupersetOf)),
        (WeakMapPrototype, "set") => Some(Value::Builtin(WeakMapSet)),
        (WeakMapPrototype, "get") => Some(Value::Builtin(WeakMapGet)),
        (WeakMapPrototype, "has") => Some(Value::Builtin(WeakMapHas)),
        (WeakMapPrototype, "delete") => Some(Value::Builtin(WeakMapDelete)),
        (WeakMapPrototype, "getOrInsert") => Some(Value::Builtin(WeakMapGetOrInsert)),
        (WeakMapPrototype, "getOrInsertComputed") => {
            Some(Value::Builtin(WeakMapGetOrInsertComputed))
        }
        _ => None,
    }
}

fn weak_set_property(key: &str) -> Option<Value> {
    use Builtin::*;
    match key {
        "add" => Some(Value::Builtin(WeakSetAdd)),
        "has" => Some(Value::Builtin(WeakSetHas)),
        "delete" => Some(Value::Builtin(WeakSetDelete)),
        "constructor" => Some(Value::Builtin(WeakSet)),
        "Symbol.toStringTag" => Some(Value::String("WeakSet".into())),
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
        Builtin::MapEntries => Some("Map.prototype.entries"),
        Builtin::MapKeys => Some("Map.prototype.keys"),
        Builtin::MapValues => Some("Map.prototype.values"),
        Builtin::SetAdd => Some("Set.prototype.add"),
        Builtin::SetHas => Some("Set.prototype.has"),
        Builtin::SetDelete => Some("Set.prototype.delete"),
        Builtin::SetClear => Some("Set.prototype.clear"),
        Builtin::SetForEach => Some("Set.prototype.forEach"),
        Builtin::WeakMapSet => Some("WeakMap.prototype.set"),
        Builtin::WeakMapGet => Some("WeakMap.prototype.get"),
        Builtin::WeakMapHas => Some("WeakMap.prototype.has"),
        Builtin::WeakMapDelete => Some("WeakMap.prototype.delete"),
        Builtin::WeakMapGetOrInsert => Some("WeakMap.prototype.getOrInsert"),
        Builtin::WeakMapGetOrInsertComputed => Some("WeakMap.prototype.getOrInsertComputed"),
        Builtin::WeakSetAdd => Some("WeakSet.prototype.add"),
        Builtin::WeakSetHas => Some("WeakSet.prototype.has"),
        Builtin::WeakSetDelete => Some("WeakSet.prototype.delete"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::MapSet => Some(1.0),
        Builtin::MapGet | Builtin::MapHas | Builtin::MapDelete => Some(1.0),
        Builtin::MapClear | Builtin::MapForEach => Some(0.0),
        Builtin::MapEntries | Builtin::MapKeys | Builtin::MapValues => Some(0.0),
        Builtin::SetAdd | Builtin::SetHas | Builtin::SetDelete => Some(1.0),
        Builtin::SetClear | Builtin::SetForEach | Builtin::SetKeys => Some(0.0),
        Builtin::SetDifference
        | Builtin::SetIntersection
        | Builtin::SetSymmetricDifference
        | Builtin::SetUnion
        | Builtin::SetIsDisjointFrom
        | Builtin::SetIsSubsetOf
        | Builtin::SetIsSupersetOf => Some(1.0),
        Builtin::WeakMapSet
        | Builtin::WeakMapGet
        | Builtin::WeakMapHas
        | Builtin::WeakMapDelete => Some(1.0),
        Builtin::WeakMapGetOrInsert | Builtin::WeakMapGetOrInsertComputed => Some(2.0),
        Builtin::WeakSetAdd | Builtin::WeakSetHas | Builtin::WeakSetDelete => Some(1.0),
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
        Builtin::MapEntries => Some("entries"),
        Builtin::MapKeys => Some("keys"),
        Builtin::MapValues => Some("values"),
        Builtin::SetAdd => Some("add"),
        Builtin::SetHas => Some("has"),
        Builtin::SetDelete => Some("delete"),
        Builtin::SetClear => Some("clear"),
        Builtin::SetForEach => Some("forEach"),
        Builtin::WeakMapSet => Some("set"),
        Builtin::WeakMapGet => Some("get"),
        Builtin::WeakMapHas => Some("has"),
        Builtin::WeakMapDelete => Some("delete"),
        Builtin::WeakMapGetOrInsert => Some("getOrInsert"),
        Builtin::WeakMapGetOrInsertComputed => Some("getOrInsertComputed"),
        Builtin::WeakSetAdd => Some("add"),
        Builtin::WeakSetHas => Some("has"),
        Builtin::WeakSetDelete => Some("delete"),
        _ => None,
    }
}
