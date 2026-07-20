//! Literal expression evaluation
//!
//! Handles evaluation of literal expressions: numbers, strings, booleans,
//! null, undefined, identifiers, object/array literals, RegExp literals,
//! and function/arrow function expressions.

/// Check if `name` resolves to the global `eval` function (direct eval).
/// Returns true only if the identifier `name` resolves to the actual built-in
/// `eval` function — not a local alias like `var my_eval = eval`.
/// For "eval": walks the environment chain and resolves the identifier to get
/// its value. Returns true only if the value is a native function named "eval".
/// For other names: returns false (never direct eval).
pub fn is_global_eval(name: &str, env: &Rc<RefCell<Environment>>) -> bool {
    if name != "eval" {
        return false;
    }
    // Walk environment chain to find the binding and resolve it
    let mut current: Option<Rc<RefCell<Environment>>> = Some(Rc::clone(env));
    while let Some(e) = current {
        if let Some(val) = e.borrow().get(name) {
            // Found the binding. Check if the VALUE is the global eval function.
            // The global eval is a NativeFunction named "eval". Local aliases
            // (var my_eval = eval) have the same value but are indirect eval.
            if let Value::NativeFunction(nf) = val {
                // The global eval function has name "eval". Local aliases
                // (var my_eval = eval) have the same value but are indirect eval.
                return nf.name == "eval";
            }
            return false;
        }
        current = e.borrow().get_parent();
    }
    false
}

