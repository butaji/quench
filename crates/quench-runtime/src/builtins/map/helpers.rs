//! Shared helpers for Map and Set built-ins.

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::eval::member::eval_object_member;
use crate::value::{JsError, NativeFunction, Object, ObjectKind, Value};

/// SameValueZero key equality: NaN equals NaN, +0 and -0 are the same key
pub fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => crate::value::strict_eq(a, b),
    }
}

/// Get the internal entries array (`_entries`) of a Map
pub fn map_entries(this: &Value) -> Option<Rc<RefCell<Object>>> {
    if let Value::Object(o) = this {
        if let Some(Value::Object(entries)) = o.borrow().get("_entries") {
            return Some(Rc::clone(&entries));
        }
    }
    None
}

/// Find the pair array holding `key`, or None
pub fn map_find_pair(entries: &Rc<RefCell<Object>>, key: &Value) -> Option<Rc<RefCell<Object>>> {
    let elements = entries.borrow().elements.clone();
    for elem in elements {
        if let Value::Object(pair) = elem {
            let k = pair.borrow().elements.first().cloned();
            if let Some(k) = k {
                if same_value_zero(&k, key) {
                    return Some(pair);
                }
            }
        }
    }
    None
}

/// Store the current entry count in the map's `size` property
pub fn map_update_size(this: &Value, entries: &Rc<RefCell<Object>>) {
    let size = entries.borrow().elements.len() as f64;
    if let Value::Object(o) = this {
        o.borrow_mut().set("size", Value::Number(size));
    }
}

/// Initialize Map internal slots on `obj`, preserving its [[Prototype]] (subclassing).
pub fn init_map_object(obj: &Rc<RefCell<Object>>) {
    let mut m = obj.borrow_mut();
    if m.get("_entries").is_none() {
        let entries = Object::new_array(0);
        m.set("_entries", Value::Object(Rc::new(RefCell::new(entries))));
        m.set("size", Value::Number(0.0));
    }
    m.kind = ObjectKind::Map;
}

/// Initialize Set internal slots on `obj`, preserving its [[Prototype]] (subclassing).
pub fn init_set_object(obj: &Rc<RefCell<Object>>) {
    let mut s = obj.borrow_mut();
    if s.get("_values").is_none() {
        let values = Object::new_array(0);
        s.set("_values", Value::Object(Rc::new(RefCell::new(values))));
        s.set("size", Value::Number(0.0));
    }
    s.kind = ObjectKind::Set;
}

/// Get the internal values array (`_values`) of a Set
pub fn set_values(this: &Value) -> Option<Rc<RefCell<Object>>> {
    if let Value::Object(o) = this {
        if let Some(Value::Object(values)) = o.borrow().get("_values") {
            return Some(Rc::clone(&values));
        }
    }
    None
}

pub fn set_has_value(values: &Rc<RefCell<Object>>, value: &Value) -> bool {
    values
        .borrow()
        .elements
        .iter()
        .any(|v| same_value_zero(v, value))
}

pub fn native_fn(f: impl Fn(Vec<Value>) -> Result<Value, JsError> + 'static) -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new(f)))
}

/// Build an iterator object over a snapshot of values (`{ next() }` protocol).
pub fn make_iterator(items: Vec<Value>) -> Value {
    let items = Rc::new(items);
    let index = Rc::new(RefCell::new(0usize));
    let next_fn = NativeFunction::new(move |_args| {
        let mut obj = Object::new(ObjectKind::Ordinary);
        let mut i = index.borrow_mut();
        if *i < items.len() {
            obj.set("value", items[*i].clone());
            obj.set("done", Value::Boolean(false));
            *i += 1;
        } else {
            obj.set("value", Value::Undefined);
            obj.set("done", Value::Boolean(true));
        }
        Ok(Value::Object(Rc::new(RefCell::new(obj))))
    });
    let mut iter = Object::new(ObjectKind::Ordinary);
    iter.set("next", Value::NativeFunction(Rc::new(next_fn)));
    Value::Object(Rc::new(RefCell::new(iter)))
}

