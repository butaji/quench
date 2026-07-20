//! VTable implementations for Object internal methods (ES 9.1).

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::object::helpers::{Desc, Key, PropertyFlags, VTable};

/// Ordinary [[GetPrototypeOf]] (ES 9.1.1)
pub fn ordinary_get_prototype_of(
    obj: &crate::value::Object,
) -> Option<Rc<RefCell<crate::value::Object>>> {
    obj.prototype.clone()
}

/// Ordinary [[SetPrototypeOf]] (ES 9.1.2)
pub fn ordinary_set_prototype_of(
    obj: &mut crate::value::Object,
    proto: Option<Rc<RefCell<crate::value::Object>>>,
) -> bool {
    if !obj.extensible && obj.prototype.is_some() {
        return false;
    }
    obj.prototype = proto;
    true
}

/// Ordinary [[IsExtensible]] (ES 9.1.4)
pub fn ordinary_is_extensible(obj: &crate::value::Object) -> bool {
    obj.extensible
}

/// Ordinary [[PreventExtensions]] (ES 9.1.5)
pub fn ordinary_prevent_extensions(obj: &mut crate::value::Object) -> bool {
    obj.extensible = false;
    true
}

/// Ordinary [[GetOwnProperty]] (ES 9.1.5) — uses the TComp props map.
pub fn ordinary_get_own_property(obj: &crate::value::Object, key: &Key) -> Option<Desc> {
    if let Some(desc) = obj.props.get(key) {
        return Some(desc.clone());
    }
    let key_str = match key {
        Key::Str(s) => s.as_ref(),
        Key::Idx(_i) => return None,
        Key::Sym(_s) => return None,
    };
    let flags = obj.descriptors.get(key_str).cloned().unwrap_or_default();
    if let Some(val) = obj.properties.get(key_str) {
        return Some(Desc {
            value: Some(val.clone()),
            writable: Some(flags.writable),
            enumerable: Some(flags.enumerable),
            configurable: Some(flags.configurable),
            ..Default::default()
        });
    }
    if let Some(g) = obj.getters.get(key_str) {
        return Some(Desc {
            get: g.func.clone(),
            enumerable: Some(flags.enumerable),
            configurable: Some(flags.configurable),
            ..Default::default()
        });
    }
    if let Some(s) = obj.setters.get(key_str) {
        return Some(Desc {
            set: s.func.clone(),
            enumerable: Some(flags.enumerable),
            configurable: Some(flags.configurable),
            ..Default::default()
        });
    }
    None
}

/// Ordinary [[DefineOwnProperty]] (ES 9.1.6)
pub fn ordinary_define_own_property(
    obj: &mut crate::value::Object,
    key: &Key,
    desc: &Desc,
) -> bool {
    let key_str = match key {
        Key::Str(s) => Some(s.as_ref()),
        _ => None,
    };
    if !obj.extensible && !obj.props.contains_key(key) {
        return false;
    }
    if desc.is_data() {
        let val = desc.value.clone().unwrap_or(crate::value::Value::Undefined);
        obj.props.insert(key.clone(), desc.clone());
        if let Some(ks) = key_str {
            obj.properties.insert(ks.to_string(), val);
            let flags = PropertyFlags {
                value: desc.value.clone(),
                writable: desc.writable.unwrap_or(false),
                enumerable: desc.enumerable.unwrap_or(false),
                configurable: desc.configurable.unwrap_or(false),
            };
            obj.descriptors.insert(ks.to_string(), flags);
            obj.getters.shift_remove(ks);
            obj.setters.shift_remove(ks);
        }
        true
    } else if desc.is_accessor() {
        obj.props.insert(key.clone(), desc.clone());
        if let Some(ks) = key_str {
            let flags = PropertyFlags {
                value: None,
                writable: false,
                enumerable: desc.enumerable.unwrap_or(true),
                configurable: desc.configurable.unwrap_or(true),
            };
            obj.descriptors.insert(ks.to_string(), flags);
            if let Some(ref g) = desc.get {
                obj.set_getter_func(ks, g.clone());
            }
            if let Some(ref s) = desc.set {
                obj.set_setter_func(ks, s.clone());
            }
            obj.properties.shift_remove(ks);
        }
        true
    } else {
        if let Some(entry) = obj.props.get_mut(key) {
            if let Some(e) = desc.enumerable {
                entry.enumerable = Some(e);
            }
            if let Some(c) = desc.configurable {
                entry.configurable = Some(c);
            }
        }
        true
    }
}

