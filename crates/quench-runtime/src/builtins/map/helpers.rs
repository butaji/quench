//! Shared helpers for Map and Set built-ins.

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::eval::member::eval_object_member;
use crate::value::{JsError, NativeFunction, Object, ObjectKind, Value};

/// SameValueZero key equality: NaN equals NaN, +0 and -0 are the same key
pub fn same_value_zero(a: &Value, b: &Value) -> bool {
    crate::value::compare::same_value_zero(a, b)
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
    let iterator_proto = crate::builtins::iterator::get_iterator_prototype()
        .unwrap_or_else(|| Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    let array_iterator_proto = Rc::new(RefCell::new(Object::with_prototype(
        ObjectKind::Ordinary,
        iterator_proto,
    )));
    let mut iter = Object::with_prototype(ObjectKind::Ordinary, array_iterator_proto);
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

/// Build a live iterator over Map/Set entries referenced by `entries_rc`.
/// Each iteration reads the current length from the entries array,
/// so deletions during iteration are reflected correctly.
pub fn make_live_entry_iterator(entries_rc: Rc<RefCell<Object>>) -> Value {
    let index = Rc::new(RefCell::new(0usize));
    let entries = Rc::clone(&entries_rc);
    let next_fn = NativeFunction::new(move |_args| {
        let mut result = Object::new(ObjectKind::Ordinary);
        let current_idx = { *index.borrow() };
        let borrowed = entries.borrow();
        let len = borrowed
            .get("length")
            .map(|v| crate::value::to_uint32(crate::value::to_number(&v)) as usize)
            .unwrap_or(borrowed.elements.len());
        if current_idx < len {
            let key = current_idx.to_string();
            let entry = borrowed.get(&key).unwrap_or_else(|| {
                if current_idx < borrowed.elements.len() {
                    borrowed.elements[current_idx].clone()
                } else {
                    Value::Undefined
                }
            });
            result.set("value", entry);
            result.set("done", Value::Boolean(false));
            *index.borrow_mut() = current_idx + 1;
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

/// Build a live iterator over Set values referenced by `values_rc`.
pub fn make_live_value_iterator(values_rc: Rc<RefCell<Object>>) -> Value {
    let index = Rc::new(RefCell::new(0usize));
    let values = Rc::clone(&values_rc);
    let next_fn = NativeFunction::new(move |_args| {
        let mut result = Object::new(ObjectKind::Ordinary);
        let current_idx = { *index.borrow() };
        let borrowed = values.borrow();
        let len = borrowed
            .get("length")
            .map(|v| crate::value::to_uint32(crate::value::to_number(&v)) as usize)
            .unwrap_or(borrowed.elements.len());
        if current_idx < len {
            let val = if current_idx < borrowed.elements.len() {
                borrowed.elements[current_idx].clone()
            } else {
                let key = current_idx.to_string();
                borrowed.get(&key).unwrap_or(Value::Undefined)
            };
            result.set("value", val);
            result.set("done", Value::Boolean(false));
            *index.borrow_mut() = current_idx + 1;
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

/// Build `{ next() }` reading indexed elements live from `arr_rc`.
pub fn make_live_index_iterator(arr_rc: Rc<RefCell<Object>>, mode: LiveIndexIteratorMode) -> Value {
    let index = Rc::new(RefCell::new(0usize));
    let arr = Rc::clone(&arr_rc);
    let next_fn = NativeFunction::new(move |_args| {
        let mut result = Object::new(ObjectKind::Ordinary);
        let current_idx = { *index.borrow() };
        let borrowed = arr.borrow();
        if borrowed.typed_array_is_out_of_bounds() {
            let (_, js_err) =
                crate::value::create_js_error_with_type("TypedArray is out of bounds", "TypeError");
            return Err(js_err);
        }
        let len = borrowed
            .get("length")
            .map(|v| crate::value::to_uint32(crate::value::to_number(&v)) as usize)
            .unwrap_or(borrowed.elements.len());
        if current_idx < len {
            let key = current_idx.to_string();
            let value = match mode {
                LiveIndexIteratorMode::Keys => Value::Number(current_idx as f64),
                LiveIndexIteratorMode::Values => {
                    // Check for accessor property (getter) — use get_own_property
                    // which properly checks the getters map.
                    let desc = borrowed.get_own_property(&key);
                    if let Some(desc) = desc {
                        if let Some(func) = desc.get {
                            drop(borrowed);
                            let _arr_val = Value::Object(Rc::clone(&arr));
                            return invoke_getter_func(
                                func,
                                &arr,
                                &key,
                                mode,
                                current_idx,
                                index.clone(),
                            );
                        }
                        if let Some(val) = desc.value {
                            val
                        } else {
                            Value::Undefined
                        }
                    } else {
                        borrowed.get(&key).unwrap_or_else(|| {
                            if current_idx < borrowed.elements.len() {
                                borrowed.elements[current_idx].clone()
                            } else {
                                Value::Undefined
                            }
                        })
                    }
                }
                LiveIndexIteratorMode::Entries => {
                    let desc = borrowed.get_own_property(&key);
                    let entry_val = if let Some(desc) = desc {
                        if let Some(func) = desc.get {
                            drop(borrowed);
                            let _arr_val = Value::Object(Rc::clone(&arr));
                            return invoke_getter_func(
                                func,
                                &arr,
                                &key,
                                mode,
                                current_idx,
                                index.clone(),
                            );
                        }
                        if let Some(val) = desc.value {
                            val
                        } else {
                            Value::Undefined
                        }
                    } else {
                        borrowed.get(&key).unwrap_or_else(|| {
                            if current_idx < borrowed.elements.len() {
                                borrowed.elements[current_idx].clone()
                            } else {
                                Value::Undefined
                            }
                        })
                    };
                    Value::Object(Rc::new(RefCell::new(Object::new_array_from(vec![
                        Value::Number(current_idx as f64),
                        entry_val,
                    ]))))
                }
            };
            result.set("value", value);
            result.set("done", Value::Boolean(false));
            *index.borrow_mut() = current_idx + 1;
        } else {
            result.set("value", Value::Undefined);
            result.set("done", Value::Boolean(true));
        }
        Ok(Value::Object(Rc::new(RefCell::new(result))))
    });
    let iterator_proto = crate::builtins::iterator::get_iterator_prototype()
        .unwrap_or_else(|| Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    let array_iterator_proto = Rc::new(RefCell::new(Object::with_prototype(
        ObjectKind::Ordinary,
        iterator_proto,
    )));
    let mut iter = Object::with_prototype(ObjectKind::Ordinary, array_iterator_proto);
    iter.set("next", Value::NativeFunction(Rc::new(next_fn)));
    Value::Object(Rc::new(RefCell::new(iter)))
}

/// Invoke a getter function and return the iterator result.
fn invoke_getter_func(
    func: Value,
    arr: &Rc<RefCell<Object>>,
    _key: &str,
    mode: LiveIndexIteratorMode,
    i: usize,
    index: Rc<RefCell<usize>>,
) -> Result<Value, JsError> {
    let arr_val = Value::Object(Rc::clone(arr));
    let val = crate::eval::function::call_value_with_this(func, vec![], arr_val)?;
    let mut result = Object::new(ObjectKind::Ordinary);
    let value = match mode {
        LiveIndexIteratorMode::Values => val,
        LiveIndexIteratorMode::Entries => {
            Value::Object(Rc::new(RefCell::new(Object::new_array_from(vec![
                Value::Number(i as f64),
                val,
            ]))))
        }
        LiveIndexIteratorMode::Keys => Value::Number(i as f64),
    };
    result.set("value", value);
    result.set("done", Value::Boolean(false));
    *index.borrow_mut() = i + 1;
    Ok(Value::Object(Rc::new(RefCell::new(result))))
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
