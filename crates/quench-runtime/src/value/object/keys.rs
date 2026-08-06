//! Property key enumeration methods for Object.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::value::object::helpers::as_array_index;

/// Get all property keys (own properties only, including getters/setters).
pub fn own_keys(obj: &crate::value::Object) -> Vec<String> {
    let mut keys = array_indices(obj);
    let mut seen: HashSet<String> = keys.iter().cloned().collect();
    for key in obj.descriptors.keys() {
        if as_array_index(key).is_none()
            && !seen.contains(key)
            && obj.is_enumerable(key)
            && !key.contains('\0')
        {
            seen.insert(key.clone());
            keys.push(key.clone());
        }
    }
    if obj.kind == crate::value::kind::ObjectKind::ModuleNamespace {
        keys.sort();
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
    let target_obj = target.borrow();
    let mut keys = Vec::new();
    let mut collected = HashSet::new();
    let mut current: Option<Rc<RefCell<crate::value::Object>>> = Some(Rc::clone(target));

    while let Some(cur_rc) = current {
        let cur = cur_rc.borrow();
        for key in enumerable_own_keys(&cur) {
            if key == "length" && target_obj.kind == crate::value::kind::ObjectKind::Array {
                continue;
            }
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
    for key in obj.descriptors.keys() {
        if as_array_index(key).is_none()
            && key != "_value"
            && !key.contains('\0')
            && !seen.contains(key)
        {
            seen.insert(key.clone());
            keys.push(key.clone());
        }
    }
    if obj.kind == crate::value::kind::ObjectKind::ModuleNamespace {
        keys.sort();
    }
    keys
}

/// Collect array index strings from the elements Vec or from numeric properties.
fn array_indices(obj: &crate::value::Object) -> Vec<String> {
    if obj.kind == crate::value::kind::ObjectKind::Array {
        let mut indices: Vec<usize> = (0..obj.elements.len())
            .filter(|i| !obj.holes.contains(i))
            .collect();
        let property_indices: Vec<usize> = obj
            .properties
            .keys()
            .filter_map(|key| as_array_index(key))
            .filter(|index| !indices.contains(index))
            .collect();
        indices.extend(property_indices);
        indices.sort_unstable();
        indices.into_iter().map(|i| i.to_string()).collect()
    } else if let crate::value::ObjData::Idx { length, .. } = obj.data {
        (0..length).map(|i| i.to_string()).collect()
    } else if matches!(obj.data, crate::value::ObjData::Args { .. }) {
        let mut indices: Vec<usize> = (0..obj.elements.len())
            .filter(|i| !obj.holes.contains(i))
            .collect();
        let property_indices: Vec<usize> = obj
            .properties
            .keys()
            .filter_map(|key| as_array_index(key))
            .filter(|index| !indices.contains(index))
            .collect();
        indices.extend(property_indices);
        indices.sort_unstable();
        indices.into_iter().map(|i| i.to_string()).collect()
    } else {
        let mut numeric: Vec<(usize, String)> = obj
            .properties
            .keys()
            .chain(obj.getters.keys())
            .chain(obj.setters.keys())
            .filter_map(|k| as_array_index(k).map(|i| (i, k.clone())))
            .collect();
        numeric.sort_by_key(|(i, _)| *i);
        numeric.dedup_by(|a, b| a.1 == b.1);
        numeric.into_iter().map(|(_, k)| k).collect()
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

    #[test]
    fn array_for_in_does_not_include_length() {
        let mut o = Object::new(ObjectKind::Array);
        o.set("0", Value::Number(1.0));
        o.set("length", Value::Number(3.0));
        o.set("1", Value::Number(2.0));
        o.set("2", Value::Number(3.0));
        let keys = enumerate_for_in_keys(&Rc::new(RefCell::new(o)));
        assert_eq!(keys, vec!["0", "1", "2"]);
    }
}
