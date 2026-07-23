//! Property key enumeration methods for Object.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::value::object::helpers::{as_array_index, ObjData};

/// Get all property keys (own properties only, including getters/setters).
pub fn own_keys(obj: &crate::value::Object) -> Vec<String> {
    let mut keys = array_indices(obj);
    let mut seen: HashSet<String> = keys.iter().cloned().collect();
    if obj.kind == crate::value::kind::ObjectKind::Array {
        add_accessor_keys(obj, &mut keys, &mut seen);
        add_non_numeric_keys(obj, &mut keys, &mut seen);
    } else {
        add_non_numeric_keys(obj, &mut keys, &mut seen);
        add_accessor_keys(obj, &mut keys, &mut seen);
    }
    keys
}

/// Own enumerable property keys in OrdinaryOwnPropertyKeys order.
pub fn enumerable_own_keys(obj: &crate::value::Object) -> Vec<String> {
    own_keys(obj)
        .into_iter()
        .filter(|key| obj.is_enumerable(key))
        .collect()
}

/// For-in enumeration: own keys then prototype chain, skipping shadowed names.
pub fn enumerate_for_in_keys(target: &Rc<RefCell<crate::value::Object>>) -> Vec<String> {
    let target_borrow = target.borrow();
    if let ObjData::Idx { length, .. } = target_borrow.data {
        return (0..length as usize).map(|i| i.to_string()).collect();
    }
    let target_obj = target_borrow;
    let mut keys = Vec::new();
    let mut collected = HashSet::new();
    let mut current: Option<Rc<RefCell<crate::value::Object>>> = Some(Rc::clone(target));

    while let Some(cur_rc) = current {
        let cur = cur_rc.borrow();
        for key in enumerable_own_keys(&cur) {
            if collected.contains(&key) {
                continue;
            }
            if !Rc::ptr_eq(&cur_rc, target) && target_obj.has_own(&key) {
                continue;
            }
            collected.insert(key.clone());
            keys.push(key);
        }
        current = cur.prototype.clone();
    }
    keys
}

/// Like `own_keys` but also includes non-enumerable own properties.
pub fn own_property_names(obj: &crate::value::Object) -> Vec<String> {
    let mut keys = array_indices(obj);
    let mut seen: std::collections::HashSet<String> = keys.iter().cloned().collect();
    for key in obj.properties.keys() {
        if as_array_index(key).is_none() && !seen.contains(key) {
            seen.insert(key.clone());
            keys.push(key.clone());
        }
    }
    for key in obj.getters.keys().chain(obj.setters.keys()) {
        if !seen.contains(key) {
            seen.insert(key.clone());
            keys.push(key.clone());
        }
    }
    keys
}

/// Collect array index strings from the elements Vec or from numeric properties.
fn array_indices(obj: &crate::value::Object) -> Vec<String> {
    if obj.kind == crate::value::kind::ObjectKind::Array {
        (0..obj.elements.len())
            .filter(|i| !obj.holes.contains(i))
            .map(|i| i.to_string())
            .collect()
    } else {
        let mut numeric: Vec<(usize, String)> = obj
            .properties
            .keys()
            .filter_map(|k| as_array_index(k).map(|i| (i, k.clone())))
            .collect();
        numeric.sort_by_key(|(i, _)| *i);
        numeric.into_iter().map(|(_, k)| k).collect()
    }
}

fn add_non_numeric_keys(
    obj: &crate::value::Object,
    keys: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    for key in obj.properties.keys() {
        if as_array_index(key).is_none() && !seen.contains(key) && obj.is_enumerable(key) {
            seen.insert(key.clone());
            keys.push(key.clone());
        }
    }
}

fn add_accessor_keys(
    obj: &crate::value::Object,
    keys: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    for key in obj.getters.keys() {
        if !seen.contains(key) && obj.is_enumerable(key) {
            seen.insert(key.clone());
            keys.push(key.clone());
        }
    }
    for key in obj.setters.keys() {
        if !seen.contains(key) && !obj.getters.contains_key(key) && obj.is_enumerable(key) {
            seen.insert(key.clone());
            keys.push(key.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::kind::ObjectKind;
    use crate::value::object::Object;
    use crate::value::Value;

    #[test]
    fn enumerate_walks_object_prototype_field() {
        let mut o = Object::new(ObjectKind::Ordinary);
        o.set("p1", Value::Number(1.0));
        let mut proto = Object::new(ObjectKind::Ordinary);
        proto.set("p4", Value::Number(1.0));
        let proto_rc = Rc::new(RefCell::new(proto));
        o.prototype = Some(Rc::clone(&proto_rc));
        let keys = enumerate_for_in_keys(&Rc::new(RefCell::new(o)));
        assert_eq!(keys, vec!["p1", "p4"]);
    }
}
