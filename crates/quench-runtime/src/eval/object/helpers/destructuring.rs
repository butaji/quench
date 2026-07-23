//! Destructuring assignment helpers.

use crate::ast::*;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::value::{JsError, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Box a primitive value for property assignment (ES §10.2.9 [[Set]]).
pub fn box_primitive_for_set(
    obj_val: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    let ctor_name = match obj_val {
        Value::Number(_) => "Number",
        Value::Boolean(_) => "Boolean",
        Value::Symbol(_) => "Symbol",
        Value::String(_) => "String",
        _ => {
            return Err(JsError(
                "box_primitive_for_set: not a primitive".to_string(),
            ))
        }
    };
    let ctor_val = env
        .borrow()
        .get(ctor_name)
        .ok_or_else(|| JsError(format!("{} not found", ctor_name)))?;
    let proto = match &ctor_val {
        Value::Object(o) => o.borrow().get("prototype"),
        Value::NativeFunction(nf) => nf
            .prototype
            .borrow()
            .as_ref()
            .map(|p| Value::Object(Rc::clone(p))),
        Value::NativeConstructor(nc) => Some(Value::Object(Rc::clone(&nc.prototype))),
        _ => None,
    };
    let proto_rc = match proto {
        Some(Value::Object(o)) => o,
        _ => return Err(JsError(format!("{} prototype not found", ctor_name))),
    };
    let mut boxed = Object::new(ObjectKind::Ordinary);
    boxed.prototype = Some(Rc::clone(&proto_rc));
    match obj_val {
        Value::Number(n) => {
            boxed.exotic_kind = Some(crate::value::kind::ExoticKind::Number);
            boxed.set("_value", Value::Number(*n));
        }
        Value::Boolean(b) => {
            boxed.exotic_kind = Some(crate::value::kind::ExoticKind::Boolean);
            boxed.set("_value", Value::Boolean(*b));
        }
        Value::Symbol(_) => {}
        _ => {}
    }
    Ok(Rc::new(RefCell::new(boxed)))
}

/// Assign to an array destructuring pattern.
pub fn assign_array_destructuring(
    bindings: &[BindingElement],
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    if let Value::String(s) = value {
        let chars: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
        let len = chars.len();
        let mut arr = Object::new(ObjectKind::Array);
        arr.elements = chars;
        arr.properties
            .insert("length".to_string(), Value::Number(len as f64));
        return assign_array_with_iterator(bindings, &Rc::new(RefCell::new(arr)), env);
    }
    let Value::Object(arr_rc) = value else {
        if let Value::Generator(gen) = value {
            let iter = crate::value::generator::generator_as_iterator_object(Rc::clone(gen));
            return assign_array_with_iterator(bindings, &iter, env);
        }
        return Err(JsError("Cannot destructure non-iterable value".to_string()));
    };
    if arr_rc.borrow().kind == ObjectKind::Array {
        return assign_array_with_iterator(bindings, arr_rc, env);
    }
    let iter = obtain_iterator(arr_rc);
    let iterator = iter.as_ref().unwrap_or(arr_rc);
    assign_array_with_iterator(bindings, iterator, env)
}

/// Obtain an iterator from an object (if it has Symbol.iterator).
fn obtain_iterator(o: &Rc<RefCell<Object>>) -> Option<Rc<RefCell<Object>>> {
    if o.borrow().properties.contains_key("next") {
        return Some(Rc::clone(o));
    }
    let symbol_obj = o.borrow().get("Symbol");
    let symbol_obj = match symbol_obj {
        Some(Value::Object(obj)) => obj,
        _ => return None,
    };
    let iter_value = symbol_obj.borrow().get("iterator");
    let iter_value = match iter_value {
        Some(value @ Value::Object(_)) => value,
        _ => return None,
    };
    let result = match crate::eval::function::call_value(iter_value.clone(), vec![]) {
        Ok(value) => value,
        Err(_) => return None,
    };
    if let Value::Object(obj) = result {
        Some(obj)
    } else {
        None
    }
}

/// Assign destructuring bindings using an iterator.
pub fn assign_array_with_iterator(
    bindings: &[BindingElement],
    iterator: &Rc<RefCell<Object>>,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let mut index = 0;
    for binding in bindings {
        let result = take_iterator_value(iterator, &mut index, env);
        let elem_value = match result {
            Ok(value) => value,
            Err(error) => {
                call_iterator_return(iterator);
                return Err(error);
            }
        };
        if let Err(error) = assign_binding_elem(binding, &elem_value, env) {
            let original = crate::value::take_thrown_value();
            let close_throw = call_iterator_return(iterator);
            if original.is_some() {
                if let Some(thrown) = original {
                    crate::value::set_thrown_value(thrown);
                }
            } else if let Some(close) = close_throw {
                return Err(close);
            }
            return Err(error);
        }
    }
    Ok(())
}

/// Take the next value from an iterator (or array-like).
pub fn take_iterator_value(
    iterator: &Rc<RefCell<Object>>,
    index: &mut usize,
    _env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if iterator.borrow().kind == ObjectKind::Array {
        let result = {
            let borrowed = iterator.borrow();
            if *index < borrowed.elements.len() {
                Some(borrowed.elements[*index].clone())
            } else {
                borrowed.properties.get(&index.to_string()).cloned()
            }
        };
        *index += 1;
        return Ok(result.unwrap_or(Value::Undefined));
    }
    let next_value = iterator.borrow().get("next");
    let Some(next_fn) = next_value else {
        return Ok(Value::Undefined);
    };
    let result = match next_fn {
        Value::Object(obj) => {
            crate::eval::function::call_value(Value::Object(Rc::clone(&obj)), vec![])?
        }
        other => crate::eval::function::call_value(other, vec![])?,
    };
    if crate::value::take_thrown_value().is_some() {
        return Err(JsError("TypeError: iterator threw".to_string()));
    }
    let Value::Object(result_obj) = result else {
        return Ok(Value::Undefined);
    };
    let done = result_obj.borrow().get("done");
    if let Some(value) = done {
        if matches!(value, Value::Boolean(true)) {
            return Ok(Value::Undefined);
        }
    }
    let value = result_obj.borrow().get("value");
    Ok(value.unwrap_or(Value::Undefined))
}

/// Call iterator.return, returning an error if it throws.
pub fn call_iterator_return(iterator: &Rc<RefCell<Object>>) -> Option<JsError> {
    let binding = iterator.borrow();
    if let Some(getter) = binding.get_getter("return") {
        let params: Vec<crate::ast::Param> = Vec::new();
        let body: Vec<crate::ast::Statement> = (*getter.body).clone();
        let closure = getter.closure.clone();
        let _ = crate::eval::function::call_value(
            crate::value::Value::Function(crate::value::ValueFunction::new_arrow(
                params,
                Box::new(crate::ast::ArrowBody::Block(std::rc::Rc::new(body))),
                closure,
            )),
            vec![],
        );
        if let Some(thrown) = crate::value::take_thrown_value() {
            return Some(JsError(crate::value::to_js_string(&thrown)));
        }
        return None;
    }
    let return_value = binding.get("return");
    let callable = match return_value {
        Some(Value::Object(_)) => true,
        Some(Value::Function(_)) => true,
        Some(Value::NativeFunction(_)) => true,
        Some(Value::NativeConstructor(_)) => true,
        Some(Value::Undefined) | None => return None,
        _ => false,
    };
    drop(binding);
    if !callable {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "iterator.return is not a function",
            "TypeError",
        );
        return Some(js_err);
    }
    let return_value = return_value.unwrap();
    let (body, closure) = {
        let binding = iterator.borrow();
        let getter = binding.get_getter("return");
        match getter {
            Some(getter) => (Some((*getter.body).clone()), Some(getter.closure.clone())),
            None => (None, None),
        }
    };
    let (body, closure) = match (body, closure) {
        (Some(body), Some(closure)) => (body, closure),
        (_, _) => {
            let _ = crate::eval::function::call_value(return_value, vec![]);
            if let Some(thrown) = crate::value::take_thrown_value() {
                return Some(JsError(crate::value::to_js_string(&thrown)));
            }
            return None;
        }
    };
    let params: Vec<crate::ast::Param> = Vec::new();
    let _ = crate::eval::function::call_value(
        crate::value::Value::Function(crate::value::ValueFunction::new_arrow(
            params,
            Box::new(crate::ast::ArrowBody::Block(std::rc::Rc::new(body))),
            closure,
        )),
        vec![],
    );
    if let Some(thrown) = crate::value::take_thrown_value() {
        return Some(JsError(crate::value::to_js_string(&thrown)));
    }
    None
}