use crate::ast::*;
use crate::builtins;
use crate::env::Environment;
use crate::eval::iteration::get_iterator;
use crate::value::error::create_js_error_with_type;
use crate::value::{to_js_string, JsError, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate an identifier expression
pub fn eval_identifier(
    name: &str,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    if name == "this" {
        return Ok(crate::interpreter::get_this_binding(env));
    }
    if name == "super" {
        return eval_super(env);
    }
    if name == "new.target" {
        // Per ES §13.2.6 GetNewTarget: arrow functions inherit new.target
        // via lexical scope (the enclosing function's env binding). For
        // ordinary functions, call_js_function_impl binds new.target in
        // the call env, so env.get resolves it correctly here.
        return Ok(env.borrow().get(name).unwrap_or(Value::Undefined));
    }
    // Arrow functions don't have their own 'arguments' binding
    if in_arrow_function && name == "arguments" {
        // Check if arguments exists in enclosing scope (arrow can access enclosing arguments)
        let found = env.borrow().get("arguments");
        if found.is_none() {
            let (_, js_err) = create_js_error_with_type(
                &format!("ReferenceError: {} is not defined", name),
                "ReferenceError",
            );
            return Err(js_err);
        }
        // Arrow can access enclosing arguments - fall through to normal lookup
    }
    if env.borrow().is_tdz(name) {
        let (_, js_err) = create_js_error_with_type(
            &format!(
                "ReferenceError: Cannot access '{}' before initialization",
                name
            ),
            "ReferenceError",
        );
        return Err(js_err);
    }

    // Use get() which handles DeclaredOnly (hoisted var) → undefined
    // and truly unknown vars → None (caught below as ReferenceError).
    match env.borrow().get(name) {
        Some(v) => Ok(v),
        None => {
            // Fallback: try to get from Context's globals directly.
            // This handles cases where the environment chain doesn't have access
            // to globalThis (e.g., super constructor calls with isolated environments).
            if let Some(global_val) = crate::context::get_global_from_context(name) {
                return Ok(global_val);
            }
            let (_, js_err) =
                create_js_error_with_type(&format!("{} is not defined", name), "ReferenceError");
            Err(js_err)
        }
    }
}

/// Get the super class value from the environment chain
fn get_super_from_env(env: &Rc<RefCell<Environment>>) -> Option<Value> {
    let mut current = Some(env.clone());
    while let Some(e) = current {
        if let Some(super_class) = e.borrow().get_super_class() {
            return Some(super_class);
        }
        current = e.borrow().get_parent();
    }
    None
}

/// Evaluate super keyword
fn eval_super(env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    get_super_from_env(env)
        .ok_or_else(|| JsError("ReferenceError: super is only valid in class methods".to_string()))
}

/// Evaluate a RegExp literal
pub fn eval_regexp_literal(pattern: &str, flags: &str) -> Result<Value, JsError> {
    use crate::value::PropertyFlags;
    use regress::Regex;
    // ES 11.8.5: regex literals must not contain line terminators
    if pattern.contains('\n')
        || pattern.contains('\r')
        || pattern.contains('\u{2028}')
        || pattern.contains('\u{2029}')
    {
        let (err_val, js_err) = crate::value::error::create_js_error_with_type(
            "Invalid regular expression: unexpected line terminator",
            "SyntaxError",
        );
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    let regress_flags: String = flags.chars().filter(|c| "imsu".contains(*c)).collect();
    let regex = Regex::with_flags(pattern, regress_flags.as_str())
        .map_err(|_| JsError::new("Invalid regular expression"))?;
    let mut obj = Object::new(ObjectKind::RegExp);
    obj.internal_regex_source = Some(pattern.to_string());
    obj.internal_regex_flags = Some(flags.to_string());
    obj.set("source", Value::String(pattern.to_string()));
    obj.set("global", Value::Boolean(flags.contains('g')));
    obj.set("ignoreCase", Value::Boolean(flags.contains('i')));
    obj.set("multiline", Value::Boolean(flags.contains('m')));
    // lastIndex must be writable, non-enumerable, non-configurable per spec
    obj.define(
        "lastIndex",
        Value::Number(0.0),
        PropertyFlags {
            value: Some(Value::Number(0.0)),
            writable: true,
            enumerable: false,
            configurable: false,
        },
    );
    obj.set("flags", Value::String(flags.to_string()));
    obj.internal_regex = Some(regex);
    let obj_rc = Rc::new(RefCell::new(obj));
    obj_rc.borrow_mut().prototype = Some(crate::builtins::regex::get_regexp_prototype());
    Ok(Value::Object(obj_rc))
}

/// Evaluate an object literal expression
pub fn eval_object_literal(
    props: &[(PropertyKey, PropertyValue)],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let keys_info: Vec<String> = props
        .iter()
        .map(|(k, v)| format!("{:?}({:?})", k, v))
        .collect();
    let _ = keys_info;
    let mut obj = Object::new(ObjectKind::Ordinary);
    if let Some(prototype) = builtins::get_object_prototype() {
        obj.prototype = Some(prototype);
    }
    for (key, value) in props {
        let key_str = eval_property_key(key, env, in_arrow_function)?;
        match value {
            PropertyValue::Value(expr) => {
                let val = crate::eval::expression::eval_expression(expr, env, in_arrow_function)?;
                obj.set(&key_str, val);
            }
            PropertyValue::Getter { params: _, body } => {
                obj.set_getter(
                    &key_str,
                    Rc::new(body.clone()),
                    crate::eval::expression::capture_env_for_closure(env),
                );
            }
            PropertyValue::Setter { param, body } => {
                obj.set_setter(
                    &key_str,
                    param.clone(),
                    Rc::new(body.clone()),
                    crate::eval::expression::capture_env_for_closure(env),
                );
            }
        }
    }
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// Evaluate a property key (identifier, string, number, or computed)
pub fn eval_property_key(
    key: &PropertyKey,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<String, JsError> {
    match key {
        PropertyKey::Ident(s) => Ok(s.clone()),
        PropertyKey::String(s) => Ok(s.clone()),
        PropertyKey::Number(n) => Ok(n.to_string()),
        PropertyKey::Computed(e) => {
            let val = crate::eval::expression::eval_expression(e, env, in_arrow_function)?;
            match &val {
                Value::Symbol(s) => Ok(s.desc.clone().map(|d| d.to_string()).unwrap_or_default()),
                _ => Ok(to_js_string(&val)),
            }
        }
    }
}

/// Evaluate an array literal expression
pub fn eval_array_literal(
    elements: &[Expression],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let mut arr = Object::new_array(0);
    for elem_expr in elements.iter() {
        match elem_expr {
            Expression::Spread(spread_expr) => {
                let spread_val =
                    crate::eval::expression::eval_expression(spread_expr, env, in_arrow_function)?;
                let items = get_iterator(&spread_val)?;
                for item in items {
                    let idx = arr.elements.len();
                    arr.set(&idx.to_string(), item);
                }
            }
            Expression::Elision => {
                // Array hole: advances length but contributes no own property.
                let idx = arr.elements.len();
                arr.elements.push(Value::Undefined);
                arr.holes.insert(idx);
                arr.properties.insert(
                    "length".to_string(),
                    Value::Number(arr.elements.len() as f64),
                );
            }
            _ => {
                let value =
                    crate::eval::expression::eval_expression(elem_expr, env, in_arrow_function)?;
                let idx = arr.elements.len();
                arr.set(&idx.to_string(), value);
            }
        }
    }
    if let Some(prototype) = builtins::get_array_prototype() {
        arr.prototype = Some(prototype);
    }
    Ok(Value::Object(Rc::new(RefCell::new(arr))))
}

/// Get the super class value from the environment (public for use by expression.rs)
pub fn get_super_value(env: &Rc<RefCell<Environment>>) -> Option<Value> {
    get_super_from_env(env)
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;
    use crate::ast::{Expression, PropertyKey, PropertyValue, Statement, VarKind};
    use crate::value::NativeFunction;
    use std::rc::Rc;
    fn e() -> Rc<RefCell<Environment>> { Rc::new(RefCell::new(Environment::new())) }
    fn nf(n: &str) -> Value { Value::NativeFunction(Rc::new(NativeFunction::new_named(n, |_| Ok(Value::Undefined)))) }

    #[test]
    fn is_global_eval_tests() {
        assert!(!is_global_eval("foo", &e())); assert!(!is_global_eval("eval", &e()));
        let a = e(); a.borrow_mut().declare_var("eval".into(), VarKind::Var); a.borrow_mut().initialize_declared("eval", nf("eval"));
        assert!(is_global_eval("eval", &a));
        let b = e(); b.borrow_mut().declare_var("eval".into(), VarKind::Var); b.borrow_mut().initialize_declared("eval", nf("not_eval"));
        assert!(!is_global_eval("eval", &b));
        let c = e(); c.borrow_mut().declare_var("eval".into(), VarKind::Let); c.borrow_mut().initialize_declared("eval", Value::Number(42.0));
        assert!(!is_global_eval("eval", &c));
        let p = e(); p.borrow_mut().declare_var("eval".into(), VarKind::Var); p.borrow_mut().initialize_declared("eval", nf("eval"));
        let ch = Rc::new(RefCell::new(Environment::with_parent(p)));
        assert!(is_global_eval("eval", &ch));
    }

    #[test]
    fn eval_identifier_tests() {
        let t = e(); t.borrow().current_scope().borrow_mut().set_this(Value::String("o".into()));
        assert_eq!(eval_identifier("this", &t, false).unwrap(), Value::String("o".into()));
        let err = eval_identifier("super", &e(), false).unwrap_err();
        assert!(err.0.contains("super is only valid"), "got: {0:?}", err.0);
        let s = e(); s.borrow_mut().set_super_class(Value::String("B".into()));
        assert_eq!(eval_identifier("super", &s, false).unwrap(), Value::String("B".into()));
        assert_eq!(eval_identifier("new.target", &e(), false).unwrap(), Value::Undefined);
        let nt = e(); nt.borrow_mut().declare_var("new.target".into(), VarKind::Let);
        nt.borrow_mut().initialize_declared("new.target", Value::String("c".into()));
        assert_eq!(eval_identifier("new.target", &nt, false).unwrap(), Value::String("c".into()));
        let er = eval_identifier("arguments", &e(), true).unwrap_err();
        assert!(er.0.contains("is not defined"), "got: {0:?}", er.0);
        let ar = e(); ar.borrow_mut().declare_var("arguments".into(), VarKind::Let);
        ar.borrow_mut().initialize_declared("arguments", Value::String("a".into()));
        assert_eq!(eval_identifier("arguments", &ar, true).unwrap(), Value::String("a".into()));
        let td = e(); td.borrow_mut().declare_var("x".into(), VarKind::Let);
        assert!(eval_identifier("x", &td, false).unwrap_err().0.contains("Cannot access"));
        let nl = e(); nl.borrow_mut().declare_var("x".into(), VarKind::Let);
        nl.borrow_mut().initialize_declared("x", Value::Number(42.0));
        assert_eq!(eval_identifier("x", &nl, false).unwrap(), Value::Number(42.0));
        let uk = eval_identifier("uk", &e(), false).unwrap_err();
        assert!(uk.0.contains("is not defined"), "got: {0:?}", uk.0);
    }

    #[test]
    fn eval_regexp_literal_tests() {
        let r = eval_regexp_literal("abc", "").unwrap();
        let Value::Object(ref o) = r else { panic!("not obj") };
        let ob = o.borrow();
        assert_eq!(ob.get("source"), Some(Value::String("abc".into())));
        assert_eq!(ob.get("lastIndex"), Some(Value::Number(0.0)));
        assert_eq!(ob.get("flags"), Some(Value::String("".into())));
        assert_eq!(ob.internal_regex_source, Some("abc".into())); assert!(ob.internal_regex.is_some());
        drop(ob);
        let r2 = eval_regexp_literal("foo", "gim").unwrap();
        let Value::Object(ref o2) = r2 else { panic!("not obj") };
        let o2b = o2.borrow();
        assert_eq!(o2b.get("global"), Some(Value::Boolean(true)));
        assert_eq!(o2b.get("ignoreCase"), Some(Value::Boolean(true)));
        assert_eq!(o2b.get("multiline"), Some(Value::Boolean(true)));
        assert_eq!(o2b.internal_regex_flags, Some("gim".into()));
        drop(o2b);
        let r3 = eval_regexp_literal("", "").unwrap();
        let Value::Object(ref o3) = r3 else { panic!("not obj") };
        assert_eq!(o3.borrow().get("source"), Some(Value::String("".into())));
        let r4 = eval_regexp_literal(r"\d+", "i").unwrap();
        let Value::Object(ref o4) = r4 else { panic!("not obj") };
        assert_eq!(o4.borrow().get("ignoreCase"), Some(Value::Boolean(true)));
        let e1 = eval_regexp_literal(r"[invalid", "").unwrap_err();
        assert!(e1.0.contains("Invalid"), "got: {0:?}", e1.0);
        let e2 = eval_regexp_literal("a\nb", "").unwrap_err();
        assert!(e2.0.contains("line terminator"), "got: {0:?}", e2.0);
        let e3 = eval_regexp_literal("a\rb", "").unwrap_err();
        assert!(e3.0.contains("line terminator"), "got: {0:?}", e3.0);
    }

    #[test]
    fn eval_property_key_tests() {
        let en = e();
        assert_eq!(eval_property_key(&PropertyKey::Ident("f".into()), &en, false).unwrap(), "f");
        assert_eq!(eval_property_key(&PropertyKey::String("b".into()), &en, false).unwrap(), "b");
        assert_eq!(eval_property_key(&PropertyKey::Number(42.0), &en, false).unwrap(), "42");
        assert_eq!(eval_property_key(&PropertyKey::Computed(Box::new(Expression::Number(7.0))), &en, false).unwrap(), "7");
        assert_eq!(eval_property_key(&PropertyKey::Computed(Box::new(Expression::String("k".into()))), &en, false).unwrap(), "k");
        let sym = Value::Symbol(Rc::new(crate::value::Symbol { desc: Some("s".into()), global: false }));
        en.borrow_mut().declare_var("sym".into(), VarKind::Let);
        en.borrow_mut().initialize_declared("sym", sym);
        assert_eq!(eval_property_key(&PropertyKey::Computed(Box::new(Expression::Identifier("sym".into()))), &en, false).unwrap(), "s");
    }

    #[test]
    fn eval_object_literal_tests() {
        let en = e();
        let Value::Object(ref o1) = eval_object_literal(&[], &en, false).unwrap() else { panic!("not obj") };
        assert_eq!(o1.borrow().kind, ObjectKind::Ordinary);
        let mut ctx = crate::Context::new().unwrap(); crate::builtins::register_builtins(&mut ctx);
        let props = vec![(PropertyKey::Ident("a".into()), PropertyValue::Value(Expression::Number(1.0))), (PropertyKey::Ident("b".into()), PropertyValue::Value(Expression::Boolean(true)))];
        let Value::Object(ref o2) = eval_object_literal(&props, &en, false).unwrap() else { panic!("not obj") };
        assert_eq!(o2.borrow().get("a"), Some(Value::Number(1.0))); assert_eq!(o2.borrow().get("b"), Some(Value::Boolean(true)));
        let gb = vec![Statement::Return(Some(Box::new(Expression::Number(42.0))))];
        let sb = vec![Statement::Expression(Box::new(Expression::Identifier("v".into())))];
        let gsp = vec![(PropertyKey::Ident("x".into()), PropertyValue::Getter { params: vec![], body: gb }), (PropertyKey::Ident("y".into()), PropertyValue::Setter { param: "val".into(), body: sb })];
        let Value::Object(ref o3) = eval_object_literal(&gsp, &en, false).unwrap() else { panic!("not obj") };
        let d = o3.borrow().get_own_property("x").unwrap(); assert!(d.get_body.is_some());
        let d2 = o3.borrow().get_own_property("y").unwrap(); assert!(d2.set_body.is_some()); assert_eq!(d2.set_param.as_deref(), Some("val"));
        let ck = PropertyKey::Computed(Box::new(Expression::String("dyn".into())));
        let Value::Object(ref o4) = eval_object_literal(&[(ck, PropertyValue::Value(Expression::Number(99.0)))], &en, false).unwrap() else { panic!("not obj") };
        assert_eq!(o4.borrow().get("dyn"), Some(Value::Number(99.0)));
    }

    #[test]
    fn eval_array_literal_tests() {
        let mut ctx = crate::Context::new().unwrap(); crate::builtins::register_builtins(&mut ctx);
        let en = e();
        let Value::Object(ref a1) = eval_array_literal(&[], &en, false).unwrap() else { panic!("not arr") };
        assert_eq!(a1.borrow().kind, ObjectKind::Array); assert_eq!(a1.borrow().get("length"), Some(Value::Number(0.0)));
        let els = vec![Expression::Number(1.0), Expression::String("s".into()), Expression::Null, Expression::Boolean(true)];
        let Value::Object(ref a2) = eval_array_literal(&els, &en, false).unwrap() else { panic!("not arr") };
        let a2b = a2.borrow();
        assert_eq!(a2b.get("0"), Some(Value::Number(1.0))); assert_eq!(a2b.get("1"), Some(Value::String("s".into())));
        assert_eq!(a2b.get("2"), Some(Value::Null)); assert_eq!(a2b.get("3"), Some(Value::Boolean(true))); assert_eq!(a2b.get("length"), Some(Value::Number(4.0)));
        drop(a2b);
        let els2 = vec![Expression::Number(1.0), Expression::Elision, Expression::Number(3.0)];
        let Value::Object(ref a3) = eval_array_literal(&els2, &en, false).unwrap() else { panic!("not arr") };
        let a3b = a3.borrow();
        assert_eq!(a3b.get("0"), Some(Value::Number(1.0))); assert!(!a3b.has("1"));
        assert_eq!(a3b.get("2"), Some(Value::Number(3.0))); assert_eq!(a3b.get("length"), Some(Value::Number(3.0))); assert!(a3b.holes.contains(&1));
        drop(a3b);
        let src = ctx.eval("[10, 20]").unwrap();
        en.borrow_mut().declare_var("src".into(), VarKind::Let); en.borrow_mut().initialize_declared("src", src);
        let els3 = vec![Expression::Number(1.0), Expression::Spread(Box::new(Expression::Identifier("src".into()))), Expression::Number(30.0)];
        let Value::Object(ref a4) = eval_array_literal(&els3, &en, false).unwrap() else { panic!("not arr") };
        let a4b = a4.borrow();
        assert_eq!(a4b.get("0"), Some(Value::Number(1.0))); assert_eq!(a4b.get("1"), Some(Value::Number(10.0)));
        assert_eq!(a4b.get("2"), Some(Value::Number(20.0))); assert_eq!(a4b.get("3"), Some(Value::Number(30.0))); assert_eq!(a4b.get("length"), Some(Value::Number(4.0)));
    }

    #[test]
    fn get_super_value_tests() {
        let en = e(); assert!(get_super_value(&en).is_none());
        en.borrow_mut().set_super_class(Value::Number(1.0));
        assert_eq!(get_super_value(&en), Some(Value::Number(1.0)));
        let p = e(); p.borrow_mut().set_super_class(Value::String("B".into()));
        let c = Rc::new(RefCell::new(Environment::with_parent(p)));
        assert_eq!(get_super_value(&c), Some(Value::String("B".into())));
    }
}
