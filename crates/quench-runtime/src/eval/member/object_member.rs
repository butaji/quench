//! Object member access evaluation

use crate::context::CURRENT_CONTEXT;
use crate::env::Environment;
use crate::eval::object::call_getter;
use crate::eval::object::{private_field_object, proxy_handler_and_target};
use crate::value::object::as_array_index;
use crate::value::object::ObjData;
use crate::value::{create_js_error_with_type, JsError, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

fn handler_get_trap(handler: &Rc<RefCell<Object>>) -> Option<Value> {
    let h = handler.borrow();
    h.get_own_value("get")
        .or_else(|| h.get_getter("get").and_then(|g| g.func.clone()))
}

fn proxy_get_property(
    proxy: &Rc<RefCell<Object>>,
    handler: &Rc<RefCell<Object>>,
    target: &Value,
    prop_name: &Value,
    env: Option<&Rc<RefCell<Environment>>>,
) -> Result<Value, JsError> {
    let trap_val = handler_get_trap(handler);
    let Some(trap_val) = trap_val else {
        let Value::Object(target_obj) = target else {
            return Ok(Value::Undefined);
        };
        return eval_object_member_inner_value(target_obj, prop_name, env);
    };
    let trap = match trap_val {
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_) => trap_val,
        Value::Undefined => {
            let Value::Object(target_obj) = target else {
                return Ok(Value::Undefined);
            };
            return eval_object_member_inner_value(target_obj, prop_name, env);
        }
        _ => {
            let (_, js_err) = create_js_error_with_type("get trap is not callable", "TypeError");
            return Err(js_err);
        }
    };
    crate::eval::function::call_value_with_this(
        trap,
        vec![
            target.clone(),
            prop_name.clone(),
            Value::Object(Rc::clone(proxy)),
        ],
        Value::Object(Rc::clone(handler)),
    )
    .map(|v| {
        let _ = crate::interpreter::take_control_flow();
        v
    })
}

/// Evaluate member access on an object. If `env` is provided, global object
/// lookups fall back to the environment's globalThis binding for properties
/// stored as built-in globals (e.g. `isFinite`, `parseInt`, etc.).
pub fn eval_object_member(
    o: &Rc<RefCell<Object>>,
    prop_name: &str,
    env: Option<&Rc<RefCell<Environment>>>,
) -> Result<Value, JsError> {
    if crate::builtins::function::is_function_prototype(o)
        && (prop_name == "arguments" || prop_name == "caller")
    {
        return Err(JsError(
            crate::builtins::function::get_restricted_prop_error(),
        ));
    }
    if crate::value::is_private_name_key(prop_name) {
        return eval_private_name_get(&private_field_object(o), prop_name, env);
    }
    if let Some((handler, target)) = proxy_handler_and_target(o) {
        let symbol_key = Value::String(prop_name.to_string());
        return proxy_get_property(o, &handler, &target, &symbol_key, env);
    }
    eval_object_member_inner(o, prop_name, env)
}

pub fn eval_object_member_value(
    o: &Rc<RefCell<Object>>,
    prop_name: &Value,
    env: Option<&Rc<RefCell<Environment>>>,
) -> Result<Value, JsError> {
    if let Value::String(s) = prop_name {
        if crate::value::is_private_name_key(s) {
            return eval_private_name_get(&private_field_object(o), s, env);
        }
    }
    if let Some((handler, target)) = proxy_handler_and_target(o) {
        return proxy_get_property(o, &handler, &target, prop_name, env);
    }
    eval_object_member_inner_value(o, prop_name, env)
}