/// Assign to an object destructuring pattern.
pub fn assign_object_destructuring(
    props: &[(PropertyKey, BindingElement)],
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let obj = match value {
        Value::Object(o) => o.clone(),
        _ => return Err(JsError("Cannot destructure non-object value".to_string())),
    };
    for (key, binding) in props {
        if let BindingElement::AssignmentTarget(target) = binding {
            let key_str = compute_property_key(key, env)?;
            let prop_value = {
                let obj_ref = obj.borrow();
                obj_ref.get(&key_str).unwrap_or(Value::Undefined)
            };
            crate::eval::object::assign_to(target, &prop_value, env)?;
        } else {
            let key_str = extract_destructure_key(key, env)?;
            let prop_value = {
                let obj_ref = obj.borrow();
                obj_ref.get(&key_str).unwrap_or(Value::Undefined)
            };
            assign_binding_elem(binding, &prop_value, env)?;
        }
    }
    Ok(())
}

/// Compute the string key for a property key.
pub fn compute_property_key(
    key: &PropertyKey,
    env: &Rc<RefCell<Environment>>,
) -> Result<String, JsError> {
    match key {
        PropertyKey::Ident(s) => Ok(s.clone()),
        PropertyKey::String(s) => Ok(s.clone()),
        PropertyKey::Number(n) => Ok(n.to_string()),
        PropertyKey::Computed(expr) => {
            let value = eval_expression(expr, env, false)?;
            Ok(crate::value::to_js_string(&value))
        }
    }
}

