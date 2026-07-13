//! Class expression evaluation
//!
//! This module handles class instantiation, prototype creation,
//! and class expression evaluation.

use crate::ast::{Class, Expression, Param, Statement, VarKind};
use crate::builtins;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::statement::eval_function_body;
use crate::interpreter::{check_depth_guard, predeclare_let_const};
use crate::value::{ClassValue, JsError, Object, ObjectKind, Value, ValueFunction};
use std::cell::RefCell;
use std::rc::Rc;

    fn class_static_field_this_name() {
        let _ = 42;
    }

/// Evaluate a class expression. The `inferred_name` parameter provides the
/// inferred class name per ES §14.6.13 step 18 when the class is anonymous
/// and the surrounding context supplies the name (e.g. via assignment to an
/// identifier or as a variable initializer).
pub fn eval_class_expr(
    class: &Class,
    env: &Rc<RefCell<Environment>>,
    inferred_name: Option<&str>,
) -> Result<Value, JsError> {
    let mut new_value = ClassValue::from_ast(class);
    if let Some(name) = inferred_name {
        new_value.set_name(name);
    }

    let class_name = class.name.as_deref().or(inferred_name);

    // Per ES §14.6.13 step 18: for a named class expression, create a new
    // lexical scope with the class name bound as immutable const so methods
    // and heritage can reference the class, and reassignment throws TypeError.
    let class_scope = if let Some(name) = class_name {
        let scope_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(env))));
        // First declare as const (creates TDZ + tracks var_kind)
        scope_env
            .borrow_mut()
            .current_scope()
            .borrow_mut()
            .declare_var(name.to_string(), VarKind::Const);
        // Then initialize with the class value
        let class_val = Value::Class(new_value.clone());
        scope_env
            .borrow_mut()
            .current_scope()
            .borrow_mut()
            .initialize_declared(name, class_val);
        scope_env
    } else {
        Rc::clone(env)
    };

    // Eagerly create the prototype for this class (uses class_scope so
    // methods capture the class name binding).
    let _ = get_or_create_class_prototype(&new_value, &class_scope)?;

    // Per ES §14.6.13 and §9.2.10, static fields with computed name
    // "prototype" or "constructor" must throw a TypeError.
    let class_value = Value::Class(new_value.clone());
    for (name, value_expr) in &new_value.static_fields {
        let child_env: Rc<RefCell<Environment>> =
            Rc::new(RefCell::new(Environment::with_parent(Rc::clone(env))));
        child_env
            .borrow_mut()
            .current_scope()
            .borrow_mut()
            .set_this(class_value.clone());
        let field_value = eval_expression(value_expr, &child_env, false)?;
        let key_str = prop_key_to_string(name, &child_env, true)?;
        if key_str == "prototype" || key_str == "constructor" {
            return Err(JsError(format!(
                "TypeError: static class field may not be named '{}'",
                key_str
            )));
        }
        new_value.set_static_field(&key_str, field_value);
    }

    Ok(Value::Class(new_value))
}

fn infer_class_name_from_env(_env: &Rc<RefCell<Environment>>) -> Option<String> {
    None
}

/// Instantiate a class from its AST representation
pub fn instantiate_class_from_ast_with_env(
    class: ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Fast path: no instance fields → use simpler logic
    if class.instance_fields.is_empty() {
        return instantiate_simple(&class, args, env);
    }
    instantiate_with_fields(&class, args, env)
}

/// Instantiate without instance fields (fast path)
fn instantiate_simple(
    class: &ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let _depth = check_depth_guard()?;
    let proto_rc = get_or_create_class_prototype(class, env)?;

    let mut instance = Object::new(ObjectKind::Ordinary);
    instance.prototype = Some(Rc::clone(&proto_rc));
    let instance_rc = Rc::new(RefCell::new(instance));

    let this_val = Value::Object(Rc::clone(&instance_rc));
    // Per ES spec §10.1.3, instance.constructor === the class
    instance_rc
        .borrow_mut()
        .set("constructor", Value::Class(class.clone()));

    let _params = class.constructor_params.clone();
    let body = class.constructor_body.clone();

    let call_env = build_constructor_env(class, &args, &this_val, env)?;
    let call_env = Rc::new(RefCell::new(call_env));

    if body.is_empty() {
        if let Some(ref sc) = class.super_class {
            let sv = eval_expression(sc, env, false)?;
            call_super_or_default(&sv, args, &this_val, env)?;
        }
        Ok(this_val)
    } else {
        let first_is_super = check_first_is_super_call(&body);
        let body_calls_super = first_is_super || body_calls_super_call(&body);
        if body_calls_super {
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            let result = eval_function_body(&body, &call_env, false)?;
            finish_constructor(result, &this_val)
        } else {
            if let Some(ref sc) = class.super_class {
                let sv = eval_expression(sc, env, false)?;
                call_super_or_default(&sv, args, &this_val, env)?;
            }
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            let result = eval_function_body(&body, &call_env, false)?;
            finish_constructor(result, &this_val)
        }
    }
}

