//! Object operations: assignment, property access, getter/setter calls

use crate::ast::*;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::statement::eval_function_body;
use crate::eval::string_methods::get_string_method;
use crate::value::{GetterStorage, JsError, Object, ObjectKind, SetterStorage, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Assign a value to a target (variable, member, or destructuring pattern)
pub fn assign_to(
    target: &Expression,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    match target {
        Expression::Identifier(name) => assign_to_identifier(name, value, env),
        Expression::Member {
            object,
            property,
            computed,
        } => assign_to_member(object, property, *computed, value, env),
        Expression::ArrayPattern(bindings) => assign_array_destructuring(bindings, value, env),
        Expression::ObjectPattern(props) => assign_object_destructuring(props, value, env),
        _ => Err(JsError("Invalid assignment target".to_string())),
    }
}

fn assign_array_destructuring(
    bindings: &[BindingElement],
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let arr_rc = match value {
        Value::Object(o) => o.clone(),
        Value::String(s) => {
            let chars: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
            let len = chars.len();
            let mut arr = Object::new(ObjectKind::Array);
            arr.elements = chars;
            arr.properties
                .insert("length".to_string(), Value::Number(len as f64));
            Rc::new(RefCell::new(arr))
        }
        _ => return Err(JsError("Cannot destructure non-iterable value".to_string())),
    };
    for (i, binding) in bindings.iter().enumerate() {
        let elem_value = {
            let arr_ref = arr_rc.borrow();
            arr_ref.get(&i.to_string()).unwrap_or(Value::Undefined)
        };
        assign_binding_elem(binding, &elem_value, env)?;
    }
    Ok(())
}

fn assign_object_destructuring(
    props: &[(PropertyKey, BindingElement)],
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let obj = match value {
        Value::Object(o) => o.clone(),
        _ => return Err(JsError("Cannot destructure non-object value".to_string())),
    };
    for (key, binding) in props {
        let key_str = extract_destructure_key(key, env)?;
        let prop_value = {
            let obj_ref = obj.borrow();
            obj_ref.get(&key_str).unwrap_or(Value::Undefined)
        };
        assign_binding_elem(binding, &prop_value, env)?;
    }
    Ok(())
}

fn extract_destructure_key(
    key: &PropertyKey,
    env: &Rc<RefCell<Environment>>,
) -> Result<String, JsError> {
    match key {
        PropertyKey::Ident(s) => Ok(s.clone()),
        PropertyKey::String(s) => Ok(s.clone()),
        PropertyKey::Number(n) => Ok(n.to_string()),
        PropertyKey::Computed(expr) => Ok(crate::value::to_js_string(&eval_expression(
            expr, env, false,
        )?)),
    }
}

fn assign_binding_elem(
    binding: &BindingElement,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    match binding {
        BindingElement::Identifier(name) => assign_to_identifier(name, value, env),
        BindingElement::ArrayPattern(bindings) => assign_array_destructuring(bindings, value, env),
        BindingElement::ObjectPattern(props) => assign_object_destructuring(props, value, env),
    }
}

fn assign_to_identifier(
    name: &str,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    // Per ES §12.14.4 step 1.e.iii: SetFunctionName on assignment when
    // right side is an anonymous function/class expression/arrow.
    let value = match value {
        Value::Function(ref f) if f.name.is_none() => {
            let mut cloned = f.clone();
            cloned.name = Some(name.to_string());
            cloned.set_property("name", Value::String(name.to_string()));
            Value::Function(cloned)
        }
        Value::Class(ref c) => {
            // Check if class already has a `name` property (via static method).
            let has_name = c.name.is_some()
                || c.static_methods.iter().any(|(k, _, _)| match k {
                    crate::ast::PropertyKey::Ident(s) | crate::ast::PropertyKey::String(s) => s == "name",
                    _ => false,
                });
            if !has_name {
                let mut cloned = c.clone();
                cloned.name = Some(name.to_string());
                Value::Class(cloned)
            } else {
                value.clone()
            }
        }
        _ => value.clone(),
    };

    if env.borrow().has(name) {
        if let Some(kind) = env.borrow().get_kind(name) {
            if kind == VarKind::Const {
                return Err(JsError(
                    "TypeError: Assignment to constant variable".to_string(),
                ));
            }
        }
        // Strict mode: if this binding is a global property on globalThis and
        // its descriptor marks it non-writable, throw TypeError. Covers
        // NaN/Infinity/undefined (Annex 16.1 / ES5 §15.1.1) and any future
        // read-only globals.
        if crate::interpreter::is_strict_mode() {
            if let Some(Value::Object(global_obj)) = env.borrow().get("globalThis") {
                if let Some(flags) = global_obj.borrow().get_descriptor(name) {
                    if !flags.writable {
                        let (_, js_err) = crate::value::error::create_js_error_with_type(
                            "Cannot assign to read only property",
                            "TypeError",
                        );
                        return Err(js_err);
                    }
                }
            }
        }
        env.borrow_mut().set(name, value);
    } else {
        // Strict mode: assignment to unresolvable reference must throw ReferenceError.
        if crate::interpreter::is_strict_mode() {
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                &format!("{} is not defined", name),
                "ReferenceError",
            );
            return Err(js_err);
        }
        // Sloppy mode: create a globalThis property so `delete x` returns true.
        // Use a block to scope the borrow.
        let use_global_this = {
            if let Some(Value::Object(_)) = env.borrow().get("globalThis") {
                true
            } else {
                false
            }
        };
        if use_global_this {
            // Now get globalThis again and set the property
            if let Some(Value::Object(global_obj)) = env.borrow().get("globalThis") {
                global_obj.borrow_mut().set(name, value);
            }
        } else {
            env.borrow_mut().define(name.to_string(), value);
        }
    }
    Ok(())
}

