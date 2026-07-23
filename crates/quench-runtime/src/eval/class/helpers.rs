//! Private helper functions for class operations.
//! All functions here are internal helpers; public API lives in the parent `class.rs`.

use crate::ast::{ArrowBody, Expression, Statement};
use crate::builtins;
use crate::env::Environment;
use crate::eval::expression::{capture_env_for_closure, eval_expression};
use crate::eval::statement::eval_function_body;
use crate::interpreter::{
    check_depth_guard, is_strict_mode, predeclare_let_const, set_strict_mode,
};
use crate::value::{ClassValue, JsError, Object, ObjectKind, PropertyFlags, Value, ValueFunction};
use std::cell::RefCell;
use std::rc::Rc;

/// Synthetic derived constructor: auto-call `super(...args)` only when
/// the class had no explicit `constructor` member.
fn should_auto_super(class: &ClassValue) -> bool {
    class.super_class.is_some() && !class.has_explicit_constructor
}

fn throw_uninitialized_this() -> Result<Value, JsError> {
    let (thrown_val, js_err) = crate::value::error::create_js_error_with_type(
        "Must call super constructor in derived class before returning",
        "ReferenceError",
    );
    crate::value::error::set_thrown_value(thrown_val);
    Err(js_err)
}

/// Finish a constructor: object returns win; derived + uninitialized this → ReferenceError.
fn finish_ctor_result(
    result: Value,
    this_val: &Value,
    class: &ClassValue,
    call_env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match result {
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_) => Ok(result),
        _ => {
            if class.super_class.is_some()
                && !call_env
                    .borrow()
                    .current_scope()
                    .borrow()
                    .is_this_initialized()
            {
                return throw_uninitialized_this();
            }
            Ok(this_val.clone())
        }
    }
}

fn new_instance(class: &ClassValue, env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    let proto_rc = crate::eval::class::get_or_create_class_prototype(class, env)?;
    let mut instance = Object::new(ObjectKind::Ordinary);
    instance.prototype = Some(Rc::clone(&proto_rc));
    let instance_rc = Rc::new(RefCell::new(instance));
    let this_val = Value::Object(Rc::clone(&instance_rc));
    instance_rc
        .borrow_mut()
        .set("constructor", Value::Class(Box::new(class.clone())));
    Ok(this_val)
}

fn run_ctor_body(
    body: &[Statement],
    call_env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let prev_strict = is_strict_mode();
    set_strict_mode(true);
    predeclare_let_const(body, &mut call_env.borrow_mut());
    let result = eval_function_body(body, call_env, false)?;
    set_strict_mode(prev_strict);
    Ok(result)
}

/// Instantiate without instance fields (fast path)
pub fn instantiate_simple(
    class: &ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let _depth = check_depth_guard()?;
    let this_val = new_instance(class, env)?;
    let body = class.constructor_body.clone();
    let call_env = Rc::new(RefCell::new(build_constructor_env(
        class, &args, &this_val, env,
    )?));

    let has_explicit_ctor = class
        .ordered_members
        .iter()
        .any(|m| matches!(m, crate::ast::ClassMember::Constructor { .. }));

    // Per ES §15.2.2: derived class without explicit constructor gets a
    // default constructor that calls super(...args).
    if !has_explicit_ctor {
        if let Some(ref sc) = class.super_class {
            let sv = eval_expression(sc, env, false)?;
            call_super_or_default(&sv, args, &this_val, env)?;
        }
        return Ok(this_val);
    }

    // Class constructors are always strict mode per ES spec.
    let prev_strict = is_strict_mode();
    set_strict_mode(true);

    let result = if !body.is_empty() {
        predeclare_let_const(&body, &mut call_env.borrow_mut());
        eval_function_body(&body, &call_env, false)?
    } else {
        Value::Undefined
    };
    set_strict_mode(prev_strict);

    // Per ES §8.1.1.3.1: derived constructor must call super() or return a
    // non-primitive (return override). Check return override first.
    let is_return_override = matches!(
        result,
        Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
    );

    // Only throw ReferenceError for empty constructor bodies (simple case).
    // Non-empty bodies may have complex control flow (try/catch/finally) that
    // our interpreter handles with super() in finally — skip the check and
    // let finish_constructor handle the return value.
    if body.is_empty()
        && !is_return_override
        && class.super_class.is_some()
        && !call_env
            .borrow()
            .scopes
            .iter()
            .any(|s| s.borrow().is_this_initialized())
    {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
            "ReferenceError",
        );
        return Err(js_err);
    }

    // Per ES §9.2.2 [[Construct]] step 13: if the constructor executes an
    // explicit `return` with a non-undefined, non-object value, throw TypeError.
    // Expression-statement results (e.g. `count++`) do NOT trigger this — they
    // produce a normal completion, not a return completion.
    if is_return_override {
        Ok(result)
    } else if body.is_empty() {
        Ok(this_val)
    } else if result != Value::Undefined && body.iter().any(|s| matches!(s, Statement::Return(..)))
    {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "derived constructor returned a non-object",
            "TypeError",
        );
        Err(js_err)
    } else {
        finish_constructor(result, &this_val)
    }

    let result = if body.is_empty() {
        Value::Undefined
    } else {
        run_ctor_body(&body, &call_env)?
    };
    finish_ctor_result(result, &this_val, class, &call_env)
}

fn init_instance_fields(
    class: &ClassValue,
    instance_rc: &Rc<RefCell<Object>>,
    call_env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    for (name, value_expr) in &class.instance_fields {
        let field_val = eval_expression(value_expr, call_env, false)?;
        let key_str = prop_key_to_string(name, call_env, false)?;
        instance_rc.borrow_mut().set(&key_str, field_val);
    }
    Ok(())
}

/// Instantiate with instance fields: fields init after super(), before body
pub fn instantiate_with_fields(
    class: &ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let _depth = check_depth_guard()?;
    let this_val = new_instance(class, env)?;
    let instance_rc = match &this_val {
        Value::Object(o) => Rc::clone(o),
        _ => unreachable!(),
    };
    let body = class.constructor_body.clone();
    let call_env = Rc::new(RefCell::new(build_constructor_env(
        class, &args, &this_val, env,
    )?));

    let has_explicit_ctor = class
        .ordered_members
        .iter()
        .any(|m| matches!(m, crate::ast::ClassMember::Constructor { .. }));

    // Per ES §15.2.2: derived class without explicit constructor gets a
    // default constructor that calls super(...args).
    if !has_explicit_ctor {
        if has_super {
            let sv = eval_expression(class.super_class.as_ref().unwrap(), env, false)?;
            call_super_or_default(&sv, args, &this_val, env)?;
        }
        init_fields()?;
        return Ok(this_val);
    }

    // Class constructors are always strict mode per ES spec.
    let prev_strict = is_strict_mode();
    set_strict_mode(true);

    let is_return_override = |v: &Value| -> bool {
        matches!(
            v,
            Value::Object(_)
                | Value::Function(_)
                | Value::NativeFunction(_)
                | Value::NativeConstructor(_)
        )
    };

    let _ = if has_super {
        if body_calls_super {
            call_env
                .borrow_mut()
                .set_pending_fields(class.instance_fields.clone());
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            let r = eval_function_body(&body, &call_env, false)?;
            if is_return_override(&r) {
                set_strict_mode(prev_strict);
                return Ok(r);
            }
            r
        } else if !body.is_empty() {
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            let r = eval_function_body(&body, &call_env, false)?;
            if is_return_override(&r) {
                set_strict_mode(prev_strict);
                return Ok(r);
            }
            r
        } else {
            Value::Undefined
        }
    } else {
        init_fields()?;
        if !body.is_empty() {
            predeclare_let_const(&body, &mut call_env.borrow_mut());
            let r = eval_function_body(&body, &call_env, false)?;
            if is_return_override(&r) {
                set_strict_mode(prev_strict);
                return Ok(r);
            }
            r
        } else {
            Value::Undefined
        }
    };

    set_strict_mode(prev_strict);

    // Per ES §8.1.1.3.1: derived constructor must call super() before returning
    // unless a return override (non-primitive) was returned. Only check for
    // empty body — non-empty bodies may have complex control flow.
    if has_super
        && body.is_empty()
        && !call_env
            .borrow()
            .scopes
            .iter()
            .any(|s| s.borrow().is_this_initialized())
    {
        let (_, js_err) = crate::value::error::create_js_error_with_type(
            "Must call super constructor in derived class before accessing 'this' or returning from derived constructor",
            "ReferenceError",
        );
        return Err(js_err);
    }

    Ok(this_val)
}