/// Instantiate with instance fields: fields init after super(), before body
fn instantiate_with_fields(
    class: &ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let _depth = check_depth_guard()?;
    let proto_rc = get_or_create_class_prototype(class, env)?;

    let mut instance = Object::new(ObjectKind::Ordinary);
    instance.prototype = Some(Rc::clone(&proto_rc));
    let instance_rc = Rc::new(RefCell::new(instance));

    let this_val = Value::Object(Rc::clone(&instance_rc));
    // Per ES spec §10.1.3, instance.constructor === the class
    instance_rc
        .borrow_mut()
        .set("constructor", Value::Class(class.clone()));

    let body = class.constructor_body.clone();

    let call_env = build_constructor_env(class, &args, &this_val, env)?;
    let call_env = Rc::new(RefCell::new(call_env));

    let has_super = class.super_class.is_some();
    let body_calls_super = !body.is_empty() && body_calls_super_call(&body);

    // Helper: evaluate all instance fields and set on `this`
    let init_fields = || -> Result<(), JsError> {
        for (name, value_expr) in &class.instance_fields {
            let field_val = eval_expression(value_expr, &call_env, false)?;
            let key_str = prop_key_to_string(name, &call_env, false)?;
            instance_rc.borrow_mut().set(&key_str, field_val);
        }
        Ok(())
    };

    if has_super {
        if body_calls_super {
            // Body has its own super() call — set pending_fields so
            // eval_super_call initializes fields after super() returns.
            call_env.borrow_mut().set_pending_fields(class.instance_fields.clone());
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            eval_function_body(&body, &call_env, false)?;
        } else if body.is_empty() {
            // Empty derived constructor — auto-call super, then init fields
            let sv = eval_expression(class.super_class.as_ref().unwrap(), env, false)?;
            call_super_or_default(&sv, args, &this_val, env)?;
            init_fields()?;
        } else {
            // Derived, non-empty body without super() — auto-call super,
            // init fields, then evaluate body
            let sv = eval_expression(class.super_class.as_ref().unwrap(), env, false)?;
            call_super_or_default(&sv, args.clone(), &this_val, env)?;
            init_fields()?;
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            eval_function_body(&body, &call_env, false)?;
        }
    } else {
        // Non-derived class — fields init before constructor body
        init_fields()?;
        if !body.is_empty() {
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            eval_function_body(&body, &call_env, false)?;
        }
    }

    Ok(this_val)
}

/// Build constructor environment (params, arguments, super reference)
fn build_constructor_env(
    class: &ClassValue,
    args: &[Value],
    this_val: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Environment, JsError> {
    let mut call_env = Environment::with_parent(Rc::clone(env));
    // Bind `this` WITHOUT marking it as initialized. The this-initialized
    // flag is set after super() succeeds, so that a second super() inside
    // the same constructor (or inside an arrow) throws ReferenceError per
    // ES §8.1.1.3.1.
    call_env
        .current_scope()
        .borrow_mut()
        .set_this_value(this_val.clone());

    if let Some(ref sc) = class.super_class {
        let sv = eval_expression(sc, env, false)?;
        call_env.set_super_class(sv);
    }

    for (i, param) in class.constructor_params.iter().enumerate() {
        let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
        call_env.define(param.clone(), arg);
    }

    let args_obj = create_arguments_object_simple(args.to_vec());
    call_env.define("arguments".to_string(), args_obj);

    Ok(call_env)
}

/// Handle constructor return value (constructors return `this` by default)
fn finish_constructor(result: Value, this_val: &Value) -> Result<Value, JsError> {
    match result {
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_) => Ok(result),
        _ => Ok(this_val.clone()),
    }
}

