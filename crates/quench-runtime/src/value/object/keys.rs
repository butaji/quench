//! Property key enumeration methods for Object.

use crate::value::object::helpers::as_array_index;

/// Get all property keys (own properties only, including getters/setters).
pub fn own_keys(obj: &crate::value::Object) -> Vec<String> {
    let mut keys = array_indices(obj);
    let mut seen: std::collections::HashSet<String> = keys.iter().cloned().collect();
    add_non_numeric_keys(obj, &mut keys, &mut seen);
    add_accessor_keys(obj, &mut keys, &mut seen);
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
