//! Iterator built-in — ES2025 §27.1
//!
//! Provides the `Iterator` namespace and %IteratorPrototype% with all helper
//! methods (map, filter, take, drop, reduce, toArray, forEach, some, every,
//! find, flatMap). Native implementations are exposed to the self-hosted JS
//! layer in `builtins/Iterator.js`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::builtins::object::get_object_prototype;
use crate::eval::object::obtain_iterator;
use crate::value::{to_number, JsError, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

// Thread-local storage for %IteratorPrototype%.
thread_local! {
    static ITERATOR_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> =
        const { RefCell::new(None) };
}

/// Get %IteratorPrototype% (for use by other builtins and interpreter).
pub fn get_iterator_prototype() -> Option<Rc<RefCell<Object>>> {
    ITERATOR_PROTOTYPE.with(|p| p.borrow().clone())
}

/// Save/restore for realm snapshots.
pub(crate) fn save_iterator_prototype() -> Option<Rc<RefCell<Object>>> {
    ITERATOR_PROTOTYPE.with(|p| p.borrow().clone())
}

pub(crate) fn restore_iterator_prototype(proto: Option<Rc<RefCell<Object>>>) {
    ITERATOR_PROTOTYPE.with(|p| *p.borrow_mut() = proto);
}

/// Register the Iterator builtin and all %IteratorPrototype% methods.
pub fn register_iterator(ctx: &mut Context) {
    let mut proto = Object::new(ObjectKind::Ordinary);
    if let Some(object_proto) = get_object_prototype() {
        proto.prototype = Some(object_proto);
    }
    let proto_rc = Rc::new(RefCell::new(proto));
    ITERATOR_PROTOTYPE.with(|p| *p.borrow_mut() = Some(Rc::clone(&proto_rc)));

    // ---- %IteratorPrototype% methods ----
    register_proto_method(&proto_rc, "map", iterator_map);
    register_proto_method(&proto_rc, "filter", iterator_filter);
    register_proto_method(&proto_rc, "take", iterator_take);
    register_proto_method(&proto_rc, "drop", iterator_drop);
    register_proto_method(&proto_rc, "flatMap", iterator_flat_map);
    register_proto_method(&proto_rc, "reduce", iterator_reduce);
    register_proto_method(&proto_rc, "toArray", iterator_to_array);
    register_proto_method(&proto_rc, "forEach", iterator_for_each);
    register_proto_method(&proto_rc, "some", iterator_some);
    register_proto_method(&proto_rc, "every", iterator_every);
    register_proto_method(&proto_rc, "find", iterator_find);

    // ---- Iterator constructor / namespace ----
    let iterator_fn = NativeFunction::new(iterator_constructor);
    let _ = iterator_fn.set_property(
        "prototype",
        Value::Object(Rc::clone(&proto_rc)),
    );
    let _ = iterator_fn.set_property("name", Value::String("Iterator".to_string()));

    // Iterator.from (static method)
    let from_fn = NativeFunction::new(iterator_from);
    let _ = iterator_fn.set_property("from", Value::NativeFunction(Rc::new(from_fn)));

    ctx.set_global(
        "Iterator".to_string(),
        Value::NativeFunction(Rc::new(iterator_fn)),
    );
}

fn register_proto_method(proto: &Rc<RefCell<Object>>, name: &str, f: fn(Vec<Value>) -> Result<Value, JsError>) {
    proto.borrow_mut().set(
        name,
        Value::NativeFunction(Rc::new(NativeFunction::new(f))),
    );
}

/// Iterator (constructor) — ES2025 §27.1.1.1
/// Returns an iterator wrapper over an iterable.
fn iterator_constructor(args: Vec<Value>) -> Result<Value, JsError> {
    let o = args.first().cloned().unwrap_or(Value::Undefined);
    iterator_from_internal(o)
}

/// Iterator.from — ES2025 §27.1.3.2
fn iterator_from(args: Vec<Value>) -> Result<Value, JsError> {
    let o = args.first().cloned().unwrap_or(Value::Undefined);
    iterator_from_internal(o)
}

/// Shared implementation for both Iterator() and Iterator.from().
fn iterator_from_internal(o: Value) -> Result<Value, JsError> {
    let obj = match o {
        Value::Object(o) => o,
        Value::String(s) => {
            // Wrap string as a simple iterator over characters
            let chars: Vec<Value> =
                s.chars().map(|c| Value::String(c.to_string())).collect();
            let mut result = Object::new(ObjectKind::Ordinary);
            let chars_rc = Rc::new(RefCell::new(chars));
            let index = Rc::new(RefCell::new(0usize));
            let chars_clone = Rc::clone(&chars_rc);
            let index_clone = Rc::clone(&index);
            let next_fn = NativeFunction::new(move |_args: Vec<Value>| {
                let mut i = index_clone.borrow_mut();
                let vals = chars_clone.borrow();
                if *i < vals.len() {
                    let v = vals[*i].clone();
                    *i += 1;
                    let mut obj = Object::new(ObjectKind::Ordinary);
                    obj.set("value", v);
                    obj.set("done", Value::Boolean(false));
                    Ok(Value::Object(Rc::new(RefCell::new(obj))))
                } else {
                    let mut obj = Object::new(ObjectKind::Ordinary);
                    obj.set("value", Value::Undefined);
                    obj.set("done", Value::Boolean(true));
                    Ok(Value::Object(Rc::new(RefCell::new(obj))))
                }
            });
            result.set("next", Value::NativeFunction(Rc::new(next_fn)));
            return Ok(Value::Object(Rc::new(RefCell::new(result))));
        }
        _ => {
            // For non-object iterables, try to get iterator
            let dummy = Object::new(ObjectKind::Ordinary);
            let dummy_rc = Rc::new(RefCell::new(dummy));
            match obtain_iterator(&dummy_rc) {
                Ok(iter) => return Ok(Value::Object(iter)),
                Err(_) => {
                    // Return empty iterator
                    let result = Object::new(ObjectKind::Ordinary);
                    let result_rc = Rc::new(RefCell::new(result));
                    let mut done_result = Object::new(ObjectKind::Ordinary);
                    done_result.set("value", Value::Undefined);
                    done_result.set("done", Value::Boolean(true));
                    let done_rc = Rc::new(RefCell::new(done_result));
                    let next_fn = NativeFunction::new(move |_args: Vec<Value>| {
                        Ok(Value::Object(Rc::clone(&done_rc)))
                    });
                    result_rc.borrow_mut().set("next", Value::NativeFunction(Rc::new(next_fn)));
                    return Ok(Value::Object(result_rc));
                }
            }
        }
    };

    // Try to get iterator from the object
    match obtain_iterator(&obj) {
        Ok(iter) => Ok(Value::Object(iter)),
        Err(e) => Err(e),
    }
}

/// Consume an iterator, collecting all values into a Vec.
fn consume_iterator(this_obj: &Rc<RefCell<Object>>) -> Result<Vec<Value>, JsError> {
    let mut values = Vec::new();
    let next = this_obj
        .borrow()
        .get("next")
        .ok_or_else(|| JsError("Iterator has no next method".to_string()))?;
    loop {
        let result = crate::eval::function::call_value_with_this(
            next.clone(),
            vec![],
            Value::Object(Rc::clone(this_obj)),
        )?;
        match result {
            Value::Object(o) => {
                let done = o
                    .borrow()
                    .get("done")
                    .map(|v| v == Value::Boolean(true))
                    .unwrap_or(false);
                if done {
                    break;
                }
                let value = o.borrow().get("value").unwrap_or(Value::Undefined);
                values.push(value);
            }
            _ => break,
        }
    }
    Ok(values)
}

/// Iterator.prototype.map — ES2025 §27.1.4.4
fn iterator_map(args: Vec<Value>) -> Result<Value, JsError> {
    let mapper = args
        .first()
        .ok_or_else(|| JsError("TypeError: undefined is not a function".to_string()))?;
    if !matches!(
        mapper,
        Value::Function(_) | Value::NativeFunction(_)
    ) {
        return Err(JsError("TypeError: undefined is not a function".to_string()));
    }

    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("TypeError: Iterator.prototype.map called with no this".to_string()))?;
    let this_obj = match this_val {
        Value::Object(o) => o,
        _ => return Err(JsError("TypeError: not an object".to_string())),
    };

    let values = consume_iterator(&this_obj)?;
    let mapped: Vec<Value> = values
        .into_iter()
        .map(|v| {
            crate::eval::function::call_value_with_this(
                mapper.clone(),
                vec![v],
                Value::Undefined,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut arr = Object::new(ObjectKind::Array);
    for (i, v) in mapped.into_iter().enumerate() {
        arr.set(&i.to_string(), v);
    }
    arr.set("length", Value::Number(arr.elements.len() as f64));

    let result = Object::new(ObjectKind::Ordinary);
    let result_rc = Rc::new(RefCell::new(result));
    let mut done_result = Object::new(ObjectKind::Ordinary);
    done_result.set("value", Value::Undefined);
    done_result.set("done", Value::Boolean(true));
    let done_rc = Rc::new(RefCell::new(done_result));

    let index = Rc::new(RefCell::new(0usize));
    let arr_vals: Vec<Value> = arr.elements.clone();
    let next_fn = NativeFunction::new(move |_args: Vec<Value>| {
        let mut i = index.borrow_mut();
        if *i < arr_vals.len() {
            let v = arr_vals[*i].clone();
            *i += 1;
            let mut obj = Object::new(ObjectKind::Ordinary);
            obj.set("value", v);
            obj.set("done", Value::Boolean(false));
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        } else {
            Ok(Value::Object(Rc::clone(&done_rc)))
        }
    });
    result_rc.borrow_mut().set("next", Value::NativeFunction(Rc::new(next_fn)));

    let arr_proto = crate::builtins::get_array_prototype();
    if let Some(p) = arr_proto {
        result_rc.borrow_mut().prototype = Some(p);
    }
    let proto = get_iterator_prototype();
    if let Some(p) = proto {
        result_rc.borrow_mut().prototype = Some(p);
    }

    Ok(Value::Object(result_rc))
}

/// Iterator.prototype.filter — ES2025 §27.1.4.3
fn iterator_filter(args: Vec<Value>) -> Result<Value, JsError> {
    let filterer = args
        .first()
        .ok_or_else(|| JsError("TypeError: undefined is not a function".to_string()))?;
    if !matches!(
        filterer,
        Value::Function(_) | Value::NativeFunction(_)
    ) {
        return Err(JsError("TypeError: undefined is not a function".to_string()));
    }

    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("TypeError: Iterator.prototype.filter called with no this".to_string()))?;
    let this_obj = match this_val {
        Value::Object(o) => o,
        _ => return Err(JsError("TypeError: not an object".to_string())),
    };

    let values = consume_iterator(&this_obj)?;
    let filtered: Vec<Value> = values
        .into_iter()
        .filter(|v| {
            let result = crate::eval::function::call_value_with_this(
                filterer.clone(),
                vec![v.clone()],
                Value::Undefined,
            );
            result.map(|r| r != Value::Boolean(false)).unwrap_or(false)
        })
        .collect();

    let result = Object::new(ObjectKind::Ordinary);
    let result_rc = Rc::new(RefCell::new(result));
    let mut done_result = Object::new(ObjectKind::Ordinary);
    done_result.set("value", Value::Undefined);
    done_result.set("done", Value::Boolean(true));
    let done_rc = Rc::new(RefCell::new(done_result));

    let index = Rc::new(RefCell::new(0usize));
    let filter_vals = filtered;
    let next_fn = NativeFunction::new(move |_args: Vec<Value>| {
        let mut i = index.borrow_mut();
        if *i < filter_vals.len() {
            let v = filter_vals[*i].clone();
            *i += 1;
            let mut obj = Object::new(ObjectKind::Ordinary);
            obj.set("value", v);
            obj.set("done", Value::Boolean(false));
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        } else {
            Ok(Value::Object(Rc::clone(&done_rc)))
        }
    });
    result_rc.borrow_mut().set("next", Value::NativeFunction(Rc::new(next_fn)));
    let proto = get_iterator_prototype();
    if let Some(p) = proto {
        result_rc.borrow_mut().prototype = Some(p);
    }

    Ok(Value::Object(result_rc))
}

/// Iterator.prototype.take — ES2025 §27.1.4.7
fn iterator_take(args: Vec<Value>) -> Result<Value, JsError> {
    let limit_val = args.first().map(to_number).unwrap_or(0.0);
    let limit = limit_val as usize;

    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("TypeError: Iterator.prototype.take called with no this".to_string()))?;
    let this_obj = match this_val {
        Value::Object(o) => o,
        _ => return Err(JsError("TypeError: not an object".to_string())),
    };

    let values = consume_iterator(&this_obj)?;
    let taken: Vec<Value> = values.into_iter().take(limit).collect();

    let result = Object::new(ObjectKind::Ordinary);
    let result_rc = Rc::new(RefCell::new(result));
    let mut done_result = Object::new(ObjectKind::Ordinary);
    done_result.set("value", Value::Undefined);
    done_result.set("done", Value::Boolean(true));
    let done_rc = Rc::new(RefCell::new(done_result));

    let index = Rc::new(RefCell::new(0usize));
    let taken_vals = taken;
    let next_fn = NativeFunction::new(move |_args: Vec<Value>| {
        let mut i = index.borrow_mut();
        if *i < taken_vals.len() {
            let v = taken_vals[*i].clone();
            *i += 1;
            let mut obj = Object::new(ObjectKind::Ordinary);
            obj.set("value", v);
            obj.set("done", Value::Boolean(false));
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        } else {
            Ok(Value::Object(Rc::clone(&done_rc)))
        }
    });
    result_rc.borrow_mut().set("next", Value::NativeFunction(Rc::new(next_fn)));
    let proto = get_iterator_prototype();
    if let Some(p) = proto {
        result_rc.borrow_mut().prototype = Some(p);
    }

    Ok(Value::Object(result_rc))
}

/// Iterator.prototype.drop — ES2025 §27.1.4.8
fn iterator_drop(args: Vec<Value>) -> Result<Value, JsError> {
    let limit_val = args.first().map(to_number).unwrap_or(0.0);
    let skip = limit_val as usize;

    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("TypeError: Iterator.prototype.drop called with no this".to_string()))?;
    let this_obj = match this_val {
        Value::Object(o) => o,
        _ => return Err(JsError("TypeError: not an object".to_string())),
    };

    let values = consume_iterator(&this_obj)?;
    let dropped: Vec<Value> = values.into_iter().skip(skip).collect();

    let result = Object::new(ObjectKind::Ordinary);
    let result_rc = Rc::new(RefCell::new(result));
    let mut done_result = Object::new(ObjectKind::Ordinary);
    done_result.set("value", Value::Undefined);
    done_result.set("done", Value::Boolean(true));
    let done_rc = Rc::new(RefCell::new(done_result));

    let index = Rc::new(RefCell::new(0usize));
    let dropped_vals = dropped;
    let next_fn = NativeFunction::new(move |_args: Vec<Value>| {
        let mut i = index.borrow_mut();
        if *i < dropped_vals.len() {
            let v = dropped_vals[*i].clone();
            *i += 1;
            let mut obj = Object::new(ObjectKind::Ordinary);
            obj.set("value", v);
            obj.set("done", Value::Boolean(false));
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        } else {
            Ok(Value::Object(Rc::clone(&done_rc)))
        }
    });
    result_rc.borrow_mut().set("next", Value::NativeFunction(Rc::new(next_fn)));
    let proto = get_iterator_prototype();
    if let Some(p) = proto {
        result_rc.borrow_mut().prototype = Some(p);
    }

    Ok(Value::Object(result_rc))
}

