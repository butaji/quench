//! Map/Set builtin metadata.

use crate::{ops::Builtin, value::Value};

/// Returns the prototype property value for Map/Set builtins.
pub fn collections_property(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    if builtin == WeakSetPrototype {
        return weak_set_property(key);
    }
    if builtin == MapPrototype {
        return map_property(key);
    }
    if builtin == SetPrototype {
        return set_property(key);
    }
    match (builtin, key) {
        (SetIteratorPrototype | MapIteratorPrototype, k) => iterator_prototype_property(builtin, k),
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

fn set_property(key: &str) -> Option<Value> {
    use Builtin::*;
    match key {
        "constructor" => Some(Value::Builtin(Set)),
        "size" => Some(Value::Builtin(SetSizeGetter)),
        "Symbol.toStringTag" => Some(Value::String("Set".into())),
        "add" => Some(Value::Builtin(SetAdd)),
        "has" => Some(Value::Builtin(SetHas)),
        "delete" => Some(Value::Builtin(SetDelete)),
        "clear" => Some(Value::Builtin(SetClear)),
        "forEach" => Some(Value::Builtin(SetForEach)),
        "keys" | "values" | "Symbol.iterator" => Some(Value::Builtin(SetIterator)),
        "entries" => Some(Value::Builtin(SetEntries)),
        "difference" => Some(Value::Builtin(SetDifference)),
        "intersection" => Some(Value::Builtin(SetIntersection)),
        "symmetricDifference" => Some(Value::Builtin(SetSymmetricDifference)),
        "union" => Some(Value::Builtin(SetUnion)),
        "isDisjointFrom" => Some(Value::Builtin(SetIsDisjointFrom)),
        "isSubsetOf" => Some(Value::Builtin(SetIsSubsetOf)),
        "isSupersetOf" => Some(Value::Builtin(SetIsSupersetOf)),
        _ => None,
    }
}

fn iterator_prototype_property(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    let (next, tag) = match builtin {
        SetIteratorPrototype => (SetIteratorNext, "Set Iterator"),
        MapIteratorPrototype => (MapIteratorNext, "Map Iterator"),
        _ => return None,
    };
    match key {
        "next" => Some(Value::Builtin(next)),
        "Symbol.iterator" => Some(Value::Builtin(IteratorSelf)),
        "Symbol.toStringTag" => Some(Value::String(tag.into())),
        _ => None,
    }
}

fn map_property(key: &str) -> Option<Value> {
    use Builtin::*;
    match key {
        "constructor" => Some(Value::Builtin(Map)),
        "size" => Some(Value::Builtin(MapSizeGetter)),
        "Symbol.toStringTag" => Some(Value::String("Map".into())),
        "set" => Some(Value::Builtin(MapSet)),
        "get" => Some(Value::Builtin(MapGet)),
        "has" => Some(Value::Builtin(MapHas)),
        "delete" => Some(Value::Builtin(MapDelete)),
        "clear" => Some(Value::Builtin(MapClear)),
        "forEach" => Some(Value::Builtin(MapForEach)),
        "entries" => Some(Value::Builtin(MapEntries)),
        "keys" => Some(Value::Builtin(MapKeys)),
        "values" => Some(Value::Builtin(MapValues)),
        "Symbol.iterator" => Some(Value::Builtin(MapEntries)),
        "getOrInsert" => Some(Value::Builtin(MapGetOrInsert)),
        "getOrInsertComputed" => Some(Value::Builtin(MapGetOrInsertComputed)),
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
    if let Some(name) = set_fn_name(b) {
        return Some(name);
    }
    match b {
        Builtin::IteratorSelf => Some("Iterator.prototype[Symbol.iterator]"),
        Builtin::IteratorNext => Some("ArrayIteratorPrototype.prototype.next"),
        Builtin::AsyncIteratorSelf => Some("[Symbol.asyncIterator]"),
        Builtin::AsyncIteratorDispose => Some("[Symbol.asyncDispose]"),
        Builtin::SetIteratorNext => Some("SetIteratorPrototype.prototype.next"),
        Builtin::MapIteratorNext => Some("MapIteratorPrototype.prototype.next"),
        Builtin::MapSet => Some("Map.prototype.set"),
        Builtin::MapSizeGetter => Some("get size"),
        Builtin::MapGet => Some("Map.prototype.get"),
        Builtin::MapHas => Some("Map.prototype.has"),
        Builtin::MapDelete => Some("Map.prototype.delete"),
        Builtin::MapClear => Some("Map.prototype.clear"),
        Builtin::MapForEach => Some("Map.prototype.forEach"),
        Builtin::MapEntries => Some("Map.prototype.entries"),
        Builtin::MapKeys => Some("Map.prototype.keys"),
        Builtin::MapValues => Some("Map.prototype.values"),
        Builtin::MapGetOrInsert => Some("getOrInsert"),
        Builtin::MapGetOrInsertComputed => Some("getOrInsertComputed"),
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

const fn set_fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::SetAdd => Some("Set.prototype.add"),
        Builtin::SetSizeGetter => Some("get size"),
        Builtin::SetHas => Some("Set.prototype.has"),
        Builtin::SetDelete => Some("Set.prototype.delete"),
        Builtin::SetClear => Some("Set.prototype.clear"),
        Builtin::SetForEach => Some("Set.prototype.forEach"),
        Builtin::SetIterator => Some("Set.prototype.values"),
        Builtin::SetEntries => Some("Set.prototype.entries"),
        Builtin::SetSpeciesGetter | Builtin::MapSpeciesGetter | Builtin::SpeciesGetter => {
            Some("get [Symbol.species]")
        }
        Builtin::SetDifference => Some("Set.prototype.difference"),
        Builtin::SetIntersection => Some("Set.prototype.intersection"),
        Builtin::SetSymmetricDifference => Some("Set.prototype.symmetricDifference"),
        Builtin::SetUnion => Some("Set.prototype.union"),
        Builtin::SetIsDisjointFrom => Some("Set.prototype.isDisjointFrom"),
        Builtin::SetIsSubsetOf => Some("Set.prototype.isSubsetOf"),
        Builtin::SetIsSupersetOf => Some("Set.prototype.isSupersetOf"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::IteratorSelf | Builtin::AsyncIteratorSelf | Builtin::AsyncIteratorDispose => {
            Some(0.0)
        }
        Builtin::IteratorNext | Builtin::SetIteratorNext | Builtin::MapIteratorNext => Some(0.0),
        Builtin::MapSet => Some(2.0),
        Builtin::MapSizeGetter | Builtin::SetSizeGetter => Some(0.0),
        Builtin::MapGet | Builtin::MapHas | Builtin::MapDelete => Some(1.0),
        Builtin::MapClear => Some(0.0),
        Builtin::MapForEach => Some(1.0),
        Builtin::MapEntries | Builtin::MapKeys | Builtin::MapValues => Some(0.0),
        Builtin::MapGroupBy => Some(2.0),
        Builtin::MapGetOrInsert | Builtin::MapGetOrInsertComputed => Some(2.0),
        Builtin::SetAdd | Builtin::SetHas | Builtin::SetDelete => Some(1.0),
        Builtin::SetClear => Some(0.0),
        Builtin::SetForEach => Some(1.0),
        Builtin::SetIterator => Some(0.0),
        Builtin::SetEntries
        | Builtin::SetSpeciesGetter
        | Builtin::MapSpeciesGetter
        | Builtin::SpeciesGetter => Some(0.0),
        Builtin::SetDifference
        | Builtin::SetIntersection
        | Builtin::SetSymmetricDifference
        | Builtin::SetUnion
        | Builtin::SetIsDisjointFrom
        | Builtin::SetIsSubsetOf
        | Builtin::SetIsSupersetOf => Some(1.0),
        Builtin::WeakMapSet => Some(2.0),
        Builtin::WeakMapGet | Builtin::WeakMapHas | Builtin::WeakMapDelete => Some(1.0),
        Builtin::WeakMapGetOrInsert | Builtin::WeakMapGetOrInsertComputed => Some(2.0),
        Builtin::WeakSetAdd | Builtin::WeakSetHas | Builtin::WeakSetDelete => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    if let Some(name) = size_getter_name(b) {
        return Some(name);
    }
    if let Some(name) = weak_short_name(b) {
        return Some(name);
    }
    match b {
        Builtin::IteratorSelf => Some("[Symbol.iterator]"),
        Builtin::AsyncIteratorSelf => Some("[Symbol.asyncIterator]"),
        Builtin::AsyncIteratorDispose => Some("[Symbol.asyncDispose]"),
        Builtin::IteratorNext | Builtin::SetIteratorNext | Builtin::MapIteratorNext => {
            Some("next")
        }
        Builtin::MapSet => Some("set"),
        Builtin::MapGet => Some("get"),
        Builtin::MapHas => Some("has"),
        Builtin::MapDelete => Some("delete"),
        Builtin::MapClear => Some("clear"),
        Builtin::MapForEach => Some("forEach"),
        Builtin::MapEntries => Some("entries"),
        Builtin::MapKeys => Some("keys"),
        Builtin::MapValues => Some("values"),
        Builtin::MapGroupBy => Some("groupBy"),
        Builtin::MapGetOrInsert => Some("getOrInsert"),
        Builtin::MapGetOrInsertComputed => Some("getOrInsertComputed"),
        Builtin::SetAdd => Some("add"),
        Builtin::SetHas => Some("has"),
        Builtin::SetDelete => Some("delete"),
        Builtin::SetClear => Some("clear"),
        Builtin::SetForEach => Some("forEach"),
        Builtin::SetIterator => Some("values"),
        Builtin::SetEntries => Some("entries"),
        Builtin::SetDifference => Some("difference"),
        Builtin::SetIntersection => Some("intersection"),
        Builtin::SetSymmetricDifference => Some("symmetricDifference"),
        Builtin::SetUnion => Some("union"),
        Builtin::SetIsDisjointFrom => Some("isDisjointFrom"),
        Builtin::SetIsSubsetOf => Some("isSubsetOf"),
        Builtin::SetIsSupersetOf => Some("isSupersetOf"),
        _ => None,
    }
}

const fn weak_short_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::WeakMapSet => Some("set"),
        Builtin::WeakSetAdd => Some("add"),
        Builtin::WeakMapGet => Some("get"),
        Builtin::WeakSetHas => Some("has"),
        Builtin::WeakMapHas => Some("has"),
        Builtin::WeakMapDelete | Builtin::WeakSetDelete => Some("delete"),
        Builtin::WeakMapGetOrInsert => Some("getOrInsert"),
        Builtin::WeakMapGetOrInsertComputed => Some("getOrInsertComputed"),
        _ => None,
    }
}

const fn size_getter_name(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::MapSizeGetter | Builtin::SetSizeGetter => Some("get size"),
        _ => None,
    }
}
