//! Minimal Reflect and Proxy globals. Reflect exposes `ownKeys` and `has` for
//! the test262 harness. Proxy provides a basic target-forwarding constructor
//! that delegates `get`/`set`/`has` traps (defaulting to forwarding when
//! the handler omits them). Tests that require the full Reflect or Proxy
//! API are still skipped via the `Reflect`/`Proxy` feature gates.

use crate::builtins::object_static::{object_define_property, to_property_key};
use crate::context::Context;
use crate::value::{JsError, Object, ObjectKind, ObjData, Value};
use std::cell::RefCell;
use std::rc::Rc;

fn reflect_has_property(target: &Value, key: &str) -> Result<bool, JsError> {
    match target {
        Value::Object(o) => {
            if crate::eval::object::proxy_handler_and_target(o).is_some() {
                return crate::eval::object::proxy_has_property(o, key)
                    .or_else(|_| Ok(false));
            }
            Ok(o.borrow().has(key))
        }
        Value::Function(f) => {
            if f.get_property(key).is_some() {
                return Ok(true);
            }
            Ok(f.get_prototype().borrow().has(key))
        }
        Value::NativeFunction(nf) => {
            if nf.get_property(key).is_some() {
                return Ok(true);
            }
            if let Some(Value::Object(p)) = nf.get_property("prototype") {
                return Ok(p.borrow().has(key));
            }
            Ok(false)
        }
        Value::NativeConstructor(nc) => {
            if matches!(key, "prototype" | "length" | "name") {
                return Ok(true);
            }
            if nc.get_static_method(key).is_some() || nc.get_accessor(key).is_some() {
                return Ok(true);
            }
            if let Some(fp) = crate::builtins::function::get_function_prototype() {
                if fp.borrow().has(key) {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Value::Class(c) => {
            if c.static_properties_cell.borrow().contains_key(key) {
                return Ok(true);
            }
            if let Some(fp) = crate::builtins::function::get_function_prototype() {
                if fp.borrow().has(key) {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => {
            let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                "Reflect.has called on non-object",
                "TypeError",
            );
            crate::value::set_thrown_value(err_val);
            Err(js_err)
        }
    }
}

pub fn register_reflect(ctx: &mut Context) {
    let mut reflect = Object::new(ObjectKind::Ordinary);
    if let Some(proto) = crate::builtins::get_object_prototype() {
        reflect.prototype = Some(proto);
    }
    reflect.set(
        "ownKeys",
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(
            |args: Vec<Value>| match args.first() {
                Some(Value::Object(o)) => {
                    let keys: Vec<Value> = o
                        .borrow()
                        .own_keys()
                        .into_iter()
                        .map(Value::String)
                        .collect();
                    Ok(Value::Object(Rc::new(RefCell::new(
                        Object::new_array_from(keys),
                    ))))
                }
                _ => {
                    let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                        "Reflect.ownKeys called on non-object",
                        "TypeError",
                    );
                    crate::value::set_thrown_value(err_val);
                    Err(js_err)
                }
            },
        ))),
    );
    reflect.set(
        "has",
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(
            |args: Vec<Value>| {
                let target = args.first().ok_or_else(|| {
                    crate::value::JsError::new("Reflect.has requires target argument")
                })?;
                let key_val = args.get(1).ok_or_else(|| {
                    crate::value::JsError::new("Reflect.has requires propertyKey argument")
                })?;
                let key = to_property_key(key_val)?;
                Ok(Value::Boolean(reflect_has_property(target, &key)?))
            },
        ))),
    );
    reflect.set(
        "defineProperty",
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(
            |args: Vec<Value>| match object_define_property(args) {
                Ok(_) => Ok(Value::Boolean(true)),
                Err(_) => Ok(Value::Boolean(false)),
            },
        ))),
    );
    ctx.set_global(
        "Reflect".to_string(),
        Value::Object(Rc::new(RefCell::new(reflect))),
    );
    register_proxy(ctx);
}