/// Iterator.prototype.flatMap — ES2025 §27.1.4.6
fn iterator_flat_map(args: Vec<Value>) -> Result<Value, JsError> {
    let mapper = args
        .first()
        .ok_or_else(|| JsError("TypeError: undefined is not a function".to_string()))?;
    if !matches!(
        mapper,
        Value::Function(_) | Value::NativeFunction(_)
    ) {
        return Err(JsError("TypeError: undefined is not a function".to_string()));
    }

    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("TypeError: Iterator.prototype.flatMap called with no this".to_string()))?;
    let this_obj = match this_val {
        Value::Object(o) => o,
        _ => return Err(JsError("TypeError: not an object".to_string())),
    };

    let values = consume_iterator(&this_obj)?;
    let mut flattened = Vec::new();
    for v in values {
        let mapped = crate::eval::function::call_value_with_this(
            mapper.clone(),
            vec![v],
            Value::Undefined,
        )?;
        // Flatten: if iterable, spread; else push single value
        if let Value::Object(o) = &mapped {
            match consume_iterator(o) {
                Ok(items) => flattened.extend(items),
                Err(_) => flattened.push(mapped),
            }
        } else {
            flattened.push(mapped);
        }
    }

    let result = Object::new(ObjectKind::Ordinary);
    let result_rc = Rc::new(RefCell::new(result));
    let mut done_result = Object::new(ObjectKind::Ordinary);
    done_result.set("value", Value::Undefined);
    done_result.set("done", Value::Boolean(true));
    let done_rc = Rc::new(RefCell::new(done_result));

    let index = Rc::new(RefCell::new(0usize));
    let flat_vals = flattened;
    let next_fn = NativeFunction::new(move |_args: Vec<Value>| {
        let mut i = index.borrow_mut();
        if *i < flat_vals.len() {
            let v = flat_vals[*i].clone();
            *i += 1;
            let mut obj = Object::new(ObjectKind::Ordinary);
            obj.set("value", v);
            obj.set("done", Value::Boolean(false));
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        } else {
            Ok(Value::Object(Rc::clone(&done_rc)))
        }
    });
    result_rc.borrow_mut().set("next", Value::NativeFunction(Rc::new(next_fn)));
    let proto = get_iterator_prototype();
    if let Some(p) = proto {
        result_rc.borrow_mut().prototype = Some(p);
    }

    Ok(Value::Object(result_rc))
}

