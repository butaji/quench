//! Array built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{
    JsError, NativeConstructor, NativeFunction, Object, ObjectKind, PropertyFlags, Value,
};
use crate::Context;

/// Maximum length accepted by the Array constructor before it would
/// materialize an unreasonable number of elements (2^20).
const MAX_ARRAY_LENGTH: f64 = 1_048_576.0;

/// Reject array lengths that are too large to materialize with a RangeError.
fn check_array_length(n: f64) -> Result<(), JsError> {
    if n > MAX_ARRAY_LENGTH {
        let (_, js_err) =
            crate::value::error::create_js_error_with_type("Invalid array length", "RangeError");
        return Err(js_err);
    }
    Ok(())
}

// Thread-local storage for Array.prototype (used by interpreter for array literal creation)
thread_local! {
    static ARRAY_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the Array.prototype object (for use by interpreter)
pub fn get_array_prototype() -> Option<Rc<RefCell<Object>>> {
    ARRAY_PROTOTYPE.with(|ap| ap.borrow().clone())
}

/// Save the thread-local prototype cache (realm snapshot support)
pub(crate) fn save_array_prototype() -> Option<Rc<RefCell<Object>>> {
    get_array_prototype()
}

/// Restore the thread-local prototype cache (realm snapshot support)
pub(crate) fn restore_array_prototype(proto: Option<Rc<RefCell<Object>>>) {
    ARRAY_PROTOTYPE.with(|ap| *ap.borrow_mut() = proto);
}

// ============================================================================
// Array
// ============================================================================

pub fn register_array(ctx: &mut Context) {
    let array_proto = Object::new(ObjectKind::Array);
    let array_proto_rc = Rc::new(RefCell::new(array_proto));

    setup_array_length_getter(&array_proto_rc);

    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        array_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    setup_array_prototype_global(&array_proto_rc);

    // Build the NativeConstructor first so we can reference it for both
    // the global binding and the prototype.constructor property.
    let array_proto_for_ctor = Rc::clone(&array_proto_rc);
    let array_constructor = NativeConstructor::new(
        move |args: Vec<Value>| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if crate::interpreter::get_new_target().is_some() {
                if let Value::Object(obj_rc) = this_val {
                    return make_array_with_new(obj_rc, &args, &array_proto_for_ctor);
                }
            }
            make_array_direct(&args, &array_proto_for_ctor)
        },
        Rc::clone(&array_proto_rc),
    );
    array_constructor.set_name("Array");

    // Set static methods on the constructor
    let mut array_from_async = NativeFunction::new(|args| {
        if let Some(map_fn) = args.get(1) {
            if !matches!(map_fn, Value::Undefined) && !map_fn.is_callable() {
                let (error, _) = crate::value::error::create_js_error_with_type(
                    "Array.fromAsync map function is not callable",
                    "TypeError",
                );
                let promise = crate::builtins::promise::create_pending_promise();
                crate::builtins::promise::settle_reject(&promise, error);
                return Ok(Value::Object(promise));
            }
        }
        if args.get(1).is_some()
            && matches!(args.first(), Some(Value::Object(items)) if items.borrow().kind == ObjectKind::Array)
        {
            return array_from_async_mapped(&args);
        }
        if args.get(1).is_none() {
            if let Some(Value::Object(items)) = args.first() {
                if items.borrow().kind == ObjectKind::Array {
                    let initial_len = items.borrow().elements.len();
                    let first = items.borrow().elements.first().cloned();
                    let promise = crate::builtins::promise::create_pending_promise();
                    let items_for_job = Rc::clone(items);
                    let promise_for_job = Rc::clone(&promise);
                    let job = Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
                        let mut values = items_for_job.borrow().elements.clone();
                        values.truncate(initial_len);
                        if let Some(first) = first.clone() {
                            if values.is_empty() {
                                values.push(first);
                            } else {
                                values[0] = first;
                            }
                        }
                        crate::builtins::promise::settle_resolve(
                            &promise_for_job,
                            Value::Object(Rc::new(RefCell::new(Object::new_array_from(values)))),
                        );
                        Ok(Value::Undefined)
                    })));
                    crate::builtins::promise::queue_microtask_impl(job);
                    return Ok(Value::Object(promise));
                }
            }
        }
        match array_from_impl(args) {
        Ok(array) if array_contains_thenable(&array) => {
            let promises = array_to_promise_values(&array)?;
            crate::builtins::promise::promise_all_impl(
                vec![promises],
                crate::builtins::promise::get_promise_proto(),
            )
        }
        Ok(array) if array_contains_promise(&array) => crate::builtins::promise::promise_all_impl(
            vec![array],
            crate::builtins::promise::get_promise_proto(),
        ),
        Ok(array) => crate::builtins::promise::promise_resolve_impl_static(
            vec![array],
            crate::builtins::promise::get_promise_proto(),
        ),
        Err(error) => {
            let reason = crate::value::get_thrown_value().unwrap_or_else(|| Value::String(error.0));
            crate::builtins::promise::promise_reject_impl_static(
                vec![reason],
                crate::builtins::promise::get_promise_proto(),
            )
        }
        }
    });
    define_static_method_length(&mut array_from_async);
    array_constructor.set_static_method(
        "fromAsync",
        Value::NativeFunction(Rc::new(array_from_async)),
    );

    // Set Array.prototype.constructor = the NativeConstructor (non-enumerable)
    let array_constructor_rc = Rc::new(array_constructor.clone());
    array_proto_rc.borrow_mut().set_builtin_method(
        "constructor",
        Value::NativeConstructor(Rc::clone(&array_constructor_rc)),
    );

    // Register Array as the NativeConstructor (typeof returns "function")
    ctx.set_global(
        "Array".to_string(),
        Value::NativeConstructor(array_constructor_rc),
    );
}