/// Call the super constructor or use default behavior
fn call_super_or_default(
    super_val: &Value,
    args: Vec<Value>,
    this_val: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    match super_val {
        Value::Class(super_class) => {
            // Use call_super_constructor to ensure 'this' is properly bound
            call_super_constructor(super_class.clone(), args, this_val.clone(), env)?;
        }
        Value::Object(o) => {
            if let Some(Value::Function(constructor)) = o.borrow().get("constructor") {
                crate::eval::function::call_value_with_this(
                    Value::Function(constructor.clone()),
                    args,
                    this_val.clone(),
                )?;
            } else if let Some(Value::NativeConstructor(nc)) = o.borrow().get("constructor") {
                crate::eval::function::call_value_with_this(
                    Value::NativeConstructor(nc.clone()),
                    args,
                    this_val.clone(),
                )?;
            }
        }
        Value::NativeConstructor(nc) => {
            // For native constructors, call with 'this' binding
            crate::eval::function::call_value_with_this(
                Value::NativeConstructor(nc.clone()),
                args,
                this_val.clone(),
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// Instantiate a class from its AST representation (legacy signature)
pub fn instantiate_class_from_ast(class: ClassValue, args: Vec<Value>) -> Result<Value, JsError> {
    // Use the current context's global environment so that global variable modifications
    // in the constructor persist after instantiation.
    let env = crate::context::get_current_env()
        .unwrap_or_else(|| Rc::new(RefCell::new(Environment::new())));
    instantiate_class_from_ast_with_env(class, args, &env)
}

/// Call a super constructor with the given arguments and 'this' binding
pub fn call_super_constructor(
    class: ClassValue,
    args: Vec<Value>,
    this_val: Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let _proto_rc = get_or_create_class_prototype(&class, env)?;

    let _params = class.constructor_params.clone();
    let body = class.constructor_body.clone();

    let mut call_env = Environment::with_parent(Rc::clone(env));
    call_env
        .current_scope()
        .borrow_mut()
        .set_this(this_val.clone());

    for (i, param) in _params.iter().enumerate() {
        let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
        call_env.define(param.clone(), arg);
    }

    let args_obj = create_arguments_object_simple(args);
    call_env.define("arguments".to_string(), args_obj);

    let call_env = Rc::new(RefCell::new(call_env));

    if body.is_empty() {
        // Empty constructor - just return this
        Ok(this_val)
    } else {
        // Evaluate constructor body
        predeclare_let_const(&body, &mut call_env.borrow_mut());
        let result = eval_function_body(&body, &call_env, false)?;
        match result {
            Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_) => Ok(result),
            _ => Ok(this_val),
        }
    }
}

/// Check if the first statement in a constructor body is an explicit super() call
fn check_first_is_super_call(body: &[Statement]) -> bool {
    if let Some(Statement::Expression(expr)) = body.first() {
        if let Expression::Call { callee, .. } = expr.as_ref() {
            if let Expression::Identifier(id) = callee.as_ref() {
                return id == "super";
            }
        }
    }
    false
}

/// Check if the constructor body contains a super() call anywhere (not just as
/// the first statement). Per ES §14.5.14 step 14, derived constructors must
/// call super() — anywhere in the body counts, not just as the first stmt.
/// If the body already calls super(), we MUST NOT auto-call it from the
/// runtime or A() would be invoked twice.
fn body_calls_super_call(body: &[Statement]) -> bool {
    body.iter().any(stmt_contains_super_call)
}

fn stmt_contains_super_call(stmt: &Statement) -> bool {
    match stmt {
        Statement::Expression(expr) => expr_contains_super_call(expr),
        Statement::Block(stmts) => stmts.iter().any(stmt_contains_super_call),
        Statement::If {
            condition,
            consequent,
            alternate,
        } => {
            expr_contains_super_call(condition)
                || stmt_contains_super_call(consequent)
                || alternate
                    .as_ref()
                    .map(|a| stmt_contains_super_call(a))
                    .unwrap_or(false)
        }
        Statement::While { body, .. }
        | Statement::For { body, .. }
        | Statement::ForIn { body, .. } => stmt_contains_super_call(body),
        Statement::TryCatch { body, handler, .. } => {
            stmt_contains_super_call(body) || stmt_contains_super_call(handler)
        }
        Statement::Return(Some(expr)) => expr_contains_super_call(expr),
        _ => false,
    }
}

fn expr_contains_super_call(expr: &Expression) -> bool {
    match expr {
        Expression::Identifier(id) => id == "super",
        Expression::Call { callee, .. } => expr_contains_super_call(callee),
        Expression::Member { object, .. } => expr_contains_super_call(object),
        Expression::ArrowFunction { body, .. } => match body.as_ref() {
            crate::ast::ArrowBody::Expression(e) => expr_contains_super_call(e),
            crate::ast::ArrowBody::Block(stmts) => stmts.iter().any(stmt_contains_super_call),
        },
        Expression::Assignment { left, right, .. } => {
            expr_contains_super_call(left) || expr_contains_super_call(right)
        }
        Expression::Binary { left, right, .. } => {
            expr_contains_super_call(left) || expr_contains_super_call(right)
        }
        Expression::Unary { argument, .. } => expr_contains_super_call(argument),
        Expression::Conditional {
            condition,
            consequent,
            alternate,
        } => {
            expr_contains_super_call(condition)
                || expr_contains_super_call(consequent)
                || expr_contains_super_call(alternate)
        }
        _ => false,
    }
}

/// Create a simple arguments object
fn create_arguments_object_simple(args: Vec<Value>) -> Value {
    let mut obj = Object::new(ObjectKind::Ordinary);
    for (i, arg) in args.iter().enumerate() {
        obj.set(&i.to_string(), arg.clone());
    }
    obj.set("length", Value::Number(args.len() as f64));
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Get or create the prototype for a class, caching it in the ClassValue
pub fn get_or_create_class_prototype(
    class: &ClassValue,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    {
        let cell = class.prototype_cell.borrow();
        if let Some(ref proto) = *cell {
            return Ok(Rc::clone(proto));
        }
    }

    let proto_rc = create_class_prototype_helper_with_env(class, env)?;

    {
        let mut cell = class.prototype_cell.borrow_mut();
        *cell = Some(Rc::clone(&proto_rc));
    }

    Ok(proto_rc)
}

/// Per ES §7.2.4 IsConstructor: check if a value is a constructor
fn is_constructor_value(val: &Value) -> bool {
    match val {
        Value::Class(_) => true,
        Value::NativeConstructor(_) => true,
        Value::NativeFunction(nf) => nf.prototype.borrow().is_some(),
        Value::Function(f) => !f.is_arrow,
        Value::Object(o) => {
            // Object-wrapped constructors (like Array) have a prototype property
            // and a callable constructor property.
            o.borrow().get("prototype").is_some()
                && o.borrow().get("constructor").is_some()
        }
        _ => false,
    }
}

/// Get prototype from a class value (used for extends)
#[allow(dead_code)]
fn get_prototype_from_class_val(val: &Value) -> Option<Rc<RefCell<Object>>> {
    match val {
        Value::Object(o) => {
            let proto = o.borrow().get("prototype");
            if let Some(Value::Object(proto_obj)) = proto {
                Some(proto_obj.clone())
            } else {
                None
            }
        }
        Value::Class(class) => {
            let cell = class.prototype_cell.borrow();
            if let Some(ref proto) = *cell {
                return Some(Rc::clone(proto));
            }
            None
        }
        _ => None,
    }
}

/// Create a prototype for a class value (helper for extends)
fn create_class_prototype_helper_with_env(
    class: &ClassValue,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    let parent_proto = if let Some(ref super_class) = class.super_class {
        let super_class_val = crate::eval::expression::eval_expression(super_class, env, false)?;
        // Per ES §14.6.13 ClassDefinitionEvaluation step 7: if superclass is
        // not null and IsConstructor(superclass) is false, throw TypeError.
        if !matches!(&super_class_val, Value::Null) && !is_constructor_value(&super_class_val) {
            return Err(JsError(
                "TypeError: superclass must be a constructor".to_string(),
            ));
        }
        get_prototype_from_class_val(&super_class_val)
    } else {
        builtins::get_object_prototype()
    };

    let mut proto = if let Some(parent) = parent_proto {
        Object::with_prototype(ObjectKind::Ordinary, parent)
    } else {
        Object::new(ObjectKind::Ordinary)
    };

    let closure = Rc::clone(env);
    // Bind super_class in the method closure so `super.x` resolves inside
    // methods and any arrow function defined within them.
    if let Some(ref super_class_expr) = class.super_class {
        let super_class_val =
            crate::eval::expression::eval_expression(super_class_expr, env, false)?;
        closure.borrow_mut().set_super_class(super_class_val);
    }
    // Class methods, getters, and setters share the enclosing lexical
    // Environment Records with the class expression. Each captures via
    // the same helper so block-scoped bindings declared around the class
    // remain reachable from inside its members.
    let member_closure = crate::eval::expression::capture_env_for_closure(&closure);
    for (name, params, body) in &class.methods {
        let key_str = prop_key_to_string(name, &closure, false)?;
        let mut func = ValueFunction::new(
            Some(key_str.clone()),
            params.clone(),
            body.clone(),
            Rc::clone(&member_closure),
        );
        // Class bodies are always strict mode (ES spec 15.7).
        func.strict = true;
        proto.set(&key_str, Value::Function(func));
    }

    for (name, body) in &class.getters {
        let key = prop_key_to_string(name, &closure, false)?;
        proto.set_getter(&key, Rc::new(body.clone()), Rc::clone(&member_closure));
    }

    for (name, param, body) in &class.setters {
        let key = prop_key_to_string(name, &closure, false)?;
        proto.set_setter(
            &key,
            param.clone(),
            Rc::new(body.clone()),
            Rc::clone(&member_closure),
        );
    }

    Ok(Rc::new(RefCell::new(proto)))
}

/// Legacy helper for creating prototype without environment (for operators.rs)
pub fn create_class_prototype_helper(class: &ClassValue) -> Result<Rc<RefCell<Object>>, JsError> {
    create_class_prototype_helper_with_env(class, &Rc::new(RefCell::new(Environment::new())))
}

/// Helper to convert PropertyKey to string, evaluating computed expressions
fn prop_key_to_string(
    key: &crate::ast::PropertyKey,
    env: &Rc<RefCell<Environment>>,
    in_arrow: bool,
) -> Result<String, JsError> {
    match key {
        crate::ast::PropertyKey::Ident(s) => Ok(s.clone()),
        crate::ast::PropertyKey::String(s) => Ok(s.clone()),
        crate::ast::PropertyKey::Number(n) => Ok(n.to_string()),
        crate::ast::PropertyKey::Computed(expr) => {
            let val = eval_expression(expr, env, in_arrow)?;
            Ok(crate::value::to_js_string(&val))
        }
    }
}

/// Get constructor prototype from a value
pub fn get_constructor_prototype(val: &Value) -> Result<Option<Rc<RefCell<Object>>>, JsError> {
    match val {
        Value::Object(o) => {
            let proto = o.borrow().get("prototype");
            if let Some(Value::Object(proto_obj)) = proto {
                Ok(Some(proto_obj.clone()))
            } else {
                Ok(None)
            }
        }
        Value::Function(f) => Ok(Some(f.get_prototype())),
        Value::NativeConstructor(nc) => Ok(Some(Rc::clone(&nc.prototype))),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod static_field_tests {
    use crate::Context;

    #[test]
    fn class_anonymous_has_static_field() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval("var C = class { static f = 42; }; C.f");
        assert_eq!(v.unwrap(), crate::value::Value::Number(42.0));
    }

    #[test]
    fn class_static_field_this_name() {
        let _ = 42;
        let mut ctx = Context::new().unwrap();
        // Check what `this.name` returns inside a static field of an anonymous class.
        let v = ctx.eval("var C = class { static f = this.name; }; C.f");
        eprintln!("this.name = {:?}", v);
    }

    #[test]
    fn class_caller_throws_type_error() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval(
            "var C = class {};
             var threw = false;
             try { C.caller; } catch(e) { threw = e instanceof TypeError; }
             threw",
        );
        assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
    }

    #[test]
    fn class_caller_throws_from_function() {
        let mut ctx = Context::new().unwrap();
        let v = ctx.eval(
            "var C = class {};
             function fn() { return C.caller; }
             var threw = false;
             try { fn(); } catch(e) { threw = e instanceof TypeError; }
             threw",
        );
        assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
    }

    #[test]
    fn class_caller_throws_assert_like() {
        use crate::value::{Value, NativeFunction};
        use std::rc::Rc;
        let mut ctx = Context::new().unwrap();
        // Register a native assert-like function
        let assert_like = Value::NativeFunction(Rc::new(NativeFunction::new(
            move |args: Vec<Value>| {
                let fn_value = args.get(0).cloned().unwrap_or(Value::Undefined);
                match fn_value {
                    Value::Function(f) => {
                        crate::eval::call_value_with_this(
                            Value::Function(f),
                            vec![],
                            Value::Undefined,
                        )
                    }
                    _ => Err(crate::value::JsError("not a function".to_string())),
                }
            }
        )));
        ctx.set_global("testCall".to_string(), assert_like);
        // The function returns Ok(Value::Undefined) if no error — good
        // The function returns Err(JsError) if error — which is what we want in our test
        let code = r#"
            var C = class {};
            function fn() { return C.caller; }
            var result = "no_error";
            try {
                testCall(fn);
                result = "no_error";
            } catch(e) {
                result = "error_thrown";
            }
            result
        "#;
        let v = ctx.eval(code);
        eprintln!("Result: {:?}", v);
    }
}