/// Iterator.prototype.reduce — ES2025 §27.1.4.5
fn iterator_reduce(args: Vec<Value>) -> Result<Value, JsError> {
    let reducer = args
        .first()
        .ok_or_else(|| JsError("TypeError: undefined is not a function".to_string()))?;
    if !matches!(
        reducer,
        Value::Function(_) | Value::NativeFunction(_)
    ) {
        return Err(JsError("TypeError: undefined is not a function".to_string()));
    }

    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("TypeError: Iterator.prototype.reduce called with no this".to_string()))?;
    let this_obj = match this_val {
        Value::Object(o) => o,
        _ => return Err(JsError("TypeError: not an object".to_string())),
    };

    let values = consume_iterator(&this_obj)?;
    let mut iter = values.into_iter();

    match (args.get(1).cloned(), iter.next()) {
        (Some(acc), Some(v)) => {
            let mut accumulator = acc;
            accumulator = crate::eval::function::call_value_with_this(
                reducer.clone(),
                vec![accumulator, v],
                Value::Undefined,
            )?;
            for remaining in iter {
                accumulator = crate::eval::function::call_value_with_this(
                    reducer.clone(),
                    vec![accumulator, remaining],
                    Value::Undefined,
                )?;
            }
            Ok(accumulator)
        }
        (None, Some(v)) => {
            // No initialValue: first element is accumulator, rest are values
            let mut accumulator = v;
            for remaining in iter {
                accumulator = crate::eval::function::call_value_with_this(
                    reducer.clone(),
                    vec![accumulator, remaining],
                    Value::Undefined,
                )?;
            }
            Ok(accumulator)
        }
        (None, None) => Err(JsError("TypeError: Reduce of empty sequence with no initial value".to_string())),
        (Some(acc), None) => Ok(acc),
    }
}