/// Iterator mode for live indexed element iteration.
#[derive(Copy, Clone)]
pub enum LiveIndexIteratorMode {
    Keys,
    Values,
    Entries,
}

/// Build `{ next() }` reading indexed elements live from `arr_rc`.
pub fn make_live_index_iterator(arr_rc: Rc<RefCell<Object>>, mode: LiveIndexIteratorMode) -> Value {
    let index = Rc::new(RefCell::new(0usize));
    let arr = Rc::clone(&arr_rc);
    let next_fn = NativeFunction::new(move |_args| {
        let mut result = Object::new(ObjectKind::Ordinary);
        let mut i = index.borrow_mut();
        let borrowed = arr.borrow();
        let len = borrowed.elements.len();
        if *i < len {
            let value = match mode {
                LiveIndexIteratorMode::Keys => Value::Number(*i as f64),
                LiveIndexIteratorMode::Values => borrowed.elements[*i].clone(),
                LiveIndexIteratorMode::Entries => {
                    Value::Object(Rc::new(RefCell::new(Object::new_array_from(vec![
                        Value::Number(*i as f64),
                        borrowed.elements[*i].clone(),
                    ]))))
                }
            };
            result.set("value", value);
            result.set("done", Value::Boolean(false));
            *i += 1;
        } else {
            result.set("value", Value::Undefined);
            result.set("done", Value::Boolean(true));
        }
        Ok(Value::Object(Rc::new(RefCell::new(result))))
    });
    let mut iter = Object::new(ObjectKind::Ordinary);
    iter.set("next", Value::NativeFunction(Rc::new(next_fn)));
    Value::Object(Rc::new(RefCell::new(iter)))
}

/// Property key for the Symbol.iterator method
pub fn iterator_prop_key() -> Option<String> {
    match crate::builtins::symbol::get_well_known_symbol_no_ctx("iterator") {
        Some(Value::Symbol(payload)) => Some(
            payload
                .desc
                .clone()
                .map(|s| s.to_string())
                .unwrap_or_default(),
        ),
        _ => None,
    }
}

/// Populate a Map from an iterable source. Per spec, `new Map(iterable)`:
/// 1. Get adder = Map.prototype.set (this may throw via getter)
/// 2. For each entry [k, v] in iterable, call adder(k, v)
pub fn map_populate(map: &Rc<RefCell<Object>>, src: &Value) -> Result<(), JsError> {
    let adder = eval_object_member(map, "set", None)?;

    let pairs: Vec<Value> = match src {
        Value::Object(o) => match map_entries(src) {
            Some(src_entries) => src_entries.borrow().elements.clone(),
            None => o.borrow().elements.clone(),
        },
        _ => Vec::new(),
    };
    for pair in pairs {
        let Value::Object(p) = pair else {
            continue;
        };
        let p_ref = p.borrow();
        let k = p_ref.get("0").unwrap_or(Value::Undefined);
        let v = p_ref.get("1").unwrap_or(Value::Undefined);
        drop(p_ref);
        call_value_with_this(adder.clone(), vec![k, v], Value::Object(Rc::clone(map)))?;
    }
    Ok(())
}

/// Populate a Set from an iterable source. Per spec, `new Set(iterable)`:
/// 1. Get adder = Set.prototype.add (this may throw via getter)
/// 2. For each value in iterable, call adder(value)
pub fn set_populate(set: &Rc<RefCell<Object>>, src: &Value) -> Result<(), JsError> {
    let adder = eval_object_member(set, "add", None)?;

    let items: Vec<Value> = match src {
        Value::Object(o) => match set_values(src) {
            Some(src_values) => src_values.borrow().elements.clone(),
            None => o.borrow().elements.clone(),
        },
        _ => Vec::new(),
    };
    for item in items {
        call_value_with_this(adder.clone(), vec![item], Value::Object(Rc::clone(set)))?;
    }
    Ok(())
}
