//! Minimal Reflect and Proxy globals. Reflect exposes `ownKeys` and `has` for
//! the test262 harness. Proxy provides a basic target-forwarding constructor
//! that delegates `get`/`set`/`has` traps (defaulting to forwarding when
//! the handler omits them). Tests that require the full Reflect or Proxy
//! API are still skipped via the `Reflect`/`Proxy` feature gates.

use crate::builtins::object_static::{object_define_property, to_property_key};
use crate::context::Context;
use crate::value::{to_object, JsError, ObjData, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

fn reflect_type_error(message: &str) -> JsError {
    let (value, error) = crate::value::error::create_js_error_with_type(message, "TypeError");
    crate::value::set_thrown_value(value);
    error
}

fn reflect_construct(args: Vec<Value>) -> Result<Value, JsError> {
    let target = args
        .first()
        .cloned()
        .ok_or_else(|| JsError::new("Reflect.construct requires target"))?;
    let list = args
        .get(1)
        .and_then(|value| match value {
            Value::Object(object) => Some(object.borrow().elements.clone()),
            _ => None,
        })
        .ok_or_else(|| JsError::new("Reflect.construct requires an argument list"))?;
    if !crate::eval::class::helpers::is_constructor_value(&target) {
        return Err(reflect_type_error("target is not a constructor"));
    }
    let new_target = args.get(2).cloned().unwrap_or_else(|| target.clone());
    if !crate::eval::class::helpers::is_constructor_value(&new_target) {
        return Err(reflect_type_error("newTarget is not a constructor"));
    }
    let realm_default = match &new_target {
        Value::Function(function) => {
            let intrinsic = match &target {
                Value::NativeConstructor(constructor) => constructor.name(),
                _ => "Object".to_string(),
            };
            function.closure.borrow().get(&intrinsic).and_then(|object| {
                crate::eval::class::get_constructor_prototype(&object)
                    .ok()
                    .flatten()
            })
        }
        _ => None,
    };
    let new_target_prototype = match &new_target {
        Value::Function(function) => match function.get_property("prototype") {
            Some(Value::Object(prototype)) => Some(prototype),
            _ => None,
        },
        _ => crate::eval::class::get_constructor_prototype(&new_target)?,
    };
    let mut prototype = new_target_prototype
        .or(realm_default)
        .or_else(|| {
            crate::eval::class::get_constructor_prototype(&target)
                .ok()
                .flatten()
        })
        .or_else(crate::builtins::get_object_prototype)
        .ok_or_else(|| JsError::new("Reflect.construct has no default prototype"))?;
    if let (Value::NativeFunction(target_function), Value::Function(new_target_function)) =
        (&target, &new_target)
    {
        if target_function.name == "Boolean"
            && matches!(
                new_target_function.get_property("prototype"),
                Some(Value::Null)
            )
        {
            if let Some(Value::NativeFunction(boolean)) =
                new_target_function.closure.borrow().get("Boolean")
            {
                prototype =
                    crate::eval::class::get_constructor_prototype(&Value::NativeFunction(boolean))?
                        .or_else(crate::builtins::get_object_prototype)
                        .ok_or_else(|| JsError::new("Reflect.construct has no realm prototype"))?;
            }
        }
    }
    let this = Value::Object(Rc::new(RefCell::new(Object::with_prototype(
        ObjectKind::Ordinary,
        prototype,
    ))));
    let previous = crate::interpreter::get_new_target();
    crate::interpreter::set_new_target(Some(new_target));
    let result = match &target {
        Value::Class(class) => {
            let env = crate::context::get_current_env()
                .unwrap_or_else(|| Rc::new(RefCell::new(crate::env::Environment::new())));
            crate::eval::class::call_super_constructor(
                class.as_ref().clone(),
                list,
                this.clone(),
                &env,
            )
        }
        _ => crate::eval::function::call_value_with_this(target, list, this.clone()),
    };
    crate::interpreter::set_new_target(previous);
    match result? {
        value @ (Value::Object(_) | Value::Function(_) | Value::NativeFunction(_)) => Ok(value),
        _ => Ok(this),
    }
}

pub fn register_reflect(ctx: &mut Context) {
    let mut reflect = Object::new(ObjectKind::Ordinary);
    if let Some(proto) = crate::builtins::get_object_prototype() {
        reflect.prototype = Some(proto);
    }
    reflect.set(
        "get",
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(
            |args: Vec<Value>| {
                let target = args
                    .first()
                    .ok_or_else(|| JsError::from("Reflect.get requires target argument"))?;
                let property_key = args
                    .get(1)
                    .ok_or_else(|| JsError::from("Reflect.get requires propertyKey argument"))?;
                to_property_key(property_key)?;
                let target = to_object(target)?;
                Ok(match target {
                    Value::Object(obj) => {
                        crate::eval::member::eval_object_member_value(&obj, property_key, None)?
                    }
                    _ => {
                        let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                            "Reflect.get target must be an object",
                            "TypeError",
                        );
                        crate::value::set_thrown_value(err_val);
                        return Err(js_err);
                    }
                })
            },
        ))),
    );
    reflect.set(
        "set",
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(
            |args: Vec<Value>| {
                let target = args
                    .first()
                    .ok_or_else(|| JsError::new("Reflect.set requires target argument"))?;
                let key =
                    to_property_key(args.get(1).ok_or_else(|| {
                        JsError::new("Reflect.set requires propertyKey argument")
                    })?)?;
                let value = args.get(2).cloned().unwrap_or(Value::Undefined);
                let receiver = args.get(3).unwrap_or(target).clone();
                let Value::Object(target_obj) = target else {
                    return Err(JsError::new("Reflect.set target must be an object"));
                };
                if let Some((handler, proxy_target)) =
                    crate::eval::object::proxy_handler_and_target(target_obj)
                {
                    return Ok(Value::Boolean(crate::eval::object::call_proxy_set_trap(
                        &proxy_target,
                        &handler,
                        &receiver,
                        &key,
                        value,
                    )?));
                }
                if let Value::Object(receiver_obj) = &receiver {
                    if crate::eval::object::proxy_handler_and_target(receiver_obj).is_some() {
                        crate::eval::object::call_proxy_get_own_property_descriptor(
                            receiver_obj,
                            &key,
                        )?;
                        return Ok(Value::Boolean(
                            crate::eval::object::call_proxy_define_property(
                                receiver_obj,
                                &key,
                                value,
                            )?,
                        ));
                    }
                }
                if target_obj.borrow().kind == ObjectKind::ModuleNamespace {
                    return Ok(Value::Boolean(false));
                }
                if target_obj
                    .borrow()
                    .descriptors
                    .get(&key)
                    .is_some_and(|flags| !flags.writable)
                {
                    return Ok(Value::Boolean(false));
                }
                target_obj.borrow_mut().set(&key, value);
                Ok(Value::Boolean(true))
            },
        ))),
    );
    reflect.set(
        "deleteProperty",
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(
            |args: Vec<Value>| {
                let target = args
                    .first()
                    .ok_or_else(|| JsError::new("Reflect.deleteProperty requires target"))?;
                let key =
                    to_property_key(args.get(1).ok_or_else(|| {
                        JsError::new("Reflect.deleteProperty requires propertyKey")
                    })?)?;
                let Value::Object(object) = target else {
                    return Err(JsError::new(
                        "Reflect.deleteProperty target must be an object",
                    ));
                };
                if object.borrow().kind == ObjectKind::ModuleNamespace {
                    return Ok(Value::Boolean(!object.borrow().has_own(&key)));
                }
                Ok(Value::Boolean(object.borrow_mut().delete(&key)))
            },
        ))),
    );
    reflect.set(
        "construct",
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(
            reflect_construct,
        ))),
    );
    reflect.set(
        "apply",
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(
            |args: Vec<Value>| {
                let target = args
                    .first()
                    .cloned()
                    .ok_or_else(|| JsError::new("Reflect.apply requires target"))?;
                let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
                let call_args =
                    crate::builtins::function::extract_args_from_array_like(args.get(2))?;
                let callable = matches!(
                    &target,
                    Value::Function(_)
                        | Value::NativeFunction(_)
                        | Value::NativeConstructor(_)
                        | Value::Class(_)
                ) || matches!(&target, Value::Object(object) if object.borrow().is_callable());
                if !callable {
                    return Err(reflect_type_error("Reflect.apply target is not callable"));
                }
                crate::interpreter::set_this_value(this_arg.clone());
                let result = crate::eval::call_value_with_this(target, call_args, this_arg);
                crate::interpreter::take_this_value();
                result
            },
        ))),
    );
    reflect.set(
        "defineProperty",
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(
            |args: Vec<Value>| match object_define_property(args) {
                Ok(_) => Ok(Value::Boolean(true)),
                Err(_) => {
                    crate::value::take_thrown_value();
                    Ok(Value::Boolean(false))
                }
            },
        ))),
    );
    ctx.set_global(
        "Reflect".to_string(),
        Value::Object(Rc::new(RefCell::new(reflect))),
    );
    register_proxy(ctx);
}

