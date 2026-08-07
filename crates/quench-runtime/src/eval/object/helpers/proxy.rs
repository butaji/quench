//! Proxy helper functions.

use crate::value::{JsError, Object, ObjectKind, PropertyFlags, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// If `obj` is a Proxy exotic object, return `(handler, target)`.
pub fn proxy_handler_and_target(
    obj: &Rc<RefCell<crate::value::Object>>,
) -> Option<(Rc<RefCell<crate::value::Object>>, Value)> {
    let borrowed = obj.borrow();
    let handler = borrowed.properties.get("__quench_proxy_handler")?;
    let target = borrowed.properties.get("__quench_proxy_target")?.clone();
    match handler {
        Value::Object(h) => Some((Rc::clone(h), target)),
        _ => None,
    }
}

/// Object that owns private fields/methods — the proxy target when `obj` is a Proxy.
pub fn private_field_object(
    obj: &Rc<RefCell<crate::value::Object>>,
) -> Rc<RefCell<crate::value::Object>> {
    if let Some(Value::Object(target)) = obj.borrow().properties.get("__quench_proxy_target") {
        return Rc::clone(target);
    }
    Rc::clone(obj)
}

/// Find a proxy in the prototype chain, returning (proxy_rc, handler, target).
#[allow(clippy::type_complexity)]
pub fn find_proxy_in_prototype_chain(
    obj: &Rc<RefCell<crate::value::Object>>,
    _prop_name: &str,
) -> Option<(
    Rc<RefCell<crate::value::Object>>,
    Rc<RefCell<crate::value::Object>>,
    Value,
)> {
    let mut current = obj
        .borrow()
        .prototype
        .as_ref()
        .and_then(|p| p.borrow().prototype.clone());
    loop {
        let proto_rc = current?;
        let proto = proto_rc.borrow();
        if let Some(Value::Object(handler)) = proto.properties.get("__quench_proxy_handler") {
            if let Some(target) = proto.properties.get("__quench_proxy_target") {
                return Some((proto_rc.clone(), handler.clone(), target.clone()));
            }
        }
        current = proto.prototype.clone();
    }
}

/// Invoke the proxy set trap, returning whether the assignment succeeded.
pub fn call_proxy_set_trap(
    target: &Value,
    handler: &Rc<RefCell<crate::value::Object>>,
    this_val: &Value,
    prop_name: &str,
    value: Value,
) -> Result<bool, JsError> {
    let trap_opt: Option<Value> = handler
        .borrow()
        .get_own_value("set")
        .or_else(|| handler.borrow().get_setter_func("set"));
    let Some(trap) = trap_opt else {
        return Ok(true);
    };
    let trap = match trap {
        Value::Object(f) => Value::Object(f),
        Value::Function(f) => Value::Function(f),
        Value::NativeFunction(nf) => Value::NativeFunction(nf),
        Value::NativeConstructor(nc) => Value::NativeConstructor(nc),
        Value::Undefined => return Ok(true),
        _ => {
            let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                "set trap is not callable",
                "TypeError",
            );
            crate::value::set_thrown_value(err_val);
            return Err(js_err);
        }
    };
    let trap_result = crate::eval::function::call_value_with_this(
        trap,
        vec![target.clone(), Value::String(prop_name.to_string()), value],
        this_val.clone(),
    );
    match trap_result {
        Ok(result) => Ok(crate::value::to_bool(&result)),
        Err(e) => Err(e),
    }
}

fn data_descriptor(value: Value) -> Rc<RefCell<Object>> {
    let mut desc = Object::new(ObjectKind::Ordinary);
    desc.set("value", value);
    desc.set("writable", Value::Boolean(true));
    desc.set("enumerable", Value::Boolean(true));
    desc.set("configurable", Value::Boolean(true));
    Rc::new(RefCell::new(desc))
}