/// Build constructor environment (params, arguments, super reference)
pub fn build_constructor_env(
    class: &ClassValue,
    args: &[Value],
    this_val: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Environment, JsError> {
    let mut call_env = Environment::with_parent(Rc::clone(env));
    call_env
        .current_scope()
        .borrow_mut()
        .set_this_value(this_val.clone());

    if let Some(ref sc) = class.super_class {
        let sv = eval_expression(sc, env, false)?;
        call_env.set_super_class(sv);
    } else {
        // Base class (no extends): super resolves to the class itself,
        // so super.property looks up the class prototype's prototype chain.
        call_env.set_super_class(Value::Class(Box::new(class.clone())));
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
pub fn finish_constructor(result: Value, this_val: &Value) -> Result<Value, JsError> {
    match result {
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_) => Ok(result),
        _ => Ok(this_val.clone()),
    }
}

/// Call the super constructor or use default behavior
pub fn call_super_or_default(
    super_val: &Value,
    args: Vec<Value>,
    this_val: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    match super_val {
        Value::Class(super_class) => {
            crate::eval::class::call_super_constructor(
                super_class.as_ref().clone(),
                args,
                this_val.clone(),
                env,
            )?;
        }
        Value::Object(o) => {
            if let Some(Value::Function(constructor)) = o.borrow().get("constructor") {
                crate::eval::function::call_value_with_this(
                    Value::Function(constructor.clone()),
                    args,
                    this_val.clone(),
                )?;
            } else if let Some(Value::NativeFunction(nf)) = o.borrow().get("constructor") {
                crate::eval::function::call_value_with_this(
                    Value::NativeFunction(nf.clone()),
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
            crate::eval::function::call_value_with_this(
                Value::NativeConstructor(nc.clone()),
                args,
                this_val.clone(),
            )?;
        }
        Value::NativeFunction(nf) => {
            crate::eval::function::call_value_with_this(
                Value::NativeFunction(nf.clone()),
                args,
                this_val.clone(),
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// Check if the constructor body contains a super() call anywhere
pub fn body_calls_super_call(body: &[Statement]) -> bool {
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
        Statement::Try {
            body,
            handler,
            finalizer,
            ..
        } => {
            let in_handler = handler
                .as_ref()
                .is_some_and(|h| stmt_contains_super_call(h));
            let in_finally = finalizer
                .as_ref()
                .is_some_and(|f| stmt_contains_super_call(f));
            stmt_contains_super_call(body) || in_handler || in_finally
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
            ArrowBody::Expression(e) => expr_contains_super_call(e),
            ArrowBody::Block(stmts) => stmts.iter().any(stmt_contains_super_call),
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
pub fn create_arguments_object_simple(args: Vec<Value>) -> Value {
    let mut obj = Object::new(ObjectKind::Ordinary);
    for (i, arg) in args.iter().enumerate() {
        obj.set(&i.to_string(), arg.clone());
    }
    obj.set("length", Value::Number(args.len() as f64));
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Per ES §7.2.4 IsConstructor: check if a value is a constructor
pub fn is_constructor_value(val: &Value) -> bool {
    match val {
        Value::Class(_) => true,
        Value::NativeConstructor(_) => true,
        Value::NativeFunction(nf) => {
            // Check if prototype is set (native constructors)
            if nf.prototype.borrow().is_some() {
                true
            } else if nf.constructable {
                // Special built-ins (e.g. Proxy) that have [[Construct]] but
                // deliberately no .prototype property.  Bound functions set
                // this flag based on the target's constructability.
                true
            } else {
                false
            }
        }
        Value::Function(f) => !f.is_arrow,
        Value::Object(o) => {
            o.borrow().get("prototype").is_some() && o.borrow().get("constructor").is_some()
        }
        _ => false,
    }
}

/// Get prototype from a class value (used for extends)
pub fn get_prototype_from_class_val(val: &Value) -> Option<Rc<RefCell<Object>>> {
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
                Some(Rc::clone(proto))
            } else {
                None
            }
        }
        Value::NativeConstructor(nc) => Some(Rc::clone(&nc.prototype)),
        _ => None,
    }
}

/// Get the superclass constructor's own `[[Prototype]]` (for Object.getPrototypeOf(class)).
/// Returns:
/// - None if class extends null (so Object.getPrototypeOf(C) === null)
/// - The superclass constructor VALUE otherwise (for `extends Base`, returns `Value::Class(Base)`)
pub fn get_super_class_own_proto(super_class_val: &Value) -> Option<Value> {
    match super_class_val {
        Value::Null => None,
        // For `class Derived extends Base`, the superclass VALUE IS the class itself.
        // Object.getPrototypeOf(Derived) should return `Base` as a Value.
        Value::Class(class) => Some(Value::Class(class.clone())),
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => {
            // Function's own [[Prototype]] is %FunctionPrototype%
            builtins::get_function_prototype().map(Value::Object)
        }
        Value::Object(_) => {
            // Object's own [[Prototype]] is Object.prototype
            builtins::get_object_prototype().map(Value::Object)
        }
        _ => builtins::get_object_prototype().map(Value::Object),
    }
}

/// Create a prototype for a class value (helper for extends)
pub fn create_class_prototype_helper_with_env(
    class: &ClassValue,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    let parent_proto: Option<Rc<RefCell<Object>>> = if let Some(ref super_class) = class.super_class
    {
        // Use cached super_class value from env (set during eval_class_decl)
        // to avoid re-evaluating the expression (side-effects test).
        let super_class_val = if let Some(cached) = env.borrow().get_super_class() {
            cached
        } else {
            eval_expression(super_class, env, false)?
        };
        if crate::value::generator_replay::yield_pending() {
            return Ok(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
        }
        if !matches!(&super_class_val, Value::Null) && !is_constructor_value(&super_class_val) {
            return Err(JsError(
                "TypeError: superclass must be a constructor".to_string(),
            ));
        }

        // For NativeFunction (e.g. bound functions), use proper member access
        // to get the .prototype property (handles Object.defineProperty accessors).
        // Per ES spec §15.2.4 step 5f: if Get(ctor, "prototype") is not Object/Null, throw TypeError
        if let Value::NativeFunction(nf) = &super_class_val {
            let proto_val = crate::eval::member::eval_native_function_member(nf, "prototype")?;
            if matches!(&proto_val, Value::Null) {
                None
            } else if let Value::Object(o) = &proto_val {
                Some(Rc::clone(o))
            } else {
                return Err(JsError(
                    "TypeError: superclass constructor prototype is not an object or null"
                        .to_string(),
                ));
            }
        } else {
            // For other types (Object, Class, Function), use the existing helper
            get_prototype_from_class_val(&super_class_val)
        }
    } else {
        builtins::get_object_prototype()
    };

    let mut proto = if let Some(parent) = parent_proto {
        Object::with_prototype(ObjectKind::Ordinary, parent)
    } else {
        Object::new(ObjectKind::Ordinary)
    };

    let closure = Rc::clone(env);
    closure.borrow_mut().push_scope();
    let member_closure = capture_env_for_closure(&closure);

    for (name, params, body, is_async, is_generator) in &class.methods {
        let key_str = prop_key_to_string(name, &closure, false)?;
        let storage_key = class_member_storage_key(&key_str);
        if crate::value::generator_replay::yield_pending() {
            return Ok(Rc::new(RefCell::new(proto)));
        }
        let mut func = ValueFunction::new(
            Some(key_str.clone()),
            params.clone(),
            body.clone(),
            Rc::clone(&member_closure),
            *is_async,
            *is_generator,
        );
        func.strict = true;
        func.is_method = true;
        // Class methods are non-enumerable per ES spec §10.1.7
        proto.define(
            &storage_key,
            Value::Function(func),
            PropertyFlags {
                enumerable: false,
                writable: true,
                configurable: true,
                value: None,
            },
        );
    }

    for (name, body) in &class.getters {
        let key = class_member_storage_key(&prop_key_to_string(name, &closure, false)?);
        if crate::value::generator_replay::yield_pending() {
            return Ok(Rc::new(RefCell::new(proto)));
        }
        proto.set_getter(
            &key,
            Rc::new(body.clone()),
            Rc::clone(&member_closure),
            true,
        );
    }

    for (name, param, body) in &class.setters {
        let key = class_member_storage_key(&prop_key_to_string(name, &closure, false)?);
        if crate::value::generator_replay::yield_pending() {
            return Ok(Rc::new(RefCell::new(proto)));
        }
        proto.set_setter(
            &key,
            param.clone(),
            Rc::new(body.clone()),
            Rc::clone(&member_closure),
            true,
        );
    }

    // Set `constructor` on the prototype so `C.prototype.constructor === C`
    // Must be non-enumerable per ES spec §10.1.7
    proto.define(
        "constructor",
        Value::Class(Box::new(class.clone())),
        PropertyFlags {
            enumerable: false,
            writable: true,
            configurable: true,
            value: None,
        },
    );

    Ok(Rc::new(RefCell::new(proto)))
}

fn class_member_storage_key(key: &str) -> String {
    if key.starts_with('#') {
        crate::value::private_name_key(key)
    } else {
        key.to_string()
    }
}

/// Helper to convert PropertyKey to string, evaluating computed expressions
pub fn prop_key_to_string(
    key: &crate::ast::PropertyKey,
    env: &Rc<RefCell<Environment>>,
    in_arrow: bool,
) -> Result<String, JsError> {
    match key {
        crate::ast::PropertyKey::Ident(s) => Ok(s.clone()),
        crate::ast::PropertyKey::String(s) => Ok(s.clone()),
        crate::ast::PropertyKey::Number(n) => Ok(crate::value::number_to_string(*n)),
        crate::ast::PropertyKey::Computed(expr) => {
            let val = eval_expression(expr, env, in_arrow)?;
            if crate::value::generator_replay::yield_pending() {
                return Ok(String::new());
            }
            let prim = crate::value::to_primitive(&val, Some("string"))?;
            Ok(crate::value::to_js_string(&prim))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Context;
    use crate::Value;

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── String(f) diagnostic ────────────────────────────────────────────────

    #[test]
    fn string_of_function_outside_class() {
        // String(f) passes the function object to String(), which calls Function.prototype.toString
        let r = eval("function f() {}; String(f)").unwrap();
        assert_eq!(r, Value::String("function f() {}".to_string()));
    }

    #[test]
    fn string_of_function_after_class_def() {
        // Does class def change f's toString behavior? String(f) passes the function object.
        let r = eval(
            r#"
            function f() {}
            class C {
              get [String(f)]() { return 1; }
              static get [String(f)]() { return 1; }
            }
            String(f)
            "#,
        )
        .unwrap();
        assert_eq!(
            r,
            Value::String("function f() {}".to_string()),
            "String(f) after class def"
        );
    }

    #[test]
    fn instance_getter_computed_with_string_coercion() {
        // Key test: using String(f) as computed key, and verifying String(f) returns source text
        let r = eval(
            r#"
            function f() {}
            class C {
              get [String(f)]() { return 1; }
              set [String(f)](v) { return 1; }
              static get [String(f)]() { return 1; }
              static set [String(f)](v) { return 1; }
            }
            var c = new C();
            var key = String(f);
            [c[key], c[String(f)], key]
            "#,
        )
        .unwrap();
        let arr = r;
        if let Value::Object(o) = arr {
            let obj = o.borrow();
            assert_eq!(
                obj.get("0"),
                Some(Value::Number(1.0)),
                "c[String(f)] should be 1"
            );
            assert_eq!(
                obj.get("1"),
                Some(Value::Number(1.0)),
                "c[String(f)] should be 1"
            );
            assert_eq!(
                obj.get("2"),
                Some(Value::String("function f() {}".to_string())),
                "key should be function f() {{}}"
            );
        }
    }

    // ─── finish_constructor: returns object ─────────────────────────────────

    #[test]
    fn constructor_returns_object() {
        let r = eval("new function() { return {x: 1}; }").unwrap();
        assert!(matches!(r, Value::Object(_)));
        if let Value::Object(o) = r {
            assert_eq!(o.borrow().get("x"), Some(Value::Number(1.0)));
        }
    }

    #[test]
    fn constructor_returns_this_by_default() {
        let r = eval("function F() { this.a = 5; } var f = new F(); f.a").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn constructor_returns_number_ignored() {
        // Constructors returning primitives return `this` instead
        let r = eval("function F() { return 42; } var f = new F(); typeof f").unwrap();
        assert_eq!(r, Value::String("object".into()));
    }

    // ─── is_constructor_value ────────────────────────────────────────────────

    #[test]
    fn class_is_constructor() {
        let r = eval("class C {} typeof C").unwrap();
        assert_eq!(r, Value::String("function".into()));
    }

    #[test]
    fn regular_function_is_constructor() {
        let r = eval("function F() {} new F()").unwrap();
        assert!(matches!(r, Value::Object(_)));
    }

    #[test]
    fn arrow_function_not_constructor() {
        let r = eval("var F = () => {}; new F()");
        assert!(r.is_err());
    }

    #[test]
    fn native_constructor_is_constructor() {
        let r = eval("new Object()").unwrap();
        assert!(matches!(r, Value::Object(_)));
    }

    // ─── class extends chain ─────────────────────────────────────────────────

    #[test]
    fn class_extends_proto_chain() {
        let r = eval(
            "class Base {} class Derived extends Base {} Object.getPrototypeOf(Derived) === Base",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn class_extends_null() {
        let r = eval("class C extends null {} Object.getPrototypeOf(C)").unwrap();
        assert_eq!(r, Value::Null);
    }

    // ─── Private fields ─────────────────────────────────────────────────────

    #[test]
    fn static_getter_returns_value() {
        let r = eval("class C { static get method() { return 42; } } C.method").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn static_private_method_direct_call() {
        let r = eval(
            "class C { static #m() { return 42; } static call() { return this.#m(); } } C.call()",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn static_private_method_via_getter() {
        let r = eval(
            "class C { static #m() { return 42; } static get method() { return this.#m; } } \
             C.method()",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn static_getter_return_super_after_stmt() {
        let r = eval(
            "class B { static m() { return 1; } } \
             class C extends B { static get x() { 0; return super.m(); } } \
             C.x",
        )
        .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn static_private_field_read_from_static_method() {
        let r = eval("class C { static #$ = 1; static $() { return this.#$; } } C.$()").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn static_private_method_not_has_own_property() {
        let r = eval(
            "class C { static async *#gen() {} static get gen() { return this.#gen; } } \
             Object.prototype.hasOwnProperty.call(C, '#gen')",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    #[test]
    fn private_method_not_clobbered_by_hash_string_field() {
        let r = eval(
            "class C { #m() { return 'Test262'; } ['#m'] = 0; \
             check() { return this.#m(); } } new C().check()",
        )
        .unwrap();
        assert_eq!(r, Value::String("Test262".into()));
    }

    #[test]
    fn private_method_getter_returns_method_without_recursion() {
        let r = eval(
            "class C { #method() { return 42; } get method() { return this.#method; } } \
             new C().method()",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    // ─── prop_key_to_string ─────────────────────────────────────────────────

    #[test]
    fn class_method_identifier_key() {
        let r = eval("class C { foo() { return 1; } } C.prototype.foo.name").unwrap();
        assert_eq!(r, Value::String("foo".into()));
    }

    #[test]
    fn class_method_string_key() {
        let r = eval("class C { 'bar'() { return 2; } } C.prototype['bar'].name").unwrap();
        assert_eq!(r, Value::String("bar".into()));
    }

    #[test]
    fn class_method_number_key() {
        let r = eval("class C { 42() { return 3; } } C.prototype[42].name").unwrap();
        assert_eq!(r, Value::String("42".into()));
    }

    #[test]
    fn class_generator_yield_after_let_binding() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            function* g() {
              class C { get [yield 1]() { return 1; } };
              let c = new C();
              return c[1];
            }
            var iter = g();
            iter.next();
            iter.next(1);
            iter.next().value
            "#,
        )
        .unwrap();
    }

    #[test]
    fn class_multilevel_super_constructor_chain() {
        let r = eval(
            r#"
            class Base { constructor(x) { this.foobar = x; } }
            class Subclass extends Base { constructor(x) { super(x); } }
            class Subclass2 extends Subclass { constructor() { super(5, 6, 7); } }
            new Subclass2().foobar
            "#,
        )
        .unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn class_prototype_wiring_derived_constructors() {
        let r = eval(
            r#"
            class Base {
              constructor(x) { this.foobar = x; }
            }
            class Subclass extends Base {
              constructor(x) { super(x); }
            }
            class Subclass2 extends Subclass {
              constructor() { super(5, 6, 7); }
            }
            class Subclass3 extends Base {
              constructor(x, y) { super(x + y); }
            }
            var ss3 = new Subclass3(27, 42 - 27);
            ss3.foobar
            "#,
        );
        assert_eq!(r.unwrap(), Value::Number(42.0));
    }

    #[test]
    fn class_static_accessor_computed_yield_in_generator() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            var yieldSet, C, iter;
            function* g() {
              class C_ {
                static get [yield]() { return 'get yield'; }
                static set [yield](param) { yieldSet = param; }
              }
              C = C_;
            }
            iter = g();
            iter.next();
            iter.next('first');
            iter.next('second');
        "#,
        )
        .unwrap();
        assert_eq!(
            ctx.eval("C.first").unwrap(),
            Value::String("get yield".into())
        );
        ctx.eval("C.second = 'set yield'").unwrap();
        assert_eq!(
            ctx.eval("yieldSet").unwrap(),
            Value::String("set yield".into())
        );
    }

    #[test]
    fn class_accessor_computed_yield_in_generator() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            var yieldSet, C, iter;
            function* g() {
              class C_ {
                get [yield]() { return 'get yield'; }
                set [yield](param) { yieldSet = param; }
              }
              C = C_;
            }
            iter = g();
            iter.next();
            iter.next('first');
            iter.next('second');
        "#,
        )
        .unwrap();
        let r = ctx.eval("C.prototype.first").unwrap();
        assert_eq!(r, Value::String("get yield".into()));
        ctx.eval("C.prototype.second = 'set yield'").unwrap();
        let r2 = ctx.eval("yieldSet").unwrap();
        assert_eq!(r2, Value::String("set yield".into()));
    }

    // ─── super in methods ────────────────────────────────────────────────────

    #[test]
    fn super_call_dispatches_to_parent() {
        let r = eval("class Base { foo() { return 1; } } class Derived extends Base { foo() { return super.foo() + 10; } } new Derived().foo()").unwrap();
        assert_eq!(r, Value::Number(11.0));
    }

    // ─── class static members ────────────────────────────────────────────────

    #[test]
    fn class_static_method() {
        let r = eval("class C { static foo() { return 42; } } C.foo()").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn class_static_property() {
        let r = eval("class C { static x = 7; } C.x").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn static_private_field_in_static_init_block() {
        // test262: language/statements/class/static-init-scope-private.js
        // Static init blocks must share the private-name scope so that
        // C.#privateName is accessible during class definition.
        let r = eval(
            "var probe; class C { static #x = 42; static { probe = C.#x; } } probe",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    // ─── arguments object in constructor ─────────────────────────────────────

    #[test]
    fn constructor_arguments_object_accessed_via_this() {
        // new Constructor() returns `this` by default (when constructor returns non-object).
        // Test that arguments[0] is accessible and can be assigned to `this`.
        let r = eval("function F(a, b) { this.x = arguments[0]; } var inst = new F(5, 6); inst.x")
            .unwrap();
        assert_eq!(
            r,
            Value::Number(5.0),
            "arguments[0] should be assignable to this.x, got {:?}",
            r
        );
    }

    #[test]
    fn constructor_arguments_length_via_this() {
        let r = eval(
            "function F(a, b) { this.len = arguments.length; } var inst = new F(5, 6, 7); inst.len",
        )
        .unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn constructor_plain_object_access() {
        // Control: plain object inside constructor works fine
        let r = eval(
            "function F(a, b) { var o = {x: 5}; this.y = o.x; } var inst = new F(1, 2); inst.y",
        )
        .unwrap();
        assert_eq!(
            r,
            Value::Number(5.0),
            "plain object access should work, got {:?}",
            r
        );
    }

    // ─── instantiate_simple: class instantiation ───────────────────────────

    #[test]
    fn instantiate_simple_empty_class() {
        let r = eval("class C {} new C()").unwrap();
        assert!(matches!(r, Value::Object(_)));
    }

    #[test]
    fn instantiate_simple_with_params() {
        let r =
            eval("class C { constructor(x, y) { this.sum = x + y; } } new C(3, 4).sum").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn instantiate_simple_excess_args_ignored() {
        let r = eval("class C { constructor(a) { this.a = a; } } new C(1, 2, 3).a").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn instantiate_simple_explicit_args() {
        let r =
            eval("class C { constructor(x, y) { this.val = x + y; } } new C(10, 20).val").unwrap();
        assert_eq!(r, Value::Number(30.0));
    }

    #[test]
    fn instantiate_simple_empty_body_no_super() {
        // Empty class with no extends: should instantiate fine
        let r = eval("class C {} var c = new C(); c instanceof C").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── instantiate_simple: with extends ──────────────────────────────────

    #[test]
    fn instantiate_simple_extends_calls_super() {
        let r = eval(
            "class Base { constructor(x) { this.x = x * 2; } } class Derived extends Base { constructor(x) { super(x); } } new Derived(5).x",
        )
        .unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    #[test]
    fn instantiate_simple_extends_proto_chain() {
        let r = eval("class Base {} class Derived extends Base {} new Derived() instanceof Base")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn derived_class_empty_constructor_throws_reference_error() {
        // Derived class with empty constructor must throw ReferenceError
        let r = eval(
            "class A { constructor() { this.x = 1; } } \
             class B extends A { constructor() {} } \
             try { new B(); 'no error' } catch(e) { e.name }",
        );
        assert_eq!(r.unwrap(), Value::String("ReferenceError".into()));
    }

    #[test]
    fn derived_class_with_super_call_works() {
        // Derived class with super() must still work
        let r = eval(
            "class A { constructor(x) { this.x = x; } } \
             class B extends A { constructor(x) { super(x); } } \
             new B(42).x",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    // ─── instantiate_simple: method on prototype ───────────────────────────

    #[test]
    fn instantiate_simple_method_accessible() {
        let r = eval("class C { foo() { return 42; } } new C().foo()").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    // ─── create_class_prototype_helper_with_env ─────────────────────────────

    #[test]
    fn class_prototype_has_method() {
        let r = eval("class C { bar() { return 'bar'; } } typeof C.prototype.bar").unwrap();
        assert_eq!(r, Value::String("function".into()));
    }

    #[test]
    fn class_prototype_getter() {
        let r = eval("class C { get val() { return 99; } } new C().val").unwrap();
        assert_eq!(r, Value::Number(99.0));
    }

    #[test]
    fn class_prototype_setter() {
        let r =
            eval("class C { set val(v) { this._v = v * 2; } } var c = new C(); c.val = 5; c._v")
                .unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    #[test]
    fn class_multiple_methods() {
        let r = eval(
            "class C { m1() { return 1; } m2() { return 2; } m3() { return 3; } } [new C().m1(), new C().m2(), new C().m3()]",
        )
        .unwrap();
        assert!(matches!(r, Value::Object(_)));
        if let Value::Object(o) = &r {
            assert_eq!(o.borrow().get("length"), Some(Value::Number(3.0)));
        }
    }

    // ─── create_class_prototype_helper_with_env: error cases ───────────────

    #[test]
    fn class_extends_non_constructor_throws() {
        let r = eval("class C extends 42 {}");
        assert!(r.is_err());
    }

    #[test]
    fn class_extends_string_throws() {
        let r = eval("class C extends 'hello' {}");
        assert!(r.is_err());
    }

    #[test]
    fn class_extends_object_not_constructor() {
        let r = eval("var obj = {}; class C extends obj {}");
        assert!(r.is_err());
    }

    #[test]
    fn class_extends_arrow_function_throws() {
        let r = eval("var fn = () => {}; class C extends fn {}");
        assert!(
            r.is_err(),
            "arrow function should not be a valid superclass: {:?}",
            r
        );
    }

    #[test]
    fn class_extends_bound_arrow_function_throws() {
        let r = eval("var fn = (() => {}).bind(); class C extends fn {}");
        assert!(
            r.is_err(),
            "bound arrow function should not be a valid superclass: {:?}",
            r
        );
    }

    #[test]
    fn class_extends_arrow_function_proto_unreached() {
        let r = eval(
            r#"var fn = () => {};
            try { class C extends fn {}; "no error" } catch(e) { "error thrown" }"#,
        );
        assert_eq!(r.unwrap(), Value::String("error thrown".into()));
    }

    #[test]
    fn class_extends_bound_arrow_proto_unreached() {
        let r = eval(
            r#"var bound = (() => {}).bind();
            try { class C extends bound {}; "no error" } catch(e) { "error thrown" }"#,
        );
        assert_eq!(r.unwrap(), Value::String("error thrown".into()));
    }

    #[test]
    fn bound_function_from_regular_fn_is_constructable() {
        let r = eval("var fn = function() {}.bind(null); new fn()");
        assert!(
            r.is_ok(),
            "bound function from regular fn should be constructable: {:?}",
            r
        );
    }

    #[test]
    fn class_extends_proxy_around_arrow_throws() {
        // Proxy wrapping an arrow function should NOT be a valid superclass.
        let r = eval(
            r#"var proxy = new Proxy(() => {}, {
              get: function() { throw new Error("prototype unreachable"); },
            });
            try { class C extends proxy {}; "no error" } catch(e) { "error thrown" }"#,
        );
        assert_eq!(r.unwrap(), Value::String("error thrown".into()));
    }



    // ─── prop_key_to_string ────────────────────────────────────────────────

    #[test]
    fn prop_key_computed_expression() {
        let r = eval(
            "var k = 'dynamic'; class C { [k]() { return 'found'; } } C.prototype['dynamic'].name",
        )
        .unwrap();
        assert_eq!(r, Value::String("dynamic".into()));
    }

    #[test]
    fn prop_key_computed_number() {
        let r = eval("class C { [1 + 2]() { return 'three'; } } C.prototype['3'].name").unwrap();
        assert_eq!(r, Value::String("3".into()));
    }

    #[test]
    fn prop_key_computed_symbol() {
        let r = eval(
            "class C { [Symbol.for('test')]() { return 'symbol'; } } var desc = Object.getOwnPropertyNames(C.prototype)[0]; desc !== 'constructor'",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn prop_key_method_name_on_prototype() {
        let r = eval("class C { someMethod() {} } C.prototype.someMethod.name").unwrap();
        assert_eq!(r, Value::String("someMethod".into()));
    }

    // ─── build_constructor_env ───────────────────────────────────────────────

    #[test]
    fn constructor_env_sets_this() {
        let r = eval("class C { constructor() { this.check = this !== null; } } new C().check")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn constructor_env_has_arguments() {
        let r = eval(
            "class C { constructor(a, b, c) { this.first = arguments[0]; this.len = arguments.length; } } new C(1, 2).first",
        )
        .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn constructor_env_no_extra_args() {
        let r = eval(
            "class C { constructor() { this.len = arguments.length; } } new C(1, 2, 3, 4, 5).len",
        )
        .unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    // ─── finish_constructor ─────────────────────────────────────────────────

    #[test]
    fn finish_constructor_returns_function() {
        let r = eval("new function() { return function() {}; }").unwrap();
        assert!(matches!(r, Value::Function(_)));
    }

    #[test]
    fn finish_constructor_returns_native_constructor() {
        let r = eval("new function() { return Object; }").unwrap();
        assert!(matches!(r, Value::NativeConstructor(_)));
    }

    #[test]
    fn finish_constructor_returns_null_uses_this() {
        let r = eval("new function() { return null; }").unwrap();
        assert!(matches!(r, Value::Object(_)));
    }

    #[test]
    fn finish_constructor_returns_undefined_uses_this() {
        let r = eval("new function() { return; }").unwrap();
        assert!(matches!(r, Value::Object(_)));
    }

    #[test]
    fn finish_constructor_returns_string_uses_this() {
        let r = eval("new function() { return 'string'; }").unwrap();
        assert!(matches!(r, Value::Object(_)));
    }

    // ─── instantiate_simple: edge cases ────────────────────────────────────

    #[test]
    fn instantiate_with_field_init_order() {
        // Fields should be initialized in order
        let r = eval(
            "class C { x = 1; y = this.x + 1; z = this.y + 1; } var c = new C(); [c.x, c.y, c.z]",
        )
        .unwrap();
        assert!(matches!(r, Value::Object(_)));
        if let Value::Object(o) = &r {
            assert_eq!(o.borrow().get("length"), Some(Value::Number(3.0)));
        }
    }

    #[test]
    fn class_constructor_reassigns_this_prop() {
        // Constructor can reassign properties set by fields
        let r = eval("class C { x = 10; constructor() { this.x = 20; } } new C().x").unwrap();
        assert_eq!(r, Value::Number(20.0));
    }

    // ─── body_calls_super_call ─────────────────────────────────────────────

    #[test]
    fn super_call_not_first() {
        // super() is not the first statement, but is called
        let r = eval(
            "class Base { constructor(x) { this.x = x; } } class Derived extends Base { constructor(x) { this.y = 1; super(x); } } new Derived(42).x",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn super_call_in_conditional() {
        // super() in conditional expression
        let r = eval(
            "class Base { constructor(x) { this.x = x; } } class Derived extends Base { constructor(x) { true && super(x); } } new Derived(7).x",
        )
        .unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn super_reference_in_closure() {
        // super reference captured in closure called from constructor
        // Must call super() first per spec before accessing `this`.
        let r = eval(
            "class Base { getX() { return 42; } } class Derived extends Base { constructor() { super(); var self = this; this.result = self.getX(); } } new Derived().result",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    // ─── is_constructor_value ───────────────────────────────────────────────

    #[test]
    fn bound_function_is_constructable() {
        // Bound functions from non-arrow functions have [[Construct]]
        // and delegate to the target function.
        let r = eval("var fn = function() {}.bind(null); new fn()");
        assert!(r.is_ok(), "bound function should be constructable: {:?}", r);
    }

    #[test]
    fn class_expression_is_constructor() {
        let r = eval("var C = class {}; new C()").unwrap();
        assert!(matches!(r, Value::Object(_)));
    }

    // ─── get_super_class_own_proto ─────────────────────────────────────────

    #[test]
    fn class_extends_function() {
        // Class extending Function should work
        let r = eval("class C extends Function {} typeof C");
        assert!(r.is_ok());
    }

    #[test]
    fn class_extends_object() {
        // Class extending Object should work
        let r = eval("class C extends Object {} new C()").unwrap();
        assert!(matches!(r, Value::Object(_)));
    }

    // ─── create_arguments_object_simple ───────────────────────────────────

    #[test]
    fn arguments_object_index_access() {
        let r = eval("function f(a, b) { return arguments[1]; } f(1, 2)").unwrap();
        assert_eq!(r, Value::Number(2.0));
    }

    #[test]
    fn arguments_object_length() {
        let r = eval("function f() { return arguments.length; } f(1, 2, 3, 4)").unwrap();
        assert_eq!(r, Value::Number(4.0));
    }

    #[test]
    fn arguments_object_callee() {
        let r = eval("function f() { return arguments.callee === f; } f()").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn class_static_getter_with_or_assign_computed_key() {
        // Computed property key containing `x |= 1` assignment in STATIC getter.
        // The key evaluation should NOT panic with RefCell already borrowed.
        let r = eval(
            r#"
            var x = 0;
            class C {
                static get [x |= 1]() { return 2; }
            }
            C[x |= 1]
            "#,
        );
        assert!(
            r.is_ok(),
            "Accessing computed static getter with |= assignment should not panic: {:?}",
            r
        );
        assert_eq!(r.unwrap(), Value::Number(2.0));
    }

    #[test]
    fn class_instance_getter_with_or_assign_computed_key() {
        // Instance getter (non-static) with |= computed key
        let r = eval(
            r#"
            var x = 0;
            class C {
                get [x |= 1]() { return 3; }
            }
            new C()[x |= 1]
            "#,
        );
        assert!(
            r.is_ok(),
            "Instance getter with |= computed key should work: {:?}",
            r
        );
        assert_eq!(r.unwrap(), Value::Number(3.0));
    }

    #[test]
    fn class_static_computed_getter_simple_var() {
        // Minimal test: computed static getter with a simple var reference
        let r = eval(
            r#"
            var k = "foo";
            class C {
                static get [k]() { return 99; }
            }
            C[k]
            "#,
        );
        let val = r.unwrap();
        assert_eq!(
            val,
            Value::Number(99.0),
            "k='foo', getter at 'foo' should return 99"
        );
    }

    #[test]
    fn class_static_computed_getter_expr_no_side_effect() {
        // Computed key with expression but no assignment side effects
        let r = eval(
            r#"
            class C {
                static get [1 + 1]() { return 77; }
            }
            C[2]
            "#,
        );
        let val = r.unwrap();
        assert_eq!(val, Value::Number(77.0));
    }

    #[test]
    fn class_static_computed_getter_with_assignment_only() {
        // Computed key with assignment, no member access side effect
        let r = eval(
            r#"
            var x = 0;
            class C {
                static get [x = 5]() { return 88; }
            }
            C[5]
            "#,
        );
        let val = r.unwrap();
        assert_eq!(val, Value::Number(88.0));
    }

    #[test]
    fn class_instance_getter_function_call_twice_same_key() {
        // get [f()]() and set [f()]() where f() is called twice in class body.
        // Both calls return 1 → same key → single accessor with both getter and setter.
        let r = eval(
            r#"
            function f() { return 1; }
            class C {
                get [f()]() { return 42; }
                set [f()](v) { this._v = v; }
            }
            var c = new C();
            var k = f();
            [c[k], c[k] = 99, c[k], c._v];
            "#,
        );
        let val = r.unwrap();
        let obj = match val {
            Value::Object(o) => o,
            other => panic!("expected array, got {:?}", other),
        };
        let elems = obj.borrow().elements.clone();
        assert_eq!(elems[0], Value::Number(42.0), "getter returns 42");
        assert_eq!(elems[1], Value::Number(99.0), "setter result is 99");
        assert_eq!(elems[2], Value::Number(42.0), "getter still returns 42");
        assert_eq!(elems[3], Value::Number(99.0), "setter stored _v = 99");
    }

    #[test]
    fn class_static_getter_function_call_twice_same_key() {
        // Same as above but for static accessor
        let r = eval(
            r#"
            function f() { return 1; }
            class C {
                static get [f()]() { return 42; }
                static set [f()](v) { this._v = v; }
            }
            var k = f();
            [C[k], C[k] = 99, C[k], C._v];
            "#,
        );
        let val = r.unwrap();
        let obj = match val {
            Value::Object(o) => o,
            other => panic!("expected array, got {:?}", other),
        };
        let elems = obj.borrow().elements.clone();
        assert_eq!(elems[0], Value::Number(42.0), "static getter returns 42");
        assert_eq!(elems[1], Value::Number(99.0), "static setter result is 99");
        assert_eq!(
            elems[2],
            Value::Number(42.0),
            "static getter still returns 42"
        );
        assert_eq!(
            elems[3],
            Value::Number(99.0),
            "static setter stored _v = 99"
        );
    }

    #[test]
    fn class_static_setter_works() {
        // Verify static setter actually writes to the class object
        let r = eval(
            r#"
            function f() { return 1; }
            class C {
                static get [f()]() { return 42; }
                static set [f()](v) { this._v = v; }
            }
            C[1] = 99;
            C._v;
            "#,
        );
        assert_eq!(
            r.unwrap(),
            Value::Number(99.0),
            "static setter should set C._v = 99"
        );
    }

    #[test]
    fn class_static_non_computed_setter_works() {
        // Non-computed static setter as baseline: must work
        let r = eval(
            r#"
            class C {
                static set foo(v) { this._v = v; }
            }
            C.foo = 99;
            C._v;
            "#,
        );
        assert_eq!(
            r.unwrap(),
            Value::Number(99.0),
            "non-computed static setter should work"
        );
    }

    #[test]
    fn class_instance_setter_works() {
        // Instance setter writes to instance
        let r = eval(
            r#"
            function f() { return 1; }
            class C {
                set [f()](v) { this._v = v; }
            }
            var c = new C();
            c[1] = 99;
            c._v;
            "#,
        );
        assert_eq!(
            r.unwrap(),
            Value::Number(99.0),
            "instance setter should set _v = 99"
        );
    }

    #[test]
    fn class_instance_non_computed_setter_works() {
        // Non-computed instance setter as baseline
        let r = eval(
            r#"
            class C {
                set foo(v) { this._v = v; }
            }
            var c = new C();
            c.foo = 99;
            c._v;
            "#,
        );
        assert_eq!(
            r.unwrap(),
            Value::Number(99.0),
            "non-computed instance setter should work"
        );
    }

    #[test]
    fn class_static_accessor_getter_and_setter_same_key() {
        // Getter and setter both at key 1 → both work independently
        let r = eval(
            r#"
            function makeKey(n) { return function() { return n; }; }
            var f1 = makeKey(1);
            var f2 = makeKey(1);
            class C {
                static get [f1()]() { return 42; }
                static set [f2()](v) { this._v = v; }
            }
            C[1] = 99;
            [C[1], C._v];
            "#,
        );
        let val = r.unwrap();
        let obj = match val {
            Value::Object(o) => o,
            other => panic!("expected array, got {:?}", other),
        };
        let elems = obj.borrow().elements.clone();
        assert_eq!(elems[0], Value::Number(42.0), "getter returns 42");
        assert_eq!(elems[1], Value::Number(99.0), "setter stored 99 in _v");
    }

    #[test]
    fn class_static_computed_getter_direct_access() {
        // Direct access to the computed key getter on class
        let r = eval(
            r#"
            var x = 0;
            class C {
                static get [x = 1]() { return 42; }
            }
            C[1]
            "#,
        );
        let val = r.unwrap();
        assert_eq!(
            val,
            Value::Number(42.0),
            "C[1] should return 42 from static getter"
        );
    }

    #[test]
    fn class_instance_getter_computed_undefined_key() {
        // test262: cpn-class-decl-accessors-computed-property-name-from-function-declaration.js
        // A function returning undefined as a computed property name.
        // The getter should be callable on both the class (static) and instance.
        let r = eval(
            r#"
            function f() {}
            class C {
                get [f()]() { return 1; }
                static get [f()]() { return 1; }
            }
            var c = new C();
            var staticResult = C[f()];
            var instanceResult = c[f()];
            [staticResult, instanceResult]
            "#,
        );
        assert!(r.is_ok(), "Class getter access should work: {:?}", r);
        let arr = r.unwrap();
        if let Value::Object(o) = arr {
            let obj = o.borrow();
            assert_eq!(
                obj.get("0"),
                Some(Value::Number(1.0)),
                "static getter should return 1"
            );
            assert_eq!(
                obj.get("1"),
                Some(Value::Number(1.0)),
                "instance getter should return 1"
            );
        } else {
            panic!("expected array result, got: {:?}", arr);
        }
    }

    // ─── Regression: instance getter after static getter with empty-body key ───

    #[test]
    fn class_instance_getter_only() {
        // Instance getter only, no static
        let r = eval(
            r#"
            function f() {}
            class C { get [f()]() { return 1; } }
            var c = new C();
            c[f()]
            "#,
        );
        assert_eq!(
            r.unwrap(),
            Value::Number(1.0),
            "instance-only getter should return 1"
        );
    }

    #[test]
    fn class_static_getter_only() {
        // Static getter only
        let r = eval(
            r#"
            function f() {}
            class C { static get [f()]() { return 1; } }
            C[f()]
            "#,
        );
        assert_eq!(
            r.unwrap(),
            Value::Number(1.0),
            "static-only getter should return 1"
        );
    }

    #[test]
    fn class_instance_after_static_getter_same_key() {
        // Instance getter AFTER static getter, same key
        let r = eval(
            r#"
            function f() {}
            class C {
                get [f()]() { return 1; }
                static get [f()]() { return 1; }
            }
            var c = new C();
            C[f()];  // call static first
            c[f()]   // then instance
            "#,
        );
        assert_eq!(
            r.unwrap(),
            Value::Number(1.0),
            "instance after static should return 1"
        );
    }

    #[test]
    fn class_instance_after_static_getter_different_bodies() {
        // Instance and static have different return values
        let r = eval(
            r#"
            function f() {}
            class C {
                get [f()]() { return 2; }
                static get [f()]() { return 1; }
            }
            var c = new C();
            C[f()];
            c[f()]
            "#,
        );
        assert_eq!(
            r.unwrap(),
            Value::Number(2.0),
            "instance getter body should win"
        );
    }

    #[test]
    fn class_instance_after_static_getter_empty_body() {
        // Key function has empty body (returns undefined)
        let r = eval(
            r#"
            function f() {}
            class C {
                get [f()]() { return 1; }
                static get [f()]() { return 1; }
            }
            var c = new C();
            C[f()];
            c[f()]
            "#,
        );
        assert_eq!(
            r.unwrap(),
            Value::Number(1.0),
            "instance getter should return 1"
        );
    }

    #[test]
    fn class_instance_after_static_getter_explicit_undefined() {
        // Key function explicitly returns undefined
        let r = eval(
            r#"
            function f() { return undefined; }
            class C {
                get [f()]() { return 1; }
                static get [f()]() { return 1; }
            }
            var c = new C();
            C[f()];
            c[f()]
            "#,
        );
        assert_eq!(
            r.unwrap(),
            Value::Number(1.0),
            "explicit undefined key should work"
        );
    }

    #[test]
    fn class_instance_getter_two_calls() {
        // Instance getter called twice (no static)
        let r = eval(
            r#"
            function f() {}
            class C { get [f()]() { return 1; } }
            var c = new C();
            var a = c[f()];
            var b = c[f()];
            [a, b]
            "#,
        );
        let arr = r.unwrap();
        if let Value::Object(o) = arr {
            let obj = o.borrow();
            assert_eq!(obj.get("0"), Some(Value::Number(1.0)));
            assert_eq!(obj.get("1"), Some(Value::Number(1.0)));
        }
    }

    #[test]
    fn class_static_then_instance_then_static() {
        // Multiple alternating calls
        let r = eval(
            r#"
            function f() {}
            class C {
                get [f()]() { return 1; }
                static get [f()]() { return 1; }
            }
            var c = new C();
            [C[f()], c[f()], C[f()]]
            "#,
        );
        let arr = r.unwrap();
        if let Value::Object(o) = arr {
            let obj = o.borrow();
            assert_eq!(obj.get("0"), Some(Value::Number(1.0)));
            assert_eq!(obj.get("1"), Some(Value::Number(1.0)));
            assert_eq!(obj.get("2"), Some(Value::Number(1.0)));
        }
    }

    #[test]
    fn class_instance_getter_non_computed_key() {
        // Non-computed: using identifier `f` instead of `f()`
        let r = eval(
            r#"
            function f() {}
            class C {
                get [f]() { return 1; }
                static get [f]() { return 1; }
            }
            var c = new C();
            C[f];
            c[f]
            "#,
        );
        assert_eq!(
            r.unwrap(),
            Value::Number(1.0),
            "non-computed key should work"
        );
    }

    #[test]
    fn class_instance_getter_non_undefined_key() {
        // Key function returns a non-undefined string
        let r = eval(
            r#"
            function f() { return "x"; }
            class C {
                get [f()]() { return 1; }
                static get [f()]() { return 1; }
            }
            var c = new C();
            C[f()];
            c[f()]
            "#,
        );
        assert_eq!(r.unwrap(), Value::Number(1.0), "string key should work");
    }

    #[test]
    fn class_static_then_instance_empty_fn_key() {
        // Step 1: instance getter alone works?
        let r1 = eval(
            r#"
            function f() {}
            class C {
                get [f()]() { return 1; }
            }
            var c = new C();
            c[f()]
            "#,
        );
        assert!(r1.is_ok(), "instance getter alone should work: {:?}", r1);
        assert_eq!(
            r1.unwrap(),
            Value::Number(1.0),
            "instance getter alone should return 1"
        );

        // Step 2: static getter alone works?
        let r2 = eval(
            r#"
            function f() {}
            class C {
                static get [f()]() { return 1; }
            }
            C[f()]
            "#,
        );
        assert!(r2.is_ok(), "static getter alone should work: {:?}", r2);
        assert_eq!(
            r2.unwrap(),
            Value::Number(1.0),
            "static getter alone should return 1"
        );

        // Step 3: instance then static (should work)
        let r3 = eval(
            r#"
            function f() {}
            class C {
                get [f()]() { return 1; }
                static get [f()]() { return 1; }
            }
            var c = new C();
            [c[f()], C[f()]]
            "#,
        );
        assert!(r3.is_ok(), "instance then static should work: {:?}", r3);
        let arr3 = r3.unwrap();
        if let Value::Object(o) = arr3 {
            let obj = o.borrow();
            assert_eq!(
                obj.get("0"),
                Some(Value::Number(1.0)),
                "instance getter should return 1"
            );
            assert_eq!(
                obj.get("1"),
                Some(Value::Number(1.0)),
                "static getter should return 1"
            );
        } else {
            panic!("expected array result, got: {:?}", arr3);
        }

        // Step 4: static first, then instance (FAILS)
        let r = eval(
            r#"
            function f() {}
            class C {
                get [f()]() { return 1; }
                static get [f()]() { return 1; }
            }
            var c = new C();
            [C[f()], c[f()]]
            "#,
        );
        assert!(r.is_ok(), "Class getter access should work: {:?}", r);
        let arr = r.unwrap();
        if let Value::Object(o) = arr {
            let obj = o.borrow();
            assert_eq!(
                obj.get("0"),
                Some(Value::Number(1.0)),
                "static getter should return 1"
            );
            assert_eq!(
                obj.get("1"),
                Some(Value::Number(1.0)),
                "instance getter should return 1"
            );
        } else {
            panic!("expected array result, got: {:?}", arr);
        }
    }

    #[test]
    fn class_static_then_instance_non_computed_key() {
        // Same as above but with non-computed key: does it fail?
        let r = eval(
            r#"
            class C {
                get foo() { return 1; }
                static get foo() { return 1; }
            }
            var c = new C();
            [C.foo, c.foo]
            "#,
        );
        assert!(r.is_ok(), "non-computed key should work: {:?}", r);
        let arr = r.unwrap();
        if let Value::Object(o) = arr {
            let obj = o.borrow();
            assert_eq!(
                obj.get("0"),
                Some(Value::Number(1.0)),
                "static getter should return 1"
            );
            assert_eq!(
                obj.get("1"),
                Some(Value::Number(1.0)),
                "instance getter should return 1"
            );
        } else {
            panic!("expected array result, got: {:?}", arr);
        }
    }

    #[test]
    fn class_static_getter_then_instance_computed_different_keys() {
        // Static with computed key, instance with DIFFERENT key
        let r = eval(
            r#"
            function f() {}
            class C {
                get [f()]() { return 1; }
                static get foo() { return 2; }
            }
            var c = new C();
            [C.foo, c[f()]]
            "#,
        );
        assert!(r.is_ok(), "different keys should work: {:?}", r);
        let arr = r.unwrap();
        if let Value::Object(o) = arr {
            let obj = o.borrow();
            assert_eq!(
                obj.get("0"),
                Some(Value::Number(2.0)),
                "static foo should be 2"
            );
            assert_eq!(
                obj.get("1"),
                Some(Value::Number(1.0)),
                "instance f() should be 1"
            );
        } else {
            panic!("expected array result, got: {:?}", arr);
        }
    }

    // ── Diagnostic tests ─────────────────────────────────────────────────────

    #[test]
    fn class_instance_then_static_same_computed_key() {
        // Instance FIRST, then static, same computed key [f()]
        let r = eval(
            r#"
            function f() {}
            class C {
                get [f()]() { return 1; }
                static get [f()]() { return 1; }
            }
            var c = new C();
            [c[f()], C[f()]]
            "#,
        );
        assert!(
            r.is_ok(),
            "instance then static same key should work: {:?}",
            r
        );
        let arr = r.unwrap();
        if let Value::Object(o) = arr {
            let obj = o.borrow();
            assert_eq!(
                obj.get("0"),
                Some(Value::Number(1.0)),
                "instance getter should return 1"
            );
            assert_eq!(
                obj.get("1"),
                Some(Value::Number(1.0)),
                "static getter should return 1"
            );
        } else {
            panic!("expected array result, got: {:?}", arr);
        }
    }

    #[test]
    fn class_computed_accessors_function_decl_g3_only_v2() {
        // Test computed property with function object as key
        let r = eval(
            r#"
            function f() {}
            class C {
              get [f]() { return 1; }
              set [f](v) { return 1; }
              static get [f]() { return 1; }
              static set [f](v) { return 1; }
            }
            var c = new C();
            c[f]
            "#,
        );
        assert!(r.is_ok(), "c[f] should work: {:?}", r);
        assert_eq!(r.unwrap(), Value::Number(1.0), "c[f] should return 1");
    }

    #[test]
    fn class_computed_accessors_function_decl_g3_after_g1() {
        // g1 first, then g3
        let r = eval(
            r#"
            function f() {}
            class C {
              get [f]() { return 1; }
              set [f](v) { return 1; }
              static get [f]() { return 1; }
              static set [f](v) { return 1; }
            }
            var c = new C();
            var g1 = c[f];
            var g3 = c[f];
            [g1, g3]
            "#,
        );
        assert!(r.is_ok(), "g1 then g3 should work: {:?}", r);
        let arr = r.unwrap();
        if let Value::Object(o) = arr {
            let obj = o.borrow();
            assert_eq!(
                obj.get("0"),
                Some(Value::Number(1.0)),
                "g1 = c[f] should be 1"
            );
            assert_eq!(
                obj.get("1"),
                Some(Value::Number(1.0)),
                "g3 = c[f] should be 1"
            );
        } else {
            panic!("expected array");
        }
    }

    #[test]
    fn class_computed_accessors_function_decl_g3_after_g1_s1_g2_s2() {
        // Exact sequence: g1, s1, g2, s2, then g3
        let r = eval(
            r#"
            function f() {}
            class C {
              get [f]() { return 1; }
              set [f](v) { return 1; }
              static get [f]() { return 1; }
              static set [f](v) { return 1; }
            }
            var c = new C();
            var g1 = c[f];
            var s1 = c[f] = 1;
            var g2 = C[f];
            var s2 = C[f] = 1;
            var g3 = c[f];
            [g1, s1, g2, s2, g3]
            "#,
        );
        assert!(r.is_ok(), "g1,s1,g2,s2,g3 sequence should work: {:?}", r);
        let arr = r.unwrap();
        if let Value::Object(o) = arr {
            let obj = o.borrow();
            assert_eq!(
                obj.get("0"),
                Some(Value::Number(1.0)),
                "g1 = c[f] should be 1"
            );
            assert_eq!(
                obj.get("1"),
                Some(Value::Number(1.0)),
                "s1 = c[f] = 1 should be 1"
            );
            assert_eq!(
                obj.get("2"),
                Some(Value::Number(1.0)),
                "g2 = C[f] should be 1"
            );
            assert_eq!(
                obj.get("3"),
                Some(Value::Number(1.0)),
                "s2 = C[f] = 1 should be 1"
            );
            assert_eq!(
                obj.get("4"),
                Some(Value::Number(1.0)),
                "g3 = c[f] should be 1"
            );
        } else {
            panic!("expected array");
        }
    }

    #[test]
    fn class_computed_accessors_function_decl_g3_after_s1() {
        // s1 (assignment) first, then g3
        let r = eval(
            r#"
            function f() {}
            class C {
              get [f()]() { return 1; }
              set [f()](v) { return 1; }
              static get [f()]() { return 1; }
              static set [f()](v) { return 1; }
            }
            var c = new C();
            var s1 = c[f()] = 1;
            var g3 = c[String(f())];
            [s1, g3]
            "#,
        );
        assert!(r.is_ok(), "s1 then g3 should work: {:?}", r);
        let arr = r.unwrap();
        if let Value::Object(o) = arr {
            let obj = o.borrow();
            assert_eq!(
                obj.get("0"),
                Some(Value::Number(1.0)),
                "s1 = c[f()] = 1 should be 1"
            );
            assert_eq!(
                obj.get("1"),
                Some(Value::Number(1.0)),
                "g3 = c[String(f())] should be 1"
            );
        } else {
            panic!("expected array");
        }
    }

    #[test]
    fn class_computed_accessors_function_decl_g3_only() {
        // Narrowed: just g3 = c[f] with function object as key
        let r = eval(
            r#"
            function f() {}
            class C {
              get [f]() { return 1; }
              set [f](v) { return 1; }
              static get [f]() { return 1; }
              static set [f](v) { return 1; }
            }
            var c = new C();
            c[f]
            "#,
        );
        assert!(r.is_ok(), "c[f] should work: {:?}", r);
        assert_eq!(r.unwrap(), Value::Number(1.0), "c[f] should return 1");
    }

    // super numeric instance method — covered by test262

    // ─── is_constructor_value: bound function ─────────────────────────────

    // bound function extends — covered by test262

    #[test]
    fn class_super_numeric_static_method() {
        // static 4() { return super[4](); } - static methods use super on class itself
        let r = eval(
            r#"
            class B {
              static 4() { return 4; }
              static get 5() { return 5; }
            }
            class C extends B {
              static 4() { return super[4](); }
              static get 5() { return super[5]; }
            }
            [C[4](), C[5]]
            "#,
        );
        assert!(
            r.is_ok(),
            "super numeric static method should work: {:?}",
            r
        );
        let arr = r.unwrap();
        if let Value::Object(o) = arr {
            let obj = o.borrow();
            assert_eq!(obj.get("0"), Some(Value::Number(4.0)));
            assert_eq!(obj.get("1"), Some(Value::Number(5.0)));
        } else {
            panic!("expected array, got {:?}", arr);
        }
    }
}