fn array_contains_thenable(array: &Value) -> bool {
    let Value::Object(array) = array else {
        return false;
    };
    array.borrow().elements.iter().any(|value| {
        matches!(value, Value::Object(object) if object.borrow().get("then").is_some_and(|then| then.is_callable()))
    })
}

fn array_to_promise_values(array: &Value) -> Result<Value, JsError> {
    let Value::Object(array) = array else {
        return Ok(array.clone());
    };
    let values = array
        .borrow()
        .elements
        .iter()
        .map(|value| {
            if matches!(value, Value::Object(object) if object.borrow().get("then").is_some_and(|then| then.is_callable())) {
                crate::builtins::promise::promise_resolve_impl_static(
                    vec![value.clone()],
                    crate::builtins::promise::get_promise_proto(),
                )
            } else {
                Ok(value.clone())
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Object(Rc::new(RefCell::new(
        Object::new_array_from(values),
    ))))
}

fn array_contains_promise(array: &Value) -> bool {
    let Value::Object(array) = array else {
        return false;
    };
    array.borrow().elements.iter().any(
        |value| matches!(value, Value::Object(object) if object.borrow().promise_data.is_some()),
    )
}

struct AsyncMapState {
    items: Value,
    map_fn: Value,
    this_arg: Value,
    values: Vec<Value>,
    index: usize,
    result: Rc<RefCell<Object>>,
}

fn array_from_async_mapped(args: &[Value]) -> Result<Value, JsError> {
    let Value::Object(items) = args.first().cloned().unwrap_or(Value::Undefined) else {
        return Err(JsError::from("Array.fromAsync requires an object"));
    };
    if items.borrow().kind != ObjectKind::Array {
        return Err(JsError::from("not an array"));
    }
    let result = crate::builtins::promise::create_pending_promise();
    let map_fn = args.get(1).cloned().unwrap_or(Value::Undefined);
    if !map_fn.is_callable() {
        let (error, _) = crate::value::error::create_js_error_with_type(
            "Array.fromAsync map function is not callable",
            "TypeError",
        );
        crate::builtins::promise::settle_reject(&result, error);
        return Ok(Value::Object(result));
    }
    let state = Rc::new(RefCell::new(AsyncMapState {
        items: Value::Object(Rc::clone(&items)),
        map_fn,
        this_arg: args.get(2).cloned().unwrap_or(Value::Undefined),
        values: Vec::new(),
        index: 0,
        result: Rc::clone(&result),
    }));
    array_from_async_map_step(state);
    Ok(Value::Object(result))
}

fn array_from_async_map_step(state: Rc<RefCell<AsyncMapState>>) {
    let (items, map_fn, this_arg, index, done) = {
        let state_ref = state.borrow();
        let Value::Object(items) = &state_ref.items else {
            return;
        };
        let done = state_ref.index >= items.borrow().elements.len();
        (
            state_ref.items.clone(),
            state_ref.map_fn.clone(),
            state_ref.this_arg.clone(),
            state_ref.index,
            done,
        )
    };
    if done {
        let values = state.borrow().values.clone();
        crate::builtins::promise::settle_resolve(
            &state.borrow().result,
            Value::Object(Rc::new(RefCell::new(Object::new_array_from(values)))),
        );
        return;
    }
    let value = match &items {
        Value::Object(items) => items.borrow().elements[index].clone(),
        _ => return,
    };
    let mapped = match crate::eval::call_value_with_this(
        map_fn,
        vec![value, Value::Number(index as f64), items.clone()],
        this_arg,
    ) {
        Ok(value) => value,
        Err(error) => {
            let reason = crate::value::get_thrown_value()
                .unwrap_or_else(|| Value::String(error.0));
            crate::builtins::promise::settle_reject(&state.borrow().result, reason);
            return;
        }
    };
    let promise = match crate::builtins::promise::promise_resolve_impl_static(
        vec![mapped],
        crate::builtins::promise::get_promise_proto(),
    ) {
        Ok(Value::Object(promise)) => promise,
        Ok(_) | Err(_) => return,
    };
    let then = match crate::eval::member::eval_object_member(&promise, "then", None) {
        Ok(then) => then,
        Err(error) => {
            crate::builtins::promise::settle_reject(
                &state.borrow().result,
                Value::String(error.0),
            );
            return;
        }
    };
    let fulfilled_state = Rc::clone(&state);
    let rejected_state = Rc::clone(&state);
    let fulfilled = Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        fulfilled_state
            .borrow_mut()
            .values
            .push(args.first().cloned().unwrap_or(Value::Undefined));
        fulfilled_state.borrow_mut().index += 1;
        array_from_async_map_step(Rc::clone(&fulfilled_state));
        Ok(Value::Undefined)
    })));
    let rejected = Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        crate::builtins::promise::settle_reject(
            &rejected_state.borrow().result,
            args.first().cloned().unwrap_or(Value::Undefined),
        );
        Ok(Value::Undefined)
    })));
    let _ = crate::eval::call_value_with_this(
        then,
        vec![fulfilled, rejected],
        Value::Object(promise),
    );
}