fn eval_object_member_inner(
    o: &Rc<RefCell<Object>>,
    prop_name: &str,
    env: Option<&Rc<RefCell<Environment>>>,
) -> Result<Value, JsError> {
    {
        let mut current: Option<Rc<RefCell<Object>>> = Some(Rc::clone(o));
        while let Some(obj_rc) = current {
            {
                let obj = obj_rc.borrow();
                if let Some(getter_storage) = obj.get_getter(prop_name) {
                    let getter_clone = getter_storage.clone();
                    drop(obj);
                    return call_getter(
                        o,
                        &getter_clone,
                        &Rc::new(RefCell::new(Environment::new())),
                    );
                }
                if obj.get_setter(prop_name).is_some() {
                    return Ok(Value::Undefined);
                }
                if let Some(idx) = as_array_index(prop_name) {
                    if let (ObjData::Args { mapped }, Some(eval_env)) = (&obj.data, env) {
                        if !obj.holes.contains(&idx) {
                            if let Some(name) = mapped.get(&(idx as u32)) {
                                return Ok(eval_env.borrow().get(name).unwrap_or(Value::Undefined));
                            }
                        }
                    }
                }
                // For TypedArrays (ObjData::Idx): always check get_own first for length/byteLength
                // and indexed elements — get_own computes these dynamically from the buffer.
                if matches!(obj.data, ObjData::Idx { .. })
                    && (prop_name == "length"
                        || prop_name == "byteLength"
                        || as_array_index(prop_name).is_some())
                {
                    // For element access on an out-of-bounds TA (buffer was shrunk),
                    // the spec requires a TypeError even for indices that would be
                    // within the current buffer if the TA's fixed bounds are exceeded.
                    if as_array_index(prop_name).is_some() && obj.typed_array_is_out_of_bounds() {
                        let (_, js_err) =
                            create_js_error_with_type("TypedArray is out of bounds", "TypeError");
                        return Err(js_err);
                    }
                    if let Some(val) = obj.get_own(prop_name) {
                        return Ok(val);
                    }
                    // get_own returned None: skip properties.get (stale) and continue to prototype chain
                } else if let Some(val) = obj.properties.get(prop_name) {
                    return Ok(val.clone());
                }
                // Check symbol properties (key stored as raw "Symbol():N" format)
                if let Some(val) = obj.symbol_properties.get(prop_name) {
                    return Ok(val.clone());
                }
                if let Some(idx) = as_array_index(prop_name) {
                    if idx < obj.elements.len() && !obj.holes.contains(&idx) {
                        return Ok(obj.elements[idx].clone());
                    }
                }
                current = obj.prototype.as_ref().map(Rc::clone);
            }
        }
    }
    {
        let obj = o.borrow();
        if obj.kind == ObjectKind::Date && prop_name == "prototype" {
            let mut proto = Object::new(ObjectKind::Ordinary);
            proto.set("constructor", Value::Object(Rc::clone(o)));
            return Ok(Value::Object(Rc::new(RefCell::new(proto))));
        }
        // Handle __proto__ as a getter for the internal prototype
        if prop_name == "__proto__" {
            if let Some(ref proto_obj) = obj.prototype {
                return Ok(Value::Object(Rc::clone(proto_obj)));
            }
            return Ok(Value::Undefined);
        }
        // For global object: built-in globals (isFinite, parseInt, etc.) are stored
        // in the environment's bindings. Fall back to globalThis bindings if the
        // property wasn't found on the object itself or its prototype chain.
        if obj.kind == ObjectKind::Global {
            // Try the provided env first, then fall back to CURRENT_CONTEXT thread-local.
            let fallback_env = env.or_else(|| {
                CURRENT_CONTEXT.with(|cell| cell.borrow().map(|ptr| unsafe { &*ptr }.env()))
            });
            if let Some(e) = fallback_env {
                if let Some(Value::Object(global_rc)) = e.borrow().get("globalThis").as_ref() {
                    let global = global_rc.borrow();
                    if let Some(found) = global.properties.get(prop_name) {
                        return Ok(found.clone());
                    }
                }
            }
        }
    }
    Ok(Value::Undefined)
}