fn create_proxy(args: Vec<Value>) -> Result<Value, crate::value::JsError> {
    let target = args
        .first()
        .cloned()
        .ok_or_else(|| crate::value::JsError::new("Proxy: target argument missing"))?;
    let handler = args
        .get(1)
        .cloned()
        .ok_or_else(|| crate::value::JsError::new("Proxy: handler argument missing"))?;
    let Value::Object(handler) = handler else {
        return Err(crate::value::JsError::new(
            "TypeError: Proxy handler must be an object",
        ));
    };
    if !matches!(
        target,
        Value::Object(_) | Value::Class(_) | Value::Function(_) | Value::NativeFunction(_)
    ) {
        return Err(crate::value::JsError::new(
            "TypeError: Proxy target must be an object",
        ));
    }
    let mut proxy = Object::new(ObjectKind::Ordinary);
    proxy.callable = target.is_callable();
    proxy.call_slot = proxy.callable.then(|| target.clone());
    if let Value::Object(target) = &target {
        proxy.data = ObjData::Proxy {
            target: Rc::clone(target),
            handler,
        };
    } else {
        proxy.set("__quench_proxy_target", target);
        proxy.set("__quench_proxy_handler", Value::Object(handler));
    }
    Ok(Value::Object(Rc::new(RefCell::new(proxy))))
}

