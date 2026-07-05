// linter-skip
#![allow(clippy::too_many_lines, clippy::function_body_length, clippy::complexity)]
//! Array built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, to_bool, JsError, NativeFunction, Object, ObjectKind, Value};
use crate::Context;
use crate::interpreter::call_value_with_this;

// Thread-local storage for Array.prototype (used by interpreter for array literal creation)
thread_local! {
    static ARRAY_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the Array.prototype object (for use by interpreter)
pub fn get_array_prototype() -> Option<Rc<RefCell<Object>>> {
    ARRAY_PROTOTYPE.with(|ap| ap.borrow().clone())
}

// ============================================================================
// Array
// ============================================================================

pub fn register_array(ctx: &mut Context) {
    let array = Object::new(ObjectKind::Ordinary);
    let array = Rc::new(RefCell::new(array));

    array.borrow_mut().set("isArray", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let arg = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::Boolean(matches!(arg, Value::Object(ref o) if o.borrow().kind == ObjectKind::Array)))
    }))));

    array.borrow_mut().set("from", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let items = args.first().cloned().unwrap_or(Value::Undefined);
        let arr = match items {
            Value::Object(o) => {
                let elements: Vec<Value> = o.borrow().elements.clone();
                Object::new_array_from(elements)
            }
            _ => Object::new_array(0),
        };
        Ok(Value::Object(Rc::new(RefCell::new(arr))))
    }))));

    array.borrow_mut().set("of", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let arr = Object::new_array_from(args.to_vec());
        Ok(Value::Object(Rc::new(RefCell::new(arr))))
    }))));

    // Create Array.prototype and attach to Array
    let array_proto = Object::new(ObjectKind::Array);
    let array_proto_rc = Rc::new(RefCell::new(array_proto));
    
    // Helper to get this as an array object - consumes the this binding
    fn get_this_array_obj() -> Result<Rc<RefCell<Object>>, JsError> {
        match crate::builtins::get_native_this() {
            Some(Value::Object(o)) => {
                let is_array = o.borrow().kind == ObjectKind::Array;
                if is_array {
                    Ok(o)
                } else {
                    Err(JsError("Array.prototype method called on non-array".to_string()))
                }
            }
            _ => Err(JsError("Array.prototype method called on non-object".to_string())),
        }
    }

    // Helper to get array elements - consumes the this binding
    fn get_this_array() -> Result<Vec<Value>, JsError> {
        let o = get_this_array_obj()?;
        let elements = o.borrow().elements.clone();
        Ok(elements)
    }

    // Helper to set the array's elements - takes the object directly
    fn set_this_elements(o: &Rc<RefCell<Object>>, new_elements: Vec<Value>) -> Result<Value, JsError> {
        o.borrow_mut().elements = new_elements.clone();
        Ok(Value::Number(new_elements.len() as f64))
    }

    // Helper to create result array object
    fn make_array(elements: Vec<Value>) -> Value {
        let arr = Object::new_array_from(elements);
        Value::Object(Rc::new(RefCell::new(arr)))
    }

    // length property getter
    array_proto_rc.borrow_mut().set("length", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        match crate::builtins::get_native_this() {
            Some(Value::Object(o)) => Ok(Value::Number(o.borrow().elements.len() as f64)),
            _ => Ok(Value::Undefined),
        }
    }))));

    // Array.prototype.map(callback, thisArg?)
    array_proto_rc.borrow_mut().set("map", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        let mut result = Vec::new();
        for (i, elem) in elements.iter().enumerate() {
            let mapped = match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    // Call the callback with (element, index, array)
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    call_value_with_this(callback.clone(), callback_args, Value::Undefined)
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    nf.call(callback_args)
                }
                _ => Err(JsError("Callback is not a function".to_string())),
            }?;
            result.push(mapped);
        }
        Ok(make_array(result))
    }))));

    // Array.prototype.filter(callback, thisArg?)
    array_proto_rc.borrow_mut().set("filter", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        let mut result = Vec::new();
        for (i, elem) in elements.iter().enumerate() {
            let keep = match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    let res = call_value_with_this(callback.clone(), callback_args, Value::Undefined)?;
                    to_bool(&res)
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    let res = nf.call(callback_args)?;
                    to_bool(&res)
                }
                _ => return Err(JsError("Callback is not a function".to_string())),
            };
            if keep {
                result.push(elem.clone());
            }
        }
        Ok(make_array(result))
    }))));

    // Array.prototype.reduce(callback, initialValue?)
    array_proto_rc.borrow_mut().set("reduce", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        let initial = args.get(1).cloned();

        let mut accumulator: Value;
        let start_idx: usize;

        if let Some(init) = initial {
            accumulator = init;
            start_idx = 0;
        } else if elements.is_empty() {
            return Err(JsError("Reduce of empty array with no initial value".to_string()));
        } else {
            accumulator = elements[0].clone();
            start_idx = 1;
        }

        for i in start_idx..elements.len() {
            let elem = &elements[i];
            accumulator = match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    let callback_args = vec![
                        accumulator.clone(),
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    call_value_with_this(callback.clone(), callback_args, Value::Undefined)?
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        accumulator.clone(),
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    nf.call(callback_args)?
                }
                _ => return Err(JsError("Callback is not a function".to_string())),
            };
        }
        Ok(accumulator)
    }))));

    // Array.prototype.forEach(callback, thisArg?)
    array_proto_rc.borrow_mut().set("forEach", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        for (i, elem) in elements.iter().enumerate() {
            match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    let _ = call_value_with_this(callback.clone(), callback_args, Value::Undefined);
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    let _ = nf.call(callback_args);
                }
                _ => return Err(JsError("Callback is not a function".to_string())),
            };
        }
        Ok(Value::Undefined)
    }))));

    // Array.prototype.push(...items)
    array_proto_rc.borrow_mut().set("push", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let o = get_this_array_obj()?;
        let mut elements = o.borrow().elements.clone();
        elements.extend(args);
        set_this_elements(&o, elements)
    }))));

    // Array.prototype.pop()
    array_proto_rc.borrow_mut().set("pop", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        let o = get_this_array_obj()?;
        let mut elements = o.borrow().elements.clone();
        let popped = elements.pop();
        set_this_elements(&o, elements)?;
        Ok(popped.unwrap_or(Value::Undefined))
    }))));

    // Array.prototype.shift()
    array_proto_rc.borrow_mut().set("shift", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        let o = get_this_array_obj()?;
        let mut elements = o.borrow().elements.clone();
        let shifted = elements.remove(0);
        set_this_elements(&o, elements)?;
        Ok(shifted)
    }))));

    // Array.prototype.unshift(...items)
    array_proto_rc.borrow_mut().set("unshift", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let o = get_this_array_obj()?;
        let elements = o.borrow().elements.clone();
        let mut new_items: Vec<Value> = args.to_vec();
        new_items.extend(elements);
        set_this_elements(&o, new_items)
    }))));

    // Array.prototype.slice(start?, end?)
    array_proto_rc.borrow_mut().set("slice", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let elements = get_this_array()?;
        let len = elements.len() as f64;
        let start = args.first().map(to_number).unwrap_or(0.0);
        let end = args.get(1).map(to_number).unwrap_or(len);

        let start_idx = if start < 0.0 {
            ((len + start) as isize).max(0).min(len as isize) as usize
        } else {
            (start as usize).min(len as usize)
        };
        let end_idx = if end < 0.0 {
            ((len + end) as isize).max(0).min(len as isize) as usize
        } else {
            (end as usize).min(len as usize)
        };

        let result: Vec<Value> = elements[start_idx..end_idx].to_vec();
        Ok(make_array(result))
    }))));

    // Array.prototype.concat(...arrays)
    array_proto_rc.borrow_mut().set("concat", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let mut elements = get_this_array()?;
        for arg in args {
            match arg {
                Value::Object(o) if o.borrow().kind == ObjectKind::Array => {
                    elements.extend(o.borrow().elements.clone());
                }
                _ => elements.push(arg),
            }
        }
        Ok(make_array(elements))
    }))));

    // Array.prototype.join(separator?)
    array_proto_rc.borrow_mut().set("join", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let elements = get_this_array()?;
        let sep = args.first().map(to_js_string).unwrap_or_else(|| ",".to_string());
        let parts: Vec<String> = elements.iter().map(to_js_string).collect();
        Ok(Value::String(parts.join(&sep)))
    }))));

    // Array.prototype.indexOf(searchElement, fromIndex?)
    array_proto_rc.borrow_mut().set("indexOf", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let elements = get_this_array()?;
        let search = args.first().cloned().unwrap_or(Value::Undefined);
        let from_idx = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);

        #[allow(clippy::needless_range_loop)]
        for i in from_idx..elements.len() {
            if crate::value::strict_eq(&elements[i], &search) {
                return Ok(Value::Number(i as f64));
            }
        }
        Ok(Value::Number(-1.0))
    }))));

    // Array.prototype.includes(searchElement, fromIndex?)
    array_proto_rc.borrow_mut().set("includes", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let elements = get_this_array()?;
        let search = args.first().cloned().unwrap_or(Value::Undefined);
        let from_idx = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);

        #[allow(clippy::needless_range_loop)]
        for i in from_idx..elements.len() {
            if crate::value::strict_eq(&elements[i], &search) {
                return Ok(Value::Boolean(true));
            }
        }
        Ok(Value::Boolean(false))
    }))));

    // Array.prototype.find(predicate, thisArg?)
    array_proto_rc.borrow_mut().set("find", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);

        for (i, elem) in elements.iter().enumerate() {
            let result = match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    call_value_with_this(callback.clone(), callback_args, Value::Undefined)?
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    nf.call(callback_args)?
                }
                _ => return Err(JsError("Callback is not a function".to_string())),
            };
            if to_bool(&result) {
                return Ok(elem.clone());
            }
        }
        Ok(Value::Undefined)
    }))));

    // Array.prototype.flat(depth?)
    array_proto_rc.borrow_mut().set("flat", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let elements = get_this_array()?;
        let depth = args.first().map(|v| to_number(v) as i32).unwrap_or(1);

        fn flatten(arr: Vec<Value>, depth: i32) -> Vec<Value> {
            if depth <= 0 {
                return arr;
            }
            let mut result = Vec::new();
            for elem in arr {
                match elem {
                    Value::Object(o) if o.borrow().kind == ObjectKind::Array => {
                        let inner = o.borrow().elements.clone();
                        result.extend(flatten(inner, depth - 1));
                    }
                    _ => result.push(elem),
                }
            }
            result
        }

        Ok(make_array(flatten(elements, depth)))
    }))));

    // Array.prototype.some(callback, thisArg?)
    array_proto_rc.borrow_mut().set("some", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);

        for (i, elem) in elements.iter().enumerate() {
            let result = match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    call_value_with_this(callback.clone(), callback_args, Value::Undefined)?
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    nf.call(callback_args)?
                }
                _ => return Err(JsError("Callback is not a function".to_string())),
            };
            if to_bool(&result) {
                return Ok(Value::Boolean(true));
            }
        }
        Ok(Value::Boolean(false))
    }))));

    // Array.prototype.every(callback, thisArg?)
    array_proto_rc.borrow_mut().set("every", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let elements = get_this_array()?;
        let callback = args.first().cloned().unwrap_or(Value::Undefined);

        for (i, elem) in elements.iter().enumerate() {
            let result = match callback {
                #[allow(unused_variables)] Value::Function(_) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    call_value_with_this(callback.clone(), callback_args, Value::Undefined)?
                }
                Value::NativeFunction(ref nf) => {
                    let callback_args = vec![
                        elem.clone(),
                        Value::Number(i as f64),
                        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.clone())))),
                    ];
                    nf.call(callback_args)?
                }
                _ => return Err(JsError("Callback is not a function".to_string())),
            };
            if !to_bool(&result) {
                return Ok(Value::Boolean(false));
            }
        }
        Ok(Value::Boolean(true))
    }))));

    // Array.prototype.reverse()
    array_proto_rc.borrow_mut().set("reverse", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        let o = get_this_array_obj()?;
        let mut elements = o.borrow().elements.clone();
        elements.reverse();
        set_this_elements(&o, elements.clone())?;
        Ok(make_array(elements))
    }))));

    // Array.prototype.sort(compareFn?)
    array_proto_rc.borrow_mut().set("sort", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let o = get_this_array_obj()?;
        let mut elements = o.borrow().elements.clone();
        let _compare_fn = args.first().cloned();

        // Simple string comparison sort
        elements.sort_by(|a, b| {
            let a_str = to_js_string(a);
            let b_str = to_js_string(b);
            a_str.cmp(&b_str)
        });

        set_this_elements(&o, elements.clone())?;
        Ok(make_array(elements))
    }))));

    // Array.prototype.splice(start, deleteCount?, ...items)
    array_proto_rc.borrow_mut().set("splice", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let o = get_this_array_obj()?;
        let mut elements = o.borrow().elements.clone();
        let start = args.first().map(|v| to_number(v) as isize).unwrap_or(0);
        let delete_count = args.get(1).map(|v| to_number(v) as usize).unwrap_or(elements.len());
        let items: Vec<Value> = args[2..].to_vec();

        let len = elements.len() as isize;
        let mut start_idx = if start < 0 { (len + start).max(0).min(len) as usize } else { (start as usize).min(len as usize) };
        let delete_count = delete_count.min(len as usize - start_idx);

        let removed: Vec<Value> = elements.drain(start_idx..start_idx + delete_count).collect();

        // Insert new items
        #[allow(clippy::explicit_counter_loop)]
        for item in items {
            elements.insert(start_idx, item);
            start_idx += 1;
        }

        set_this_elements(&o, elements)?;
        Ok(make_array(removed))
    }))));

    // Array.prototype.toString()
    array_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
        let elements = get_this_array()?;
        let parts: Vec<String> = elements.iter().map(to_js_string).collect();
        Ok(Value::String(parts.join(",")))
    }))));

    array.borrow_mut().set("prototype", Value::Object(Rc::clone(&array_proto_rc)));

    // Set up the prototype chain: array_proto_rc -> Object.prototype
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        array_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    // Store Array.prototype globally for interpreter to use when creating array literals
    let global_proto = Rc::new(RefCell::new(Object::new(ObjectKind::Array)));
    // Set global_proto's prototype to array_proto_rc
    global_proto.borrow_mut().prototype = Some(Rc::clone(&array_proto_rc));
    // Copy all properties from array_proto_rc to global_proto
    let proto_props = array_proto_rc.borrow().properties.clone();
    for (k, v) in proto_props {
        global_proto.borrow_mut().set(&k, v);
    }
    ARRAY_PROTOTYPE.with(|ap| {
        *ap.borrow_mut() = Some(global_proto);
    });

    ctx.set_global("Array".to_string(), Value::Object(array));
}