/// ES CreateDataPropertyOrThrow — routes public field init through proxy defineProperty.
pub fn create_data_property_or_throw(
    obj: &Rc<RefCell<Object>>,
    key: &str,
    value: Value,
) -> Result<(), JsError> {
    if !obj.borrow().extensible && !obj.borrow().properties.contains_key(key) {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "Cannot add property to non-extensible object",
            "TypeError",
        );
        return Err(js_err);
    }

    if let Some((handler, target)) = proxy_handler_and_target(obj) {
        let trap_opt = handler
            .borrow()
            .get_own_value("defineProperty")
            .or_else(|| handler.borrow().get_setter_func("defineProperty"));
        let Some(trap) = trap_opt else {
            if let Value::Object(target_obj) = target {
                let flags = PropertyFlags {
                    value: Some(value.clone()),
                    writable: true,
                    enumerable: true,
                    configurable: true,
                };
                target_obj.borrow_mut().define(key, value, flags);
            }
            return Ok(());
        };
        let trap = match trap {
            Value::Object(f) => Value::Object(f),
            Value::Function(f) => Value::Function(f),
            Value::NativeFunction(nf) => Value::NativeFunction(nf),
            Value::NativeConstructor(nc) => Value::NativeConstructor(nc),
            Value::Undefined => {
                if let Value::Object(target_obj) = target {
                    let flags = PropertyFlags {
                        value: Some(value.clone()),
                        writable: true,
                        enumerable: true,
                        configurable: true,
                    };
                    target_obj.borrow_mut().define(key, value, flags);
                }
                return Ok(());
            }
            _ => {
                let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                    "defineProperty trap is not callable",
                    "TypeError",
                );
                crate::value::set_thrown_value(err_val);
                return Err(js_err);
            }
        };
        let this_val = Value::Object(Rc::clone(obj));
        let trap_result = crate::eval::function::call_value_with_this(
            trap,
            vec![
                target,
                Value::String(key.to_string()),
                Value::Object(data_descriptor(value)),
            ],
            this_val,
        );
        match trap_result {
            Ok(result) if crate::value::to_bool(&result) => Ok(()),
            Ok(_) => {
                let (_, js_err) = crate::value::error::create_js_error_with_type(
                    "Cannot define property",
                    "TypeError",
                );
                Err(js_err)
            }
            Err(e) => Err(e),
        }
    } else {
        let flags = PropertyFlags {
            value: Some(value.clone()),
            writable: true,
            enumerable: true,
            configurable: true,
        };
        if key.contains('\0') {
            obj.borrow_mut().set_symbol(key, value);
        } else {
            obj.borrow_mut().define(key, value, flags);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::Context;
    use crate::Value;

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── Proxy: basic get trap ──────────────────────────────────────────────

    #[test]
    fn proxy_basic_get() {
        let r = eval(
            "var target = {x: 1}; var handler = {get(o, k) { return o[k] * 2; }}; var p = new Proxy(target, handler); p.x",
        );
        assert!(r.is_ok());
    }

    // ─── Proxy: basic set trap ──────────────────────────────────────────────

    #[test]
    fn proxy_basic_set() {
        let r = eval(
            "var target = {}; var handler = {set(o, k, v) { o[k] = v + 1; return true; }}; var p = new Proxy(target, handler); p.a = 5; target.a",
        );
        assert!(r.is_ok());
    }

    #[test]
    fn public_class_field_define_property_observable_by_proxy() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let r = ctx.eval(
            "let arr = []; \
             function ProxyBase() { \
               return new Proxy(this, { \
                 defineProperty(target, key, descriptor) { \
                   arr.push(key); arr.push(descriptor.value); \
                   return Reflect.defineProperty(target, key, descriptor); \
                 } \
               }); \
             } \
             class Test extends ProxyBase { f = 3; g = 'Test262'; } \
             let t = new Test(); \
             t.f === 3 && t.g === 'Test262' && arr.length === 4 && arr[0] === 'f' && arr[1] === 3",
        );
        assert_eq!(r.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn public_class_field_proxy_define_property_throws() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let err = ctx.eval(
            "function ProxyBase() { \
               return new Proxy(this, { defineProperty() { throw new Error('proxy'); } }); \
             } \
             class Base extends ProxyBase { f = 'Test262'; } \
             new Base();",
        );
        assert!(err.is_err());
    }
}