/// Iterator.prototype.toArray — ES2025 §27.1.4.10
fn iterator_to_array(_args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("TypeError: Iterator.prototype.toArray called with no this".to_string()))?;
    let this_obj = match this_val {
        Value::Object(o) => o,
        _ => return Err(JsError("TypeError: not an object".to_string())),
    };

    let values = consume_iterator(&this_obj)?;
    let mut arr = Object::new(ObjectKind::Array);
    for v in values {
        arr.elements.push(v);
    }
    arr.set("length", Value::Number(arr.elements.len() as f64));

    Ok(Value::Object(Rc::new(RefCell::new(arr))))
}

/// Iterator.prototype.forEach — ES2025 §27.1.4.9
fn iterator_for_each(args: Vec<Value>) -> Result<Value, JsError> {
    let callback = args
        .first()
        .ok_or_else(|| JsError("TypeError: undefined is not a function".to_string()))?;
    if !matches!(
        callback,
        Value::Function(_) | Value::NativeFunction(_)
    ) {
        return Err(JsError("TypeError: undefined is not a function".to_string()));
    }
    let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("TypeError: Iterator.prototype.forEach called with no this".to_string()))?;
    let this_obj = match this_val {
        Value::Object(o) => o,
        _ => return Err(JsError("TypeError: not an object".to_string())),
    };

    let values = consume_iterator(&this_obj)?;
    for v in values {
        crate::eval::function::call_value_with_this(
            callback.clone(),
            vec![v],
            this_arg.clone(),
        )?;
    }
    Ok(Value::Undefined)
}