fn array_from_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let items = args.first().cloned().unwrap_or(Value::Undefined);
    let original_items = items.clone();
    let mut map_fn = args.get(1).cloned();
    let this_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
    let iterable = if matches!(items, Value::Object(ref object) if object.borrow().kind != ObjectKind::Array)
    {
        let has_iterator = crate::builtins::map::helpers::iterator_prop_key()
            .and_then(|key| match &items {
                Value::Object(object) => object.borrow().get(&key),
                _ => None,
            })
            .is_some();
        if has_iterator {
            if let Some(function) = map_fn.clone() {
                let values = array_from_iterable_with_map(&items, function, this_arg.clone())?;
                map_fn = None;
                Some(values)
            } else {
                Some(crate::eval::iteration::get_iterator(&items)?)
            }
        } else {
            None
        }
    } else {
        None
    };
    let elements = match iterable {
        Some(values) => values,
        None => match items {
            Value::Object(object) if object.borrow().kind == ObjectKind::Array => {
                object.borrow().elements.clone()
            }
            Value::Object(object) => {
                let length = crate::eval::member::eval_object_member_value(
                    &object,
                    &Value::String("length".to_string()),
                    None,
                )
                .ok()
                .and_then(|value| crate::value::try_to_number(&value).ok())
                .unwrap_or(0.0)
                .max(0.0) as usize;
                (0..length)
                    .map(|index| {
                        crate::eval::member::eval_object_member_value(
                            &object,
                            &Value::String(index.to_string()),
                            None,
                        )
                        .unwrap_or(Value::Undefined)
                    })
                    .collect()
            }
            _ => Vec::new(),
        },
    };
    let elements = elements
        .into_iter()
        .map(await_fulfilled_promise)
        .collect::<Vec<_>>();
    let elements = if let Some(map_fn) = map_fn {
        elements
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let call_args = vec![value, Value::Number(index as f64), original_items.clone()];
                match &map_fn {
                    Value::Function(_) => crate::eval::call_value_with_this(
                        map_fn.clone(),
                        call_args,
                        this_arg.clone(),
                    ),
                    Value::NativeFunction(function) => function.call(this_arg.clone(), call_args),
                    _ => Err(JsError(
                        "Array.from map function is not callable".to_string(),
                    )),
                }
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        elements
    };
    Ok(Value::Object(Rc::new(RefCell::new(
        Object::new_array_from(elements),
    ))))
}

fn await_fulfilled_promise(value: Value) -> Value {
    let Value::Object(object) = &value else {
        return value;
    };
    let fulfilled = object.borrow().promise_data.as_ref().and_then(|data| {
        (data.state == crate::value::object::PromiseState::Fulfilled).then(|| data.result.clone())
    });
    fulfilled.unwrap_or(value)
}

fn array_from_iterable_with_map(
    items: &Value,
    map_fn: Value,
    this_arg: Value,
) -> Result<Vec<Value>, JsError> {
    let Value::Object(object) = items else {
        return Ok(Vec::new());
    };
    let environment = Rc::new(RefCell::new(crate::env::Environment::new()));
    let iterator = crate::eval::object::obtain_iterator(object)?;
    let mut index = 0usize;
    let mut values = Vec::new();
    loop {
        let (value, done) =
            crate::eval::object::take_iterator_step(&iterator, &mut index, &environment)?;
        if done {
            return Ok(values);
        }
        let call_args = vec![value, Value::Number(values.len() as f64), items.clone()];
        let mapped = match &map_fn {
            Value::Function(_) => {
                crate::eval::call_value_with_this(map_fn.clone(), call_args, this_arg.clone())
            }
            Value::NativeFunction(function) => function.call(this_arg.clone(), call_args),
            _ => Err(JsError(
                "Array.from map function is not callable".to_string(),
            )),
        };
        match mapped {
            Ok(value) => values.push(value),
            Err(error) => {
                let _ = crate::eval::object::call_iterator_return(&iterator);
                return Err(error);
            }
        }
    }
}