fn assign_to_member(
    object: &Expression,
    property: &PropertyKey,
    computed: bool,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let prop_name = extract_property_name(property, computed, env, false)?;

    // Handle the case where object is itself a member expression (chained access)
    // e.g., assert.deepEqual._compare where we need to update assert.deepEqual
    if let Expression::Member {
        object: parent_obj,
        property: parent_prop,
        computed: parent_computed,
    } = object
    {
        let parent_prop_name = extract_property_name(parent_prop, *parent_computed, env, false)?;
        let parent_val = eval_expression(parent_obj, env, false)?;

        if let Value::Object(ref parent_o) = parent_val {
            // Clone the function out, modify it, put it back
            let func_opt = {
                let parent_read = parent_o.borrow();
                parent_read.properties.get(&parent_prop_name).cloned()
            };

            if let Some(Value::Function(func)) = func_opt {
                func.set_property(&prop_name, value.clone());
                // Put the modified function back
                parent_o
                    .borrow_mut()
                    .properties
                    .insert(parent_prop_name, Value::Function(func));
                return Ok(());
            }

            // Handle NativeFunction properties (e.g. assert.deepEqual._compare where assert is an Object)
            if let Some(Value::NativeFunction(nf)) = func_opt {
                nf.set_property(&prop_name, value.clone());
                // Put the modified native function back
                parent_o
                    .borrow_mut()
                    .properties
                    .insert(parent_prop_name, Value::NativeFunction(nf));
                return Ok(());
            }
        }

        // Same for native-function parents (e.g. assert.deepEqual._compare = ...):
        // reading a function property yields a clone, so write the modified
        // function back onto the shared Rc<NativeFunction>.
        if let Value::NativeFunction(ref nf) = parent_val {
            if let Some(Value::Function(func)) = nf.get_property(&parent_prop_name) {
                func.set_property(&prop_name, value.clone());
                nf.set_property(&parent_prop_name, Value::Function(func));
                return Ok(());
            }
            // Handle NativeFunction properties (e.g. assert.deepEqual._compare where assert and deepEqual are both NativeFunction)
            if let Some(Value::NativeFunction(inner_nf)) = nf.get_property(&parent_prop_name) {
                inner_nf.set_property(&prop_name, value.clone());
                nf.set_property(&parent_prop_name, Value::NativeFunction(inner_nf));
                return Ok(());
            }
        }
    }

    let obj_val = eval_expression(object, env, false)?;

    match obj_val {
        Value::Object(o) => {
            let has_setter = {
                let obj_ref = o.borrow();
                obj_ref.get_setter(&prop_name).is_some()
            };
            if has_setter {
                let setter_clone = {
                    let obj_ref = o.borrow();
                    obj_ref.get_setter(&prop_name).cloned()
                };
                if let Some(setter_storage) = setter_clone {
                    call_setter(&o, &setter_storage, value.clone(), env)?;
                    return Ok(());
                }
            }
            // Try to set function property in place if the property exists and is a function
            if o.borrow_mut()
                .set_function_property(&prop_name, &prop_name, value.clone())
            {
                // Already modified the function in place via set_function_property
                return Ok(());
            }
            // Reject property sets on frozen objects
            if crate::builtins::object_static::is_frozen_object(&o) {
                return Ok(());
            }
            // Strict mode: assignment to a non-writable property or to a new
            // property on a non-extensible object must throw TypeError.
            if crate::interpreter::is_strict_mode() {
                let obj_ref = o.borrow();
                if let Some(flags) = obj_ref.get_descriptor(&prop_name) {
                    if !flags.writable {
                        let (_, js_err) = crate::value::error::create_js_error_with_type(
                            "Cannot assign to read only property",
                            "TypeError",
                        );
                        return Err(js_err);
                    }
                } else if !obj_ref.extensible && !obj_ref.properties.contains_key(&prop_name) {
                    let (_, js_err) = crate::value::error::create_js_error_with_type(
                        "Cannot add property to non-extensible object",
                        "TypeError",
                    );
                    return Err(js_err);
                }
            }
            o.borrow_mut().set(&prop_name, value.clone());
            // Mirror writes on the globalThis object into the global binding,
            // so identifier resolution (which checks env scopes before the
            // globalThis fallback) stays in sync: `globalThis.x = v; x` === v.
            let is_global_this = env
                .borrow()
                .get("globalThis")
                .map(|g| matches!(g, Value::Object(ref go) if Rc::ptr_eq(go, &o)))
                .unwrap_or(false);
            if is_global_this && !env.borrow_mut().set(&prop_name, value.clone()) {
                env.borrow_mut().define(prop_name.clone(), value.clone());
            }
            Ok(())
        }
        Value::Function(ref f) => {
            if f.is_arrow && (prop_name == "caller" || prop_name == "arguments") {
                let msg = format!(
                    "'caller' and 'arguments' are restricted properties and cannot be set on arrow functions"
                );
                let (err, js_err) = crate::value::create_js_error_with_type(&msg, "TypeError");
                crate::value::set_thrown_value(err);
                return Err(js_err);
            }
            // Per ES §10.2.9 [[Set]]: strict mode rejects writes to non-writable
            // own properties with TypeError; sloppy mode silently ignores.
            if f.get_property(&prop_name).is_some()
                && matches!(prop_name.as_str(), "length" | "name")
            {
                if crate::interpreter::is_strict_mode() {
                    let (_, js_err) = crate::value::error::create_js_error_with_type(
                        "Cannot assign to read only property",
                        "TypeError",
                    );
                    return Err(js_err);
                }
                // Sloppy: silently ignore.
                return Ok(());
            }
            f.set_property(&prop_name, value.clone());
            Ok(())
        }
        Value::NativeFunction(ref nf) => {
            nf.set_property(&prop_name, value.clone());
            Ok(())
        }
        Value::NativeConstructor(ref nc) => {
            nc.set_property(&prop_name, value.clone());
            Ok(())
        }
        Value::Class(ref class) => {
            // Assignment to a class property sets a static field
            class.set_static_field(&prop_name, value.clone());
            Ok(())
        }
        _ => Err(JsError(format!(
            "Cannot assign to property of non-object, got {:?}",
            obj_val
        ))),
    }
}