/// Iterator.prototype.some — ES2025 §27.1.4.11
fn iterator_some(args: Vec<Value>) -> Result<Value, JsError> {
    let callback = args
        .first()
        .ok_or_else(|| JsError("TypeError: undefined is not a function".to_string()))?;
    if !matches!(
        callback,
        Value::Function(_) | Value::NativeFunction(_)
    ) {
        return Err(JsError("TypeError: undefined is not a function".to_string()));
    }
    let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("TypeError: Iterator.prototype.some called with no this".to_string()))?;
    let this_obj = match this_val {
        Value::Object(o) => o,
        _ => return Err(JsError("TypeError: not an object".to_string())),
    };

    let values = consume_iterator(&this_obj)?;
    for v in values {
        let result = crate::eval::function::call_value_with_this(
            callback.clone(),
            vec![v],
            this_arg.clone(),
        )?;
        if result == Value::Boolean(true) {
            return Ok(Value::Boolean(true));
        }
    }
    Ok(Value::Boolean(false))
}

/// Iterator.prototype.every — ES2025 §27.1.4.12
fn iterator_every(args: Vec<Value>) -> Result<Value, JsError> {
    let callback = args
        .first()
        .ok_or_else(|| JsError("TypeError: undefined is not a function".to_string()))?;
    if !matches!(
        callback,
        Value::Function(_) | Value::NativeFunction(_)
    ) {
        return Err(JsError("TypeError: undefined is not a function".to_string()));
    }
    let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("TypeError: Iterator.prototype.every called with no this".to_string()))?;
    let this_obj = match this_val {
        Value::Object(o) => o,
        _ => return Err(JsError("TypeError: not an object".to_string())),
    };

    let values = consume_iterator(&this_obj)?;
    for v in values {
        let result = crate::eval::function::call_value_with_this(
            callback.clone(),
            vec![v],
            this_arg.clone(),
        )?;
        if result != Value::Boolean(true) {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

/// Iterator.prototype.find — ES2025 §27.1.4.13
fn iterator_find(args: Vec<Value>) -> Result<Value, JsError> {
    let callback = args
        .first()
        .ok_or_else(|| JsError("TypeError: undefined is not a function".to_string()))?;
    if !matches!(
        callback,
        Value::Function(_) | Value::NativeFunction(_)
    ) {
        return Err(JsError("TypeError: undefined is not a function".to_string()));
    }
    let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

    let this_val = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("TypeError: Iterator.prototype.find called with no this".to_string()))?;
    let this_obj = match this_val {
        Value::Object(o) => o,
        _ => return Err(JsError("TypeError: not an object".to_string())),
    };

    let values = consume_iterator(&this_obj)?;
    for v in values {
        let result = crate::eval::function::call_value_with_this(
            callback.clone(),
            vec![v.clone()],
            this_arg.clone(),
        )?;
        if result == Value::Boolean(true) {
            return Ok(v);
        }
    }
    Ok(Value::Undefined)
}