fn eval_object_member_inner_value(
    o: &Rc<RefCell<Object>>,
    prop_name: &Value,
    env: Option<&Rc<RefCell<Environment>>>,
) -> Result<Value, JsError> {
    match prop_name {
        Value::Symbol(_) | Value::BigInt(_) | Value::Number(_) | Value::Boolean(_) => {
            let mut current: Option<Rc<RefCell<Object>>> = Some(Rc::clone(o));
            while let Some(obj_rc) = current {
                let obj = obj_rc.borrow();
                let key = crate::value::to_js_string(prop_name);
                if let Some(getter_storage) = obj.get_getter(&key) {
                    let getter_clone = getter_storage.clone();
                    drop(obj);
                    return call_getter(
                        o,
                        &getter_clone,
                        &Rc::new(RefCell::new(Environment::new())),
                    );
                }
                if obj.get_setter(&key).is_some() {
                    return Ok(Value::Undefined);
                }
                if let Value::Symbol(sym) = prop_name {
                    let key = sym.property_key();
                    if let Some(getter_storage) = obj.get_getter(&key) {
                        let getter_clone = getter_storage.clone();
                        drop(obj);
                        return call_getter(
                            o,
                            &getter_clone,
                            &Rc::new(RefCell::new(Environment::new())),
                        );
                    }
                }
                if let Some(val) = obj.get_property(prop_name) {
                    return Ok(val);
                }
                current = obj.prototype.as_ref().map(Rc::clone);
            }
        }
        _ => {
            return eval_object_member_inner(o, &crate::value::to_js_string(prop_name), env);
        }
    }
    let obj = o.borrow();
    if obj.kind == ObjectKind::Date && matches!(prop_name, Value::String(s) if s == "prototype") {
        let mut proto = Object::new(ObjectKind::Ordinary);
        proto.set("constructor", Value::Object(Rc::clone(o)));
        return Ok(Value::Object(Rc::new(RefCell::new(proto))));
    }
    if obj.kind == ObjectKind::Global && env.is_some() {
        let fallback_env = env.or_else(|| {
            CURRENT_CONTEXT.with(|cell| cell.borrow().map(|ptr| unsafe { &*ptr }.env()))
        });
        if let Some(e) = fallback_env {
            if let Some(Value::Object(global_rc)) = e.borrow().get("globalThis").as_ref() {
                let global = global_rc.borrow();
                if let Value::String(s) = prop_name {
                    if let Some(found) = global.get(s) {
                        return Ok(found.clone());
                    }
                }
            }
        }
    }
    Ok(Value::Undefined)
}

fn eval_private_name_get(
    o: &Rc<RefCell<Object>>,
    prop_name: &str,
    env: Option<&Rc<RefCell<Environment>>>,
) -> Result<Value, JsError> {
    let obj = o.borrow();
    if let Some(getter_storage) = obj.get_getter(prop_name) {
        let getter_clone = getter_storage.clone();
        drop(obj);
        return call_getter(o, &getter_clone, &Rc::new(RefCell::new(Environment::new())));
    }
    if let Some(val) = obj.properties.get(prop_name) {
        return Ok(val.clone());
    }
    if let Some(ctor) = env.and_then(|e| e.borrow().get("TypeError")) {
        if let Ok(error) = crate::eval::call_value_with_this(
            ctor,
            vec![Value::String(
                "Cannot read private member from an object whose class did not declare it"
                    .to_string(),
            )],
            Value::Undefined,
        ) {
            crate::value::set_thrown_value(error);
            return Err(JsError::from(
                "TypeError: Cannot read private member from an object whose class did not declare it",
            ));
        }
    }
    let (_, js_err) = create_js_error_with_type(
        "Cannot read private member from an object whose class did not declare it",
        "TypeError",
    );
    Err(js_err)
}

#[cfg(test)]
mod tests {
    use crate::value::Value;
    use crate::Context;

    #[test]
    fn non_canonical_numeric_member_does_not_alias_element() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("var a = [10, 20]; a['01']").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn private_field_brand_check_throws_for_foreign_object() {
        let mut ctx = Context::new().unwrap();
        let err = ctx
            .eval(
                "class C { #m = 44; getWithEval() { return eval(\"this.#m\"); } } \
                 class D { #m = 44; } \
                 C.prototype.getWithEval.call(new D())",
            )
            .unwrap_err();
        assert!(err.0.contains("TypeError"), "got {}", err.0);
    }

    #[test]
    fn private_field_get_on_primitive_receiver_throws_type_error() {
        let mut ctx = Context::new().unwrap();
        let err = ctx
            .eval("class C { #p = 1; m() { this.#p; } } C.prototype.m.call(15);")
            .unwrap_err();
        assert!(err.0.contains("TypeError"), "got {}", err.0);
    }

    #[test]
    fn static_private_setter_only_get_throws_type_error() {
        let mut ctx = Context::new().unwrap();
        let err = ctx
            .eval(
                "class C { static set #f(v) { throw new Test262Error(); } \
                 static getAccess() { return this.#f; } } C.getAccess();",
            )
            .unwrap_err();
        assert!(err.0.contains("TypeError"), "got {}", err.0);
    }

    #[test]
    fn with_proxy_unscopables_lookup_passes_symbol_key() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                "var log = []; \
                 var unscopables = Symbol.unscopables; \
                 var env = new Proxy({ x: 1 }, { \
                   get(_t, p) { \
                     log.push(p); \
                     if (p === unscopables) { return {}; } \
                     return 0; \
                   } \
                 }); \
                 with (env) { x; } \
                 log[0] === unscopables",
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }
}