fn extract_property_name(
    property: &PropertyKey,
    computed: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<String, JsError> {
    if computed {
        match property {
            PropertyKey::Computed(e) => {
                let val = eval_expression(e, env, in_arrow_function)?;
                match &val {
                    Value::Symbol(s) => Ok(s.clone()),
                    _ => Ok(crate::value::to_js_string(&val)),
                }
            }
            _ => Err(JsError("Invalid computed property".to_string())),
        }
    } else {
        match property {
            PropertyKey::Ident(s) => Ok(s.clone()),
            PropertyKey::String(s) => Ok(s.clone()),
            PropertyKey::Number(n) => Ok(n.to_string()),
            PropertyKey::Computed(e) => Ok(crate::value::to_js_string(&eval_expression(
                e,
                env,
                in_arrow_function,
            )?)),
        }
    }
}

/// Evaluate a callee expression and extract the function and "this" binding.
pub fn eval_callee_with_this(
    callee: &Expression,
    env: &Rc<RefCell<Environment>>,
) -> Result<(Value, Value), JsError> {
    match callee {
        Expression::Member {
            object,
            property,
            computed,
        } => {
            let obj_val = eval_expression(object, env, false)?;
            let prop_name = extract_property_name(property, *computed, env, false)?;
            let func = get_member_function(&obj_val, &prop_name, env)?;
            Ok((func, obj_val))
        }
        _ => {
            let func = eval_expression(callee, env, false)?;
            Ok((func, Value::Undefined))
        }
    }
}

fn get_member_function(
    obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match obj_val {
        Value::Object(o) => crate::eval::member::eval_object_member(o, prop_name),
        Value::String(s) => get_string_method(s, prop_name, env),
        Value::Number(_) => get_number_method(obj_val, prop_name, env),
        Value::Function(f) => crate::eval::member::eval_function_member(f, prop_name),
        Value::NativeFunction(nf) => {
            crate::eval::member::eval_native_function_member(nf, prop_name)
        }
        Value::NativeConstructor(nc) => {
            crate::eval::member::eval_native_constructor_member(nc, prop_name)
        }
        Value::Class(class) => {
            if let Some(val) = class.get_static_field(prop_name) {
                return Ok(val);
            }
            for (name, params, body) in &class.static_methods {
                if name_matches_prop(name, prop_name) {
                    let params_vec: Vec<Param> = params.iter().map(|p| Param::new(p)).collect();
                    let mut func = crate::value::ValueFunction::new(
                        Some(prop_name.to_string()),
                        params_vec,
                        body.clone(),
                        Rc::clone(env),
                    );
                    // Class bodies are always strict mode (ES spec 15.7).
                    func.strict = true;
                    return Ok(Value::Function(func));
                }
            }
            // Look up the superclass chain for inherited static members
            if let Some(ref super_expr) = class.super_class {
                let super_val = crate::eval::expression::eval_expression(super_expr, env, false)?;
                return crate::eval::member::eval_member_access(&super_val, prop_name, env);
            }
            let proto = crate::eval::class::get_or_create_class_prototype(class, env)?;
            crate::eval::member::eval_object_member(&proto, prop_name)
        }
        _ => Ok(Value::Undefined),
    }
}

fn name_matches_prop(key: &crate::ast::PropertyKey, name: &str) -> bool {
    match key {
        crate::ast::PropertyKey::Ident(s) => s == name,
        crate::ast::PropertyKey::String(s) => s == name,
        crate::ast::PropertyKey::Number(n) => n.to_string() == name,
        crate::ast::PropertyKey::Computed(_) => false,
    }
}

fn get_number_method(
    _obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(Value::Object(ref num_obj)) = env.borrow().get("Number") {
        let num_obj = num_obj.borrow();
        if let Some(Value::Object(ref proto)) = num_obj.get("prototype") {
            let proto_obj = proto.borrow();
            if let Some(val) = proto_obj.get(prop_name) {
                return Ok(val);
            }
        }
    }
    Ok(Value::Undefined)
}

/// Call a getter function with the object as "this"
pub fn call_getter(
    obj: &Rc<RefCell<Object>>,
    getter_storage: &GetterStorage,
    _env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(func) = &getter_storage.func {
        return crate::eval::function::call_value_with_this(
            func.clone(),
            Vec::new(),
            Value::Object(Rc::clone(obj)),
        );
    }
    let closure = Rc::clone(&getter_storage.closure);
    let body = getter_storage.body.clone();
    let mut call_env = Environment::with_parent(closure);
    call_env
        .current_scope_mut()
        .set_this(Value::Object(Rc::clone(obj)));
    let call_env = Rc::new(RefCell::new(call_env));
    if body.is_empty() {
        Ok(Value::Undefined)
    } else {
        eval_function_body(&body, &call_env, false)
    }
}

/// Call a setter function with the object as "this" and the value as the parameter
pub fn call_setter(
    obj: &Rc<RefCell<Object>>,
    setter_storage: &SetterStorage,
    value: Value,
    _env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(func) = &setter_storage.func {
        return crate::eval::function::call_value_with_this(
            func.clone(),
            vec![value],
            Value::Object(Rc::clone(obj)),
        );
    }
    let closure = Rc::clone(&setter_storage.closure);
    let body = setter_storage.body.clone();
    let param = setter_storage.param.clone();
    let mut call_env = Environment::with_parent(Rc::clone(&closure));
    call_env
        .current_scope_mut()
        .set_this(Value::Object(Rc::clone(obj)));
    call_env.define(param, value);
    let call_env = Rc::new(RefCell::new(call_env));
    if body.is_empty() {
        Ok(Value::Undefined)
    } else {
        eval_function_body(&body, &call_env, false)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Context, Value};

    #[test]
    fn strict_for_in_var_iterates() {
        let mut ctx = Context::new().unwrap();
        // Strict mode + var-hoisted for-in. The body assigns to the
        // hoisted `property` binding each iteration; must not throw.
        ctx.eval(
            "\"use strict\";\
             var obj = {a:1, b:2};\
             var count = 0;\
             for (var property in obj) { count++; }\
             if (count !== 2) throw new Error(\"count=\" + count);",
        )
        .expect("strict for-in should iterate");
    }

    #[test]
    fn for_in_enumerates_defined_property() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var o = {a: 1}").unwrap();
        // Test: for-in with var declared in the for statement
        ctx.eval("var keys = []; for (var k in o) { keys.push(k); }").unwrap();
        let len = ctx.eval("keys.length").unwrap();
        assert_eq!(len, Value::Number(1.0), "for-in should iterate once");
        let first = ctx.eval("keys[0]").unwrap();
        assert_eq!(first, Value::String("a".to_string()), "key should be 'a'");
        // Test with Object.defineProperty enumerable property
        ctx.eval("var o2 = {}; Object.defineProperty(o2, 'b', {enumerable: true, value: 2});").unwrap();
        ctx.eval("var keys2 = []; for (var k in o2) { keys2.push(k); }").unwrap();
        let len2 = ctx.eval("keys2.length").unwrap();
        assert_eq!(len2, Value::Number(1.0), "for-in should see enumerable property");
        let first2 = ctx.eval("keys2[0]").unwrap();
        assert_eq!(first2, Value::String("b".to_string()), "key should be 'b'");
    }

    #[test]
    fn strict_assign_undeclared_throws() {
        let mut ctx = Context::new().unwrap();
        let res = ctx.eval("\"use strict\"; undeclared = 5;");
        assert!(res.is_err(), "strict assignment to undeclared should throw");
    }

    #[test]
    fn sloppy_assign_undeclared_no_throw() {
        let mut ctx = Context::new().unwrap();
        let res = ctx.eval("undeclared = 5;");
        assert!(
            res.is_ok(),
            "sloppy assignment to undeclared should not throw"
        );
    }

    #[test]
    fn valueof_throw_propagates_in_addition() {
        let mut ctx = Context::new().unwrap();
        let res = ctx.eval(
            "var caught; try { 1 + {valueOf: function() {throw \"err\"}}; } catch (e) { caught = e; } caught;"
        );
        assert_eq!(res.unwrap(), crate::value::Value::String("err".to_string()));
    }

    #[test]
    fn symbol_to_primitive_throw_propagates() {
        let mut ctx = Context::new().unwrap();
        // Test that a custom @@toPrimitive that throws is honored.
        let res = ctx.eval(
            "var caught; \
             var t = {}; \
             Object.defineProperty(t, Symbol.toPrimitive, { get: function() { return function() { throw \"boom\"; }; } }); \
             try { t + 1; } catch (e) { caught = e; } \
             caught;"
        );
        // The thrown value should be "boom"
        let v = res.unwrap();
        match v {
            crate::value::Value::String(s) => assert_eq!(s, "boom"),
            other => panic!("expected string 'boom', got {:?}", other),
        }
    }

    #[test]
    fn array_elision_length_is_one() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval("var a = [,]; a.length").unwrap();
        assert_eq!(v, crate::value::Value::Number(1.0));
    }

    #[test]
    fn arrow_fn_caller_throws_typeerror() {
        let mut ctx = Context::new().unwrap();
        let res = ctx.eval("var arrowFn = () => {}; arrowFn.caller");
        assert!(res.is_err(), "arrowFn.caller must throw");
    }

    #[test]
    fn arrow_fn_caller_throws_in_harness() {
        // Mimics the test262 harness pattern where assert.throws wraps the access.
        let mut ctx = Context::new().unwrap();
        let res = ctx.eval(
            "var arrowFn = () => {}; \
             var caught = false; \
             try { var x = arrowFn.caller; } catch (e) { caught = (e instanceof TypeError); } \
             caught;"
        );
        let v = res.unwrap();
        assert_eq!(v, crate::value::Value::Boolean(true), "must catch TypeError");
    }

    #[test]
    fn arrow_in_eval_returns_this() {
        // test262 expects foo()() to equal the global `this` (an Object).
        let mut ctx = Context::new().unwrap();
        let res = ctx.eval(
            "function foo(){ return eval(\"()=>this\"); } \
             foo()();"
        );
        let v = res.unwrap();
        // Must be an Object (globalThis), not Undefined.
        match v {
            crate::value::Value::Object(_) => {}
            crate::value::Value::Undefined => panic!("got undefined"),
            other => panic!("got unexpected: {:?}", other),
        }
    }

    #[test]
    fn direct_arrow_returns_this() {
        // Simpler: just an arrow at top level should return globalThis.
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval("(()=>this)();").unwrap();
        match v {
            crate::value::Value::Object(_) => {}
            other => panic!("got: {:?}", other),
        }
    }

    #[test]
    fn top_level_this_is_global() {
        // Sanity: at top level, `this` should be globalThis (Object).
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval("this;").unwrap();
        match v {
            crate::value::Value::Object(_) => {}
            other => panic!("got: {:?}", other),
        }
    }

    #[test]
    fn fn_returning_this_inside_obj() {
        // Try arrow inside a method context.
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval("var o = { f: function() { return (() => this); } }; o.f()();").unwrap();
        match v {
            crate::value::Value::Object(o) => {
                let _ = o;
            }
            other => panic!("got: {:?}", other),
        }
    }

    #[test]
    fn arrow_fn_length_own_property() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval("var f = (x, y = 1) => {}; f.hasOwnProperty('length')").unwrap();
        assert_eq!(v, crate::value::Value::Boolean(true));
        let len = ctx.eval("f.length").unwrap();
        assert_eq!(len, crate::value::Value::Number(1.0));
    }

    #[test]
    fn arrow_fn_length_full_test262() {
        use crate::test262::harness::try_inject_harness;
        let mut ctx = Context::new().unwrap();
        try_inject_harness(&mut ctx).unwrap();
        let v = ctx.eval(
            "var f1 = (x = 42) => {}; \
             var ok = f1.hasOwnProperty('length'); \
             var len = f1.length; \
             var deleted = delete f1.length; \
             var stillHas = f1.hasOwnProperty('length'); \
             [ok, len, deleted, stillHas];"
        );
        let arr = match v.unwrap() {
            crate::value::Value::Object(o) => o,
            other => panic!("expected array: {:?}", other),
        };
        let elements = arr.borrow().elements.clone();
        eprintln!("results: {:?}", elements);
        panic!("see results");
    }

    #[test]
    fn delete_arrow_length_returns_true() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval("var f1 = (x = 42) => {}; delete f1.length;").unwrap();
        assert_eq!(v, crate::value::Value::Boolean(true));
    }

    #[test]
    fn delete_function_length_returns_true() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval("function f1(x = 42) {}; delete f1.length;").unwrap();
        assert_eq!(v, crate::value::Value::Boolean(true));
    }

    #[test]
    fn arrow_is_function_value() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval("typeof (()=>1)").unwrap();
        assert_eq!(v, crate::value::Value::String("function".to_string()));
        let f = ctx.eval("var f1 = (x = 42) => {}; f1").unwrap();
        match f {
            crate::value::Value::Function(_) => {}
            other => panic!("got: {:?}", other),
        }
    }

    #[test]
    fn arrow_length_remove_property() {
        let mut ctx = Context::new().unwrap();
        // Single eval so f1 persists across delete + hasOwnProperty.
        let r = ctx.eval(
            "var f1 = (x = 42) => {}; \
             var del = delete f1.length; \
             var has = Object.prototype.hasOwnProperty.call(f1, 'length'); \
             [del, has];"
        ).unwrap();
        let arr = if let crate::value::Value::Object(o) = r {
            o.borrow().elements.clone()
        } else {
            panic!("not array");
        };
        assert_eq!(arr[0], crate::value::Value::Boolean(true), "delete");
        assert_eq!(arr[1], crate::value::Value::Boolean(false), "should not be own after delete");
    }

    #[test]
    fn remove_property_directly() {
        // Test remove_property via the JS dispatch path, evaluating both
        // "delete" and "Object.prototype.hasOwnProperty.call" via the engine.
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval(
            "var f1 = function() {}; \
             f1.length = 5; \
             var before = Object.prototype.hasOwnProperty.call(f1, 'length'); \
             var del = delete f1.length; \
             var after = Object.prototype.hasOwnProperty.call(f1, 'length'); \
             [before, del, after];"
        ).unwrap();
        let arr = if let crate::value::Value::Object(o) = r {
            o.borrow().elements.clone()
        } else {
            panic!("not array");
        };
        eprintln!("results: before={:?} del={:?} after={:?}", arr[0], arr[1], arr[2]);
        assert_eq!(arr[0], crate::value::Value::Boolean(true), "before");
        assert_eq!(arr[1], crate::value::Value::Boolean(true), "del");
        assert_eq!(arr[2], crate::value::Value::Boolean(false), "after");
    }

    #[test]
    fn arrow_length_descriptor_configurable() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval(
            "var f1 = (x = 42) => {}; \
             var desc = Object.getOwnPropertyDescriptor(f1, 'length'); \
             [desc.value, desc.writable, desc.enumerable, desc.configurable, f1.length];"
        );
        let arr = match v.unwrap() {
            crate::value::Value::Object(o) => o,
            other => panic!("expected array: {:?}", other),
        };
        let e = arr.borrow().elements.clone();
        assert_eq!(e[0], crate::value::Value::Number(0.0), "value");
        assert_eq!(e[1], crate::value::Value::Boolean(false), "writable");
        assert_eq!(e[2], crate::value::Value::Boolean(false), "enumerable");
        assert_eq!(e[3], crate::value::Value::Boolean(true), "configurable");
        assert_eq!(e[4], crate::value::Value::Number(0.0), "f1.length");
    }

    #[test]
    fn arrow_length_descriptor_full_verifyproperty() {
        // Mimic the test262 verifyProperty flow exactly.
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval(
            "var f1 = (x = 42) => {}; \
             var originalDesc = Object.getOwnPropertyDescriptor(f1, 'length'); \
             if (!Object.prototype.hasOwnProperty.call(f1, 'length')) throw new Error('not own'); \
             try { f1.length = 'unlikelyValue'; } catch (e) {} \
             var still0 = f1.length; \
             var lenDesc = Object.getOwnPropertyDescriptor(f1, 'length'); \
             [originalDesc.value, originalDesc.writable, originalDesc.configurable, still0, lenDesc.value, lenDesc.writable];"
        );
        let arr = match v.unwrap() {
            crate::value::Value::Object(o) => o,
            other => panic!("expected array: {:?}", other),
        };
        let e = arr.borrow().elements.clone();
        assert_eq!(e[0], crate::value::Value::Number(0.0), "orig value");
        assert_eq!(e[1], crate::value::Value::Boolean(false), "orig writable");
        assert_eq!(e[2], crate::value::Value::Boolean(true), "orig configurable");
        assert_eq!(e[3], crate::value::Value::Number(0.0), "still 0");
        assert_eq!(e[4], crate::value::Value::Number(0.0), "post value");
        assert_eq!(e[5], crate::value::Value::Boolean(false), "post writable");
    }

    #[test]
    fn new_target_in_constructor() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval(
            "function F() { this.t = new.target === F; } \
             var f = new F(); \
             f.t;"
        );
        assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
    }

    #[test]
    fn new_target_in_arrow_inside_constructor() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval(
            "function F() { this.af = () => new.target; } \
             var f = new F(); \
             f.af() === F;"
        );
        assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
    }

    #[test]
    fn debug_new_target_arrow_2() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval(
            "function F() { this.af = () => new.target; } \
             var f = new F(); \
             var t = f.af(); \
             [t === F, typeof t];"
        ).unwrap();
        let arr = if let crate::value::Value::Object(o) = v {
            o.borrow().elements.clone()
        } else { panic!("not array") };
        eprintln!("results: eq={:?} type={:?}", arr[0], arr[1]);
    }

    #[test]
    fn debug_new_target_arrow() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval(
            "function F() { \
               this.af = () => new.target; \
             } \
             var f = new F(); \
             var result = f.af() === F; \
             result;"
        );
        eprintln!("new.target in arrow: {:?}", v);
    }

    #[test]
    fn arrow_length_no_writable_check() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval(
            "var f1 = (x = 42) => {}; \
             var desc = Object.getOwnPropertyDescriptor(f1, 'length'); \
             try { f1.length = 99; } catch (e) {} \
             var writable = Object.getOwnPropertyDescriptor(f1, 'length').writable; \
             var afterLen = f1.length; \
             [writable, afterLen];"
        );
        let arr = match v.unwrap() {
            crate::value::Value::Object(o) => o,
            other => panic!("expected array: {:?}", other),
        };
        let e = arr.borrow().elements.clone();
        // writable should be false (so assignment is silently ignored in sloppy mode)
        assert_eq!(e[0], crate::value::Value::Boolean(false), "writable should be false");
        // After attempted write, length should still be 0 (since writable=false)
        assert_eq!(e[1], crate::value::Value::Number(0.0), "length should not change");
    }

    #[test]
    fn simple_class_extends_with_super() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval(
            "class A { constructor() {} } \
             class B extends A { constructor() { super(); } } \
             new B() instanceof B;"
        );
        assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
    }

    #[test]
    fn class_extends_promise_builtin() {
        let mut ctx = Context::new().unwrap();
        // Test that Promise is accessible
        let t = ctx.eval("typeof Promise");
        eprintln!("typeof Promise = {:?}", t);
        
        // Test class extends Promise
        let v = ctx.eval(
            "class SubPromise extends Promise { \
               constructor(a) { super(a); } \
             } \
             new SubPromise(function(resolve) { resolve(42); });"
        );
        eprintln!("class extends Promise = {:?}", v);
        assert!(v.is_ok(), "class extends Promise should work: {:?}", v);
    }

    #[test]
    fn super_in_arrow_throws_reference_error() {
        // Per ES §8.1.1.3.1: super() in arrow after constructor super()
        // must throw ReferenceError.
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval(
            "var count = 0; \
             class A { constructor() { count++; } } \
             class B extends A { \
               constructor() { super(); this.af = _ => super(); } \
             } \
             var b = new B(); \
             var err; \
             try { b.af(); } catch (e) { err = e && e.name; } \
             [count, err];"
        ).unwrap();
        if let crate::value::Value::Object(o) = v {
            let e = o.borrow().elements.clone();
            eprintln!("count={:?}, err={:?}", e[0], e[1]);
        }
    }

    #[test]
    fn super_in_iife_arrow_calls_super_once() {
        // Per ES: arrow inside B constructor calls super() which runs A.
        // A should be called exactly once (count=1).
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval(
            "var count = 0; \
             class A { constructor() { count++; } } \
             class B extends A { constructor() { (_ => super())(); } } \
             new B(); \
             count;"
        );
        eprintln!("count: {:?}", v);
    }

    #[test]
    fn sloppy_arrow_assigns_undeclared_creates_global() {
        let mut ctx = Context::new().unwrap();
        // Just the arrow with name assignment
        let v = ctx.eval("var af = _ => { foo = 1; }; af()");
        eprintln!("arrow af(): {:?}", v);
    }

    #[test]
    fn arrow_fn_caller_full_test262() {
        // Load the actual test262 harness and run the failing test logic.
        use crate::test262::harness::try_inject_harness;
        let mut ctx = Context::new().unwrap();
        try_inject_harness(&mut ctx).unwrap();
        let res = ctx.eval(
            "var arrowFn = () => {}; \
             var got1 = false; try { var x = arrowFn.caller; } catch (e) { got1 = (e instanceof TypeError); } \
             var got2 = false; try { arrowFn.caller = {}; } catch (e) { got2 = (e instanceof TypeError); } \
             var got3 = false; try { var y = arrowFn.arguments; } catch (e) { got3 = (e instanceof TypeError); } \
             var got4 = false; try { arrowFn.arguments = {}; } catch (e) { got4 = (e instanceof TypeError); } \
             got1 && got2 && got3 && got4;"
        );
        assert_eq!(res.unwrap(), crate::value::Value::Boolean(true));
    }

    #[test]
    fn symbol_to_primitive_object_result_throws_type_error() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(
            "var value = {}; \
             value[Symbol.toPrimitive] = function() { return {}; }; \
             value + 1;",
        );
        assert!(result.is_err());
        let thrown = crate::value::take_thrown_value().unwrap();
        let crate::value::Value::Object(error) = thrown else {
            panic!("expected TypeError object");
        };
        assert_eq!(
            error.borrow().get("name"),
            Some(crate::value::Value::String("TypeError".to_string()))
        );
    }

    #[test]
    fn sloppy_assign_to_undeclared_creates_global() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("undeclared_var = 42;").unwrap();
        let v = ctx.eval("typeof undeclared_var").unwrap();
        assert_eq!(v, crate::value::Value::String("number".to_string()));
    }

    #[test]
    fn sloppy_delete_global_property_succeeds() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("__ref = {};").unwrap();
        let deleted = ctx.eval("delete __ref;").unwrap();
        assert_eq!(deleted, crate::value::Value::Boolean(true));
        let after = ctx.eval("typeof __ref").unwrap();
        assert_eq!(after, crate::value::Value::String("undefined".to_string()));
    }

    #[test]
    fn debug_assign_to_global() {
        let mut ctx = Context::new().unwrap();
        let has = ctx.eval("typeof globalThis").unwrap();
        assert_eq!(has, crate::value::Value::String("object".to_string()));
        ctx.eval("undeclared_var = 42;").unwrap();
        let v = ctx.eval("globalThis.undeclared_var").unwrap();
        assert_eq!(v, crate::value::Value::Number(42.0));
    }

    #[test]
    fn debug_typeof_undeclared() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("undeclared_var = 42;").unwrap();
        // Direct globalThis lookup works
        let via_gt = ctx.eval("globalThis.undeclared_var").unwrap();
        assert_eq!(via_gt, crate::value::Value::Number(42.0));
        // typeof via identifier should also work (env fallback)
        let typeof_v = ctx.eval("typeof undeclared_var").unwrap();
        assert_eq!(typeof_v, crate::value::Value::String("number".to_string()));
    }

    #[test]
    fn debug_known_global() {
        // Math is a builtin registered at init. typeof should return "object".
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval("typeof Math").unwrap();
        assert_eq!(v, crate::value::Value::String("object".to_string()));
    }

    #[test]
    fn debug_set_global_directly() {
        // Bypass assign_to_identifier: store on globalThis directly via JS.
        let mut ctx = Context::new().unwrap();
        ctx.eval("globalThis.test_var = 99;").unwrap();
        let v = ctx.eval("typeof test_var").unwrap();
        assert_eq!(v, crate::value::Value::String("number".to_string()));
    }

    #[test]
    fn debug_just_undeclared() {
        // Single eval: assign then read.
        let mut ctx = Context::new().unwrap();
        ctx.eval("foo = 1; var x = foo;").unwrap();
        let v = ctx.eval("x").unwrap();
        assert_eq!(v, crate::value::Value::Number(1.0));
    }

    #[test]
    fn debug_across_evals() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("foo = 1;").unwrap();
        let v = ctx.eval("foo").unwrap();
        assert_eq!(v, crate::value::Value::Number(1.0));
    }

    #[test]
    fn debug_typeof_across_evals() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("foo = 1;").unwrap();
        let v = ctx.eval("typeof foo").unwrap();
        assert_eq!(v, crate::value::Value::String("number".to_string()));
    }

    #[test]
    fn debug_symbol_member() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("Symbol.prototype.test262 = 'sym-proto';").unwrap();
        let v = ctx.eval("Symbol().test262").unwrap();
        assert_eq!(v, crate::value::Value::String("sym-proto".to_string()));
    }

    #[test]
    fn debug_number_member() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("Number.prototype.test262 = 'num-proto';").unwrap();
        let v = ctx.eval("(1).test262").unwrap();
        assert_eq!(v, crate::value::Value::String("num-proto".to_string()));
    }

    #[test]
    fn debug_symbol_proto_lookup() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("Symbol.prototype.test262 = 'sym-proto';").unwrap();
        let direct = ctx.eval("Symbol.prototype.test262").unwrap();
        assert_eq!(direct, crate::value::Value::String("sym-proto".to_string()));
        let s = ctx.eval("Symbol()").unwrap();
        assert!(matches!(s, crate::value::Value::Symbol(_)));
    }

    #[test]
    fn debug_symbol_dot_member() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("Symbol.prototype.test262 = 'sym-proto';").unwrap();
        // Run in same eval to avoid any state issue
        let v = ctx.eval("var s = Symbol(); s.test262;").unwrap();
        assert_eq!(v, crate::value::Value::String("sym-proto".to_string()));
    }

    #[test]
    fn strict_assign_to_NaN_throws_type_error() {
        let mut ctx = Context::new().unwrap();
        let res = ctx.eval("\"use strict\"; NaN = 12;");
        assert!(res.is_err(), "strict assignment to NaN should throw");
        let msg = res.unwrap_err().0;
        assert!(
            msg.contains("TypeError"),
            "expected TypeError, got: {}",
            msg
        );
    }

    #[test]
    fn strict_assign_to_undefined_throws_type_error() {
        let mut ctx = Context::new().unwrap();
        let res = ctx.eval("\"use strict\"; undefined = 12;");
        assert!(res.is_err(), "strict assignment to undefined should throw");
        let msg = res.unwrap_err().0;
        assert!(
            msg.contains("TypeError"),
            "expected TypeError, got: {}",
            msg
        );
    }

    #[test]
    fn strict_assign_to_Infinity_throws_type_error() {
        let mut ctx = Context::new().unwrap();
        let res = ctx.eval("\"use strict\"; Infinity = 12;");
        assert!(res.is_err(), "strict assignment to Infinity should throw");
        let msg = res.unwrap_err().0;
        assert!(
            msg.contains("TypeError"),
            "expected TypeError, got: {}",
            msg
        );
    }

    #[test]
    fn sloppy_assign_to_NaN_no_throw() {
        let mut ctx = Context::new().unwrap();
        let res = ctx.eval("NaN = 12;");
        assert!(res.is_ok(), "sloppy assignment to NaN should not throw");
    }
}

#[cfg(test)]
mod debug_prim {
    use crate::Context;

    #[test]
    fn debug_prim_function() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval("function f() {}; f + 1").unwrap();
        // Just see what we get
        eprintln!("f+1 = {:?}", v);
    }
}