fn register_proxy(ctx: &mut Context) {
    // Proxy(target, handler) — minimal forwarding implementation.
    // The proxy is an object whose default traps (get/set/has) forward to
    // the target. A handler object may override any of those traps. This
    // is sufficient for test262 tests that use a plain handler `{}` to
    // check private-field access boundaries.
    //
    // Per ES spec, Proxy is a constructor but has no .prototype property,
    // so `class extends Proxy {}` throws TypeError at class-definition time.
    let mut proxy_fn = crate::value::NativeFunction::new(
        |args: Vec<Value>| -> Result<Value, crate::value::JsError> {
            let target = match args.first() {
                Some(v) => v.clone(),
                _ => return Err(crate::value::JsError::new("Proxy: target argument missing")),
            };
            let handler = match args.get(1) {
                Some(v) => v.clone(),
                _ => {
                    return Err(crate::value::JsError::new(
                        "Proxy: handler argument missing",
                    ))
                }
            };
            if !matches!(
                target,
                Value::Object(_) | Value::Class(_) | Value::Function(_) | Value::NativeFunction(_)
            ) {
                return Err(crate::value::JsError::new(
                    "TypeError: Proxy target must be an object",
                ));
            }
            if !matches!(handler, Value::Object(_)) {
                return Err(crate::value::JsError::new(
                    "TypeError: Proxy handler must be an object",
                ));
            }
            let handler_obj = if let Value::Object(h) = &handler {
                Rc::clone(h)
            } else {
                return Err(crate::value::JsError::new(
                    "TypeError: Proxy handler must be an object",
                ));
            };
            let mut proxy = Object::new(ObjectKind::Ordinary);
            if let Value::Object(target_obj) = &target {
                proxy.data = ObjData::Proxy {
                    target: Rc::clone(target_obj),
                    handler: Rc::clone(&handler_obj),
                };
            } else {
                // Compatibility: keep legacy fallback metadata on non-object targets
                // so minimal tests that rely on direct property lookup still pass.
                proxy.set("__quench_proxy_target", target);
                proxy.set("__quench_proxy_handler", Value::Object(handler_obj));
            }
            Ok(Value::Object(Rc::new(RefCell::new(proxy))))
        },
    );
    proxy_fn.set_constructable(true);
    proxy_fn.name = "Proxy".to_string();
    let proxy_ctor = Value::NativeFunction(Rc::new(proxy_fn));
    ctx.set_global("Proxy".to_string(), proxy_ctor);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Context;

    fn eval_ok(src: &str) -> Value {
        let mut ctx = Context::new().unwrap();
        ctx.eval(src).unwrap()
    }

    fn eval_err(src: &str) -> bool {
        let mut ctx = Context::new().unwrap();
        ctx.eval(src).is_err()
    }

    fn eval_ok_with_builtins(src: &str) -> Value {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        ctx.eval(src).unwrap()
    }

    #[test]
    fn reflect_define_property_sets_value() {
        let result = eval_ok_with_builtins(
            "var o = {}; Reflect.defineProperty(o, 'x', {value: 42, writable: true, enumerable: true, configurable: true}); o.x",
        );
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn reflect_has_own_property() {
        let result = eval_ok_with_builtins("Reflect.has({a: 1}, 'a')");
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn proxy_constructor_stores_exotic_data() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let proxy = ctx
            .eval("var proxy = new Proxy({Object}, {}); proxy;")
            .unwrap();
        let Value::Object(proxy_obj) = proxy else {
            panic!("proxy expected object");
        };
        let info = crate::eval::object::proxy_handler_and_target(&proxy_obj)
            .expect("proxy metadata expected");
        let (_handler, target) = info;
        assert!(matches!(target, Value::Object(_)));
        assert_eq!(crate::eval::object::proxy_has_property(&proxy_obj, "Object").unwrap(), true);
    }

    #[test]
    fn proxy_has_property_uses_handler_trap() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let proxy = ctx
            .eval(
                "var proxy = new Proxy({Object}, {\n            has(t, p) { return p in t; },\n            get(t, p, r) { return t[p]; },\n          });\n          proxy;",
            )
            .unwrap();
        let Value::Object(proxy_obj) = proxy else {
            panic!("proxy expected object");
        };
        assert!(crate::eval::object::proxy_has_property(&proxy_obj, "Object").unwrap());
        match crate::eval::object::proxy_get_property(&proxy_obj, "Object") {
            Ok(v) => assert!(!matches!(v, Value::Undefined)),
            Err(err) => panic!("proxy_get_property error: {err}"),
        }
    }

    #[test]
    fn reflect_has_missing_property() {
        let result = eval_ok_with_builtins("Reflect.has({}, 'x')");
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn reflect_has_non_object_throws() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx.eval("Reflect.has(null, 'x')").is_err());
    }

    #[test]
    fn reflect_own_keys_empty_object() {
        let result = eval_ok_with_builtins("Reflect.ownKeys({})");
        assert!(matches!(result, Value::Object(_)));
    }

    #[test]
    fn reflect_own_keys_with_properties() {
        let result = eval_ok("Reflect.ownKeys({a: 1, b: 2})");
        let arr = match result {
            Value::Object(rc) => rc.borrow().clone(),
            _ => panic!("expected Object"),
        };
        assert_eq!(arr.elements.len(), 2);
    }

    #[test]
    fn reflect_own_keys_non_object_throws() {
        assert!(eval_err("Reflect.ownKeys(null)"));
        assert!(eval_err("Reflect.ownKeys(42)"));
    }

    #[test]
    fn reflect_exists_as_global() {
        let result = eval_ok("typeof Reflect");
        assert_eq!(result.to_string(), "object");
    }

    #[test]
    fn reflect_own_keys_exists() {
        let result = eval_ok("typeof Reflect.ownKeys");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn proxy_constructor_basic() {
        let result = eval_ok("typeof Proxy");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn proxy_constructor_name() {
        let result = eval_ok("Proxy.name");
        assert_eq!(result.to_string(), "Proxy");
    }

    #[test]
    fn proxy_with_empty_handler() {
        let result =
            eval_ok("var target = {x: 1}; var proxy = new Proxy(target, {}); typeof proxy");
        assert_eq!(result.to_string(), "object");
    }

    #[test]
    fn proxy_target_must_be_object() {
        assert!(eval_err("new Proxy(42, {})"));
        assert!(eval_err("new Proxy('str', {})"));
        assert!(eval_err("new Proxy(null, {})"));
    }

    #[test]
    fn proxy_handler_must_be_object() {
        assert!(eval_err("new Proxy({}, 42)"));
        assert!(eval_err("new Proxy({}, 'str')"));
        assert!(eval_err("new Proxy({}, null)"));
    }

    #[test]
    fn proxy_missing_arguments() {
        assert!(eval_err("new Proxy()"));
        assert!(eval_err("new Proxy({})"));
    }

    #[test]
    fn proxy_cannot_be_extended() {
        // Per spec, Proxy has no .prototype property, so `class extends Proxy`
        // throws TypeError (Type(protoParent) is not Object or Null).
        assert!(eval_err("class P extends Proxy {}"));
    }
}