fn define_static_method_length(function: &mut NativeFunction) {
    define_method_length(function, 1.0);
}

fn define_method_length(function: &mut NativeFunction, length: f64) {
    function.define_property(
        "length",
        Value::Number(length),
        PropertyFlags {
            value: Some(Value::Number(length)),
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
}

fn make_array_with_new(
    obj_rc: Rc<RefCell<Object>>,
    args: &[Value],
    proto: &Rc<RefCell<Object>>,
) -> Result<Value, JsError> {
    let mut obj = obj_rc.borrow_mut();
    if obj.prototype.is_none() {
        obj.prototype = Some(Rc::clone(proto));
    }
    obj.kind = ObjectKind::Array;
    obj.elements.clear();
    obj.holes.clear();
    obj.properties
        .retain(|key, _| key == "length" || key.parse::<usize>().is_err());
    obj.descriptors
        .retain(|key, _| key == "length" || key.parse::<usize>().is_err());
    if args.len() == 1 {
        if let Value::Number(n) = args[0] {
            if n == n.floor() && (0.0..4294967296.0).contains(&n) {
                check_array_length(n)?;
                obj.elements = vec![Value::Undefined; n as usize];
                obj.holes.extend(0..n as usize);
                obj.define_array_length(n);
            } else {
                return Err(JsError("Invalid array length".to_string()));
            }
        } else {
            obj.elements = vec![args[0].clone()];
            obj.define_array_length(1.0);
        }
    } else if args.len() > 1 {
        obj.elements = args.to_vec();
        obj.define_array_length(args.len() as f64);
    }
    drop(obj);
    Ok(Value::Object(obj_rc))
}

#[cfg(test)]
mod tests {
    use crate::{Context, Value};

    #[test]
    fn array_of_has_standard_length_descriptor() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var d=Object.getOwnPropertyDescriptor(Array,'of'); [d.value.length,d.value===Array.of,d.writable,d.enumerable,d.configurable].join('|')"),
            Ok(Value::String("0|true|false|false|true".to_string()))
        );
    }

    #[test]
    fn array_from_uses_this_arg_for_mapping_callback() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Array.from([1], function () { return this.value; }, {value: 7})[0]"),
            Ok(Value::Number(7.0))
        );
    }
}

fn make_array_direct(args: &[Value], proto: &Rc<RefCell<Object>>) -> Result<Value, JsError> {
    let mut arr = if args.is_empty() {
        Object::new_array(0)
    } else if args.len() == 1 {
        if let Value::Number(n) = args[0] {
            if n == n.floor() && (0.0..4294967296.0).contains(&n) {
                check_array_length(n)?;
                Object::new_array(n as usize)
            } else {
                return Err(JsError("Invalid array length".to_string()));
            }
        } else {
            Object::new_array_from(vec![args[0].clone()])
        }
    } else {
        Object::new_array_from(args.to_vec())
    };
    arr.prototype = Some(Rc::clone(proto));
    Ok(Value::Object(Rc::new(RefCell::new(arr))))
}

fn setup_array_length_getter(array_proto: &Rc<RefCell<Object>>) {
    array_proto.borrow_mut().set(
        "length",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |_| {
            match crate::builtins::get_native_this() {
                Some(Value::Object(o)) => Ok(Value::Number(o.borrow().elements.len() as f64)),
                _ => Ok(Value::Undefined),
            }
        }))),
    );
}

fn setup_array_prototype_global(array_proto: &Rc<RefCell<Object>>) {
    ARRAY_PROTOTYPE.with(|ap| {
        *ap.borrow_mut() = Some(Rc::clone(array_proto));
    });
}

/// Wire `Array.prototype[Symbol.iterator]` after `Symbol` is registered.
pub fn register_array_iterator() {
    let Some(array_proto) = get_array_prototype() else {
        return;
    };
    let Some(Value::Symbol(sym)) =
        crate::builtins::symbol::get_well_known_symbol_no_ctx("iterator")
    else {
        return;
    };
    let key = sym.property_key();
    let values = array_proto.borrow().get_own_value("values");
    if let Some(values) = values {
        let mut prototype = array_proto.borrow_mut();
        prototype.set_symbol(&key, values);
        if let Some(flags) = prototype.descriptors.get_mut(&key) {
            flags.enumerable = false;
        }
    }
}