/// Ordinary [[HasProperty]] (ES 9.1.7)
pub fn ordinary_has_property(obj: &crate::value::Object, key: &Key) -> bool {
    obj.props.contains_key(key)
        || match key {
            Key::Str(s) => {
                obj.properties.contains_key(s.as_ref())
                    || obj.getters.contains_key(s.as_ref())
                    || obj.setters.contains_key(s.as_ref())
            }
            _ => false,
        }
}

/// Ordinary [[Get]] (ES 9.1.8)
pub fn ordinary_get(
    obj: &crate::value::Object,
    key: &Key,
    _receiver: crate::value::Value,
) -> crate::value::Value {
    if let Some(desc) = obj.props.get(key) {
        if let Some(ref val) = desc.value {
            return val.clone();
        }
        if let Some(ref get_func) = desc.get {
            return get_func.clone();
        }
    }
    let key_str = match key {
        Key::Str(s) => s.as_ref(),
        Key::Idx(i) => {
            return obj
                .get(&i.to_string())
                .unwrap_or(crate::value::Value::Undefined)
        }
        Key::Sym(s) => {
            return if let Some(ref d) = s.desc {
                obj.get(d).unwrap_or(crate::value::Value::Undefined)
            } else {
                crate::value::Value::Undefined
            }
        }
    };
    obj.get(key_str).unwrap_or(crate::value::Value::Undefined)
}

/// Ordinary [[Set]] (ES 9.1.9)
pub fn ordinary_set(
    obj: &mut crate::value::Object,
    key: &Key,
    value: crate::value::Value,
    _receiver: crate::value::Value,
) -> bool {
    if !obj.extensible {
        return false;
    }
    if let Some(desc) = obj.props.get(key) {
        if desc.set.is_some() || desc.get.is_some() {
            return false;
        }
        if desc.writable == Some(false) {
            return false;
        }
    }
    obj.props.insert(
        key.clone(),
        Desc {
            value: Some(value.clone()),
            writable: Some(true),
            enumerable: Some(true),
            configurable: Some(true),
            ..Default::default()
        },
    );
    let key_str = match key {
        Key::Str(s) => s.as_ref(),
        Key::Idx(i) => {
            let s = i.to_string();
            obj.set(&s, value);
            return true;
        }
        Key::Sym(_) => return true,
    };
    obj.set(key_str, value);
    true
}

/// Ordinary [[Delete]] (ES 9.1.10)
pub fn ordinary_delete(obj: &mut crate::value::Object, key: &Key) -> bool {
    obj.props.shift_remove(key);
    if let Key::Str(s) = key {
        obj.properties.shift_remove(s.as_ref());
        obj.descriptors.shift_remove(s.as_ref());
        obj.getters.shift_remove(s.as_ref());
        obj.setters.shift_remove(s.as_ref());
    } else if let Key::Idx(i) = key {
        let s = i.to_string();
        obj.properties.shift_remove(&s);
        if (*i as usize) < obj.elements.len() {
            obj.elements[*i as usize] = crate::value::Value::Undefined;
            obj.holes.insert(*i as usize);
        }
    }
    true
}

/// Ordinary [[OwnPropertyKeys]] (ES 9.1.12)
pub fn ordinary_own_property_keys(obj: &crate::value::Object) -> Vec<Key> {
    let mut keys: Vec<Key> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for (k, _) in &obj.props {
        match k {
            Key::Idx(i) => indices.push(*i),
            other => keys.push(other.clone()),
        }
    }
    indices.sort_unstable();
    let mut result: Vec<Key> = indices.into_iter().map(Key::Idx).collect();
    result.extend(keys);
    result
}

/// VTable for ordinary (non-exotic) objects.
pub static ORDINARY_VTABLE: VTable = VTable {
    get_prototype_of: ordinary_get_prototype_of,
    set_prototype_of: ordinary_set_prototype_of,
    is_extensible: ordinary_is_extensible,
    prevent_extensions: ordinary_prevent_extensions,
    get_own_property: ordinary_get_own_property,
    define_own_property: ordinary_define_own_property,
    has_property: ordinary_has_property,
    get: ordinary_get,
    set: ordinary_set,
    delete: ordinary_delete,
    own_property_keys: ordinary_own_property_keys,
    call: None,
    construct: None,
};