fn proxy_revocable(args: Vec<Value>) -> Result<Value, crate::value::JsError> {
    let proxy = create_proxy(args)?;
    let Value::Object(proxy_object) = &proxy else {
        unreachable!()
    };
    let revoked = Rc::clone(proxy_object);
    let revoke = Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(move |_| {
        revoked
            .borrow_mut()
            .set("__quench_proxy_revoked", Value::Boolean(true));
        Ok(Value::Undefined)
    })));
    let mut record = Object::new(ObjectKind::Ordinary);
    record.set("proxy", proxy);
    record.set("revoke", revoke);
    Ok(Value::Object(Rc::new(RefCell::new(record))))
}

fn register_proxy(ctx: &mut Context) {
    let mut proxy_fn = crate::value::NativeFunction::new(create_proxy);
    proxy_fn.set_constructable(true);
    proxy_fn.name = "Proxy".to_string();
    let _ = proxy_fn.set_property(
        "revocable",
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(proxy_revocable))),
    );
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
    fn reflect_get_uses_get_trap_for_proxy() {
        let result = eval_ok_with_builtins(
            r#"
var log = [];
var proxy = new Proxy({x: 1, [Symbol.unscopables]: {}}, {
  get(t, pk) {
    log.push(String(pk));
    return t[pk];
  },
});
var x = Reflect.get(proxy, 'x');
if (x !== 1) { throw new Error('expected 1'); }
if (log[0] !== 'x') { throw new Error('expected x'); }
if (Reflect.get(proxy, Symbol.unscopables) !== proxy[Symbol.unscopables]) { throw new Error('unscopables mismatch'); }
"#,
        );
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn reflect_set_proxy_trap_can_forward_to_reflect() {
        let result = eval_ok_with_builtins(
            r#"
var log = [];
var target = {p: 0};
var proxy = new Proxy(target, {
  set(t, pk, v, r) {
    log.push("set:" + String(pk));
    return Reflect.set(t, pk, v, r);
  },
});
with (proxy) { p = 1; }
JSON.stringify([target.p, log]);
"#,
        );
        assert_eq!(result, Value::String("[1,[\"set:p\"]]".to_string()));
    }

    #[test]
    fn reflect_set_uses_undefined_when_value_is_omitted() {
        let result = eval_ok_with_builtins(
            "var o = {}; Reflect.set(o, 'x'); JSON.stringify([o.x, Reflect.set(o, 'x')]);",
        );
        assert_eq!(result, Value::String("[null,true]".to_string()));
    }

    #[test]
    fn reflect_get_own_property_descriptor_returns_fields() {
        let result = eval_ok_with_builtins(
            "var o = {}; Object.defineProperty(o, 'x', {value: 42, writable: false, enumerable: true, configurable: false}); var d = Reflect.getOwnPropertyDescriptor(o, 'x'); JSON.stringify([d.value, d.writable, d.enumerable, d.configurable]);",
        );
        assert_eq!(result, Value::String("[42,false,true,false]".to_string()));
    }

    #[test]
    fn with_var_declaration_keeps_identifier_binding_after_scope_exit() {
        let result = eval_ok_with_builtins(
            "var object = {value: 'object'}; with (object) { var value = 'value'; } object.value",
        );
        assert_eq!(result, Value::String("value".to_string()));
    }

    #[test]
    fn with_primitive_assignment_falls_through_to_outer_var() {
        let result = eval_ok_with_builtins("var foo = 1; with (2) { foo = 42; } foo");
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn with_compound_assignment_updates_visible_object_binding() {
        let result = eval_ok_with_builtins(
            "var object = {x: 4, [Symbol.unscopables]: {}}; with (object) { x++; } object.x",
        );
        assert_eq!(result, Value::Number(5.0));
    }

    #[test]
    fn reflect_has_own_property() {
        let result = eval_ok_with_builtins("Reflect.has({a: 1}, 'a')");
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn reflect_prevent_extensions_returns_true() {
        let result = eval_ok_with_builtins(
            "var o = {}; Reflect.preventExtensions(o) && !Reflect.isExtensible(o)",
        );
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
        assert!(crate::eval::object::proxy_has_property(&proxy_obj, "Object").unwrap());
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
    fn reflect_define_property_converts_nonextensible_error_to_false() {
        assert_eq!(
            eval_ok("Reflect.defineProperty(Object.preventExtensions({}), 'x', {})"),
            Value::Boolean(false)
        );
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
    fn reflect_apply_invokes_target_with_undefined_new_target() {
        let result = eval_ok(
            "var newTarget = null; function f() { newTarget = new.target; } Reflect.apply(f, {}, []); typeof newTarget",
        );
        assert_eq!(result.to_string(), "undefined");
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