/// Extract string key from a destructure property key.
pub fn extract_destructure_key(
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

/// Assign to a single binding element (identifier, pattern, or default).
pub fn assign_binding_elem(
    binding: &BindingElement,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    match binding {
        BindingElement::Identifier(name) if name == "__hole" => Ok(()),
        BindingElement::Identifier(name) => assign_to_identifier(name, value, env),
        BindingElement::ArrayPattern(bindings) => assign_array_destructuring(bindings, value, env),
        BindingElement::ObjectPattern(props) => assign_object_destructuring(props, value, env),
        BindingElement::Default(binding, default) => {
            let value = if matches!(value, Value::Undefined) {
                eval_expression(default, env, false)?
            } else {
                value.clone()
            };
            assign_binding_elem(binding, &value, env)
        }
        BindingElement::AssignmentTarget(target) => {
            if let Expression::Member {
                object, property, ..
            } = target
            {
                let lref_obj = eval_expression(object, env, false)?;
                let key_string = match property {
                    PropertyKey::Computed(expr) => {
                        let key_value = eval_expression(expr, env, false)?;
                        crate::value::to_js_string(&key_value)
                    }
                    PropertyKey::Ident(name) => name.clone(),
                    PropertyKey::String(s) => s.clone(),
                    PropertyKey::Number(n) => n.to_string(),
                };
                if let Value::Object(o) = lref_obj {
                    if let Some(setter) = o.borrow().get_setter(&key_string) {
                        crate::eval::object::call_setter(&o, setter, value.clone(), env)?;
                    } else {
                        o.borrow_mut().set(&key_string, value.clone());
                    }
                } else {
                    return Err(JsError(
                        "Cannot assign to property of non-object".to_string(),
                    ));
                }
                Ok(())
            } else {
                crate::eval::object::assign_to(target, value, env)
            }
        }
    }
}

/// Assign a value to an identifier (variable reference).
pub fn assign_to_identifier(
    name: &str,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let value = match value {
        Value::Function(ref f) if f.name.is_none() => {
            let mut cloned = f.clone();
            cloned.name = Some(name.to_string());
            let _ = cloned.set_property("name", Value::String(name.to_string()));
            Value::Function(cloned)
        }
        Value::Class(ref c) => {
            let has_name = c.name.is_some()
                || c.static_methods.iter().any(|(k, _, _, _, _)| match k {
                    crate::ast::PropertyKey::Ident(s) | crate::ast::PropertyKey::String(s) => {
                        s == "name"
                    }
                    _ => false,
                });
            if !has_name {
                let mut cloned = c.as_ref().clone();
                cloned.name = Some(name.to_string());
                Value::Class(Box::new(cloned))
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
        if crate::interpreter::is_strict_mode() {
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                &format!("{} is not defined", name),
                "ReferenceError",
            );
            return Err(js_err);
        }
        let use_global_this = matches!(env.borrow().get("globalThis"), Some(Value::Object(_)));
        if use_global_this {
            if let Some(Value::Object(global_obj)) = env.borrow().get("globalThis") {
                global_obj.borrow_mut().set(name, value);
            }
        } else {
            env.borrow_mut().define(name.to_string(), value);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::test262::host::Test262Host;
    use crate::Context;
    use crate::Value;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── box_primitive_for_set: Number ────────────────────────────────────────

    #[test]
    fn box_primitive_number() {
        let r = eval("var n = Object(5); n.valueOf()").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn box_primitive_boolean() {
        let r = eval("var b = Object(true); b.valueOf()").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── generator destructuring ─────────────────────────────────────────────

    #[test]
    fn assign_array_destructuring_generator_elision() {
        use crate::ast::BindingElement;
        use crate::eval::object::helpers::destructuring::assign_array_destructuring;

        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var first = 0, second = 0; \
             function* g() { first += 1; yield; second += 1; }",
        )
        .unwrap();
        let gen = ctx.eval("g()").unwrap();
        let env = Rc::clone(ctx.env());
        let bindings = vec![BindingElement::Identifier("__hole".into())];
        assign_array_destructuring(&bindings, &gen, &env).unwrap();
        assert_eq!(ctx.eval("first").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn bind_params_destructures_generator_elision() {
        use crate::ast::{BindingElement, Param};
        use crate::env::Environment;
        use crate::eval::function::bind_params;
        use crate::value::ValueFunction;

        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        ctx.eval(
            "var first = 0, second = 0; \
             function* g() { first += 1; yield; second += 1; }",
        )
        .unwrap();
        let gen = ctx.eval("g()").unwrap();
        let params = vec![Param {
            name: "arg".to_string(),
            default: None,
            pattern: Some(BindingElement::ArrayPattern(vec![
                BindingElement::Identifier("__hole".into()),
            ])),
            rest: false,
        }];
        let env = Rc::clone(ctx.env());
        let f = ValueFunction::new(None, params.clone(), vec![], Rc::clone(&env), false, false);
        let call_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&env))));
        bind_params(&f, &params, std::slice::from_ref(&gen), &call_env, false).unwrap();
        assert_eq!(ctx.eval("first").unwrap(), Value::Number(1.0));
        assert_eq!(ctx.eval("second").unwrap(), Value::Number(0.0));
    }

    #[test]
    fn destructure_generator_elision_advances_iterator() {
        let mut host = crate::test262::QuenchHost::new();
        host.run_script(
            "var first = 0, second = 0; \
             function* g() { first += 1; yield; second += 1; } \
             class C { method([,]) {} } \
             new C().method(g()); \
             if (first !== 1 || second !== 0) throw new Error('got ' + first + ',' + second);",
        )
        .expect("class method generator destructuring");
    }

    #[test]
    fn destructure_generator_elision_iife() {
        let r = eval(
            "var first = 0, second = 0; \
             function* g() { first += 1; yield; second += 1; } \
             (function([,]) {})(g()); \
             first + second * 10",
        )
        .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    // ─── array destructuring ─────────────────────────────────────────────────

    #[test]
    fn array_destructuring_basic() {
        let r = eval("var [a, b] = [1, 2]; a + b").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn array_destructuring_spread() {
        let r = eval("var [first, ...rest] = [1, 2, 3]; rest[0] + rest[1]").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn array_destructuring_skip() {
        let r = eval("var [, second] = [10, 20]; second").unwrap();
        assert_eq!(r, Value::Number(20.0));
    }

    #[test]
    fn array_destructuring_default() {
        let r = eval("var [a = 1] = []; a").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn array_destructuring_nested() {
        let r = eval("var [[inner]] = [[42]]; inner").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    // ─── object destructuring ────────────────────────────────────────────────

    #[test]
    fn object_destructuring_basic() {
        let r = eval("var {x, y} = {x: 1, y: 2}; x + y").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn object_destructuring_rename() {
        let r = eval("var {x: alias} = {x: 99}; alias").unwrap();
        assert_eq!(r, Value::Number(99.0));
    }

    #[test]
    fn object_destructuring_default() {
        let r = eval("var {missing = 5} = {}; missing").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn object_destructuring_nested() {
        let r = eval("var {outer: {inner}} = {outer: {inner: 7}}; inner").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn object_destructuring_rest() {
        let r = eval("var {a, ...rest} = {a: 1, b: 2, c: 3}; rest.b + rest.c").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    // ─── compute_property_key ────────────────────────────────────────────────

    #[test]
    fn destructuring_string_key() {
        let r = eval("var {'foo': x} = {'foo': 42}; x").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    // ─── assign_binding_elem: identifier assignment ───────────────────────────

    #[test]
    fn binding_elem_identifier_const() {
        let r = eval("const x = 5; x").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn binding_elem_identifier_let() {
        let r = eval("let y = 10; y").unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    // ─── assign_to_identifier: const assignment throws ─────────────────────

    #[test]
    fn assign_to_const_throws() {
        let r = eval("const x = 1; x = 2");
        assert!(r.is_err());
    }

    #[test]
    fn assign_to_undeclared_strict_throws() {
        let r = eval("'use strict'; z = 1");
        assert!(r.is_err());
    }

    // ─── string is iterable for destructuring ────────────────────────────────

    #[test]
    fn string_is_iterable_for_destructuring() {
        let r = eval("var [a, b, c] = 'xyz'; a + b + c").unwrap();
        assert_eq!(r, Value::String("xyz".into()));
    }

    // ─── assign_array_with_iterator: excess bindings ────────────────────────

    #[test]
    fn array_destructuring_fewer_values() {
        let r = eval("var [a, b, c] = [1]; b").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn array_destructuring_more_values() {
        let r = eval("var [a] = [1, 2, 3]; a").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }
}
