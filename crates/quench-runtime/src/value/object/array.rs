//! Array exotic object methods (ES 9.4.2).

use crate::value::object::helpers::{as_key, Desc, Key};

/// Array exotic [[DefineOwnProperty]] (ES 9.4.2.1).
pub fn array_define_own_property(obj: &mut crate::value::Object, key: &Key, desc: &Desc) -> bool {
    if key == &as_key("length") {
        return array_set_length(obj, desc);
    }
    if let Key::Idx(index) = key {
        let current_length = array_length_value(obj) as u32;
        if *index >= current_length && *index < 4294967295 {
            let new_len_val = crate::value::Value::Number((*index + 1) as f64);
            obj.props.insert(
                as_key("length"),
                Desc {
                    value: Some(new_len_val),
                    writable: Some(true),
                    enumerable: Some(false),
                    configurable: Some(false),
                    ..Default::default()
                },
            );
            obj.properties.insert(
                "length".to_string(),
                crate::value::Value::Number((*index + 1) as f64),
            );
            let needed = (*index + 1) as usize;
            if obj.elements.len() < needed {
                obj.elements.resize(needed, crate::value::Value::Undefined);
            }
            obj.elements[*index as usize] =
                desc.value.clone().unwrap_or(crate::value::Value::Undefined);
        }
    }
    crate::value::object::vtable::ordinary_define_own_property(obj, key, desc)
}

/// Get the numeric length from an array object.
pub fn array_length_value(obj: &crate::value::Object) -> f64 {
    if let Some(desc) = obj.props.get(&as_key("length")) {
        if let Some(crate::value::Value::Number(n)) = desc.value {
            return n;
        }
    }
    obj.properties
        .get("length")
        .and_then(|v| match v {
            crate::value::Value::Number(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(0.0)
}

/// ArraySetLength (ES 9.4.2.4).
pub fn array_set_length(obj: &mut crate::value::Object, desc: &Desc) -> bool {
    let new_len = match &desc.value {
        Some(crate::value::Value::Number(n)) => *n as u32,
        Some(_) => return false,
        None => return true,
    };
    let old_len = array_length_value(obj) as u32;
    if new_len < old_len {
        for i in new_len..old_len {
            obj.props.shift_remove(&Key::Idx(i));
        }
        if new_len as usize <= obj.elements.len() {
            obj.elements.truncate(new_len as usize);
        }
    }
    let len_val = crate::value::Value::Number(new_len as f64);
    obj.props.insert(
        as_key("length"),
        Desc {
            value: Some(len_val.clone()),
            writable: Some(true),
            enumerable: Some(false),
            configurable: Some(false),
            ..Default::default()
        },
    );
    obj.properties.insert("length".to_string(), len_val);
    true
}

/// VTable for Array exotic objects — overrides only define_own_property.
pub static ARRAY_VTABLE: crate::value::object::helpers::VTable =
    crate::value::object::helpers::VTable {
        define_own_property: array_define_own_property,
        ..crate::value::object::vtable::ORDINARY_VTABLE
    };
