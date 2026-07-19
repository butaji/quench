//! Call expression evaluation
//!
//! Handles evaluation of function calls, constructor calls (new),
//! super() calls, and member access expressions.

use crate::ast::*;
use crate::env::Environment;
use crate::eval::class::{get_constructor_prototype, instantiate_class_from_ast_with_env};
use crate::eval::function::call_value_with_this;
use crate::eval::iteration::get_iterator;
use crate::eval::literal::{eval_property_key, get_super_value};
use crate::eval::member::eval_member_access;
use crate::interpreter::get_this_binding;
use crate::value::error::create_js_error_with_type;
use crate::value::{to_js_string, JsError, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate a call expression
pub fn eval_call(
    callee: &Expression,
    arguments: &[Expression],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    // Handle super() calls specially
    if let Expression::Identifier(name) = callee {
        if name == "super" {
            return eval_super_call(arguments, env, in_arrow_function);
        }
    }
    let (func, this_val, is_direct_eval) = crate::eval::object::eval_callee_with_this(callee, env)?;
    let args = eval_call_arguments(arguments, env, in_arrow_function)?;
    // Save the previous DIRECT_EVAL flag, then set for this call.
    // This ensures nested eval calls don't clobber the outer eval's flag.
    let prev_direct = crate::interpreter::is_direct_eval();
    crate::interpreter::set_direct_eval(is_direct_eval);
    let result = call_value_with_this(func, args, this_val);
    // Restore the previous flag (important for nested eval)
    crate::interpreter::set_direct_eval(prev_direct);
    result
}

/// Evaluate call arguments, expanding spread expressions
pub fn eval_call_arguments(
    arguments: &[Expression],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Vec<Value>, JsError> {
    let mut result = Vec::new();
    for arg in arguments.iter() {
        match arg {
            Expression::Spread(expr) => {
                let spread_val =
                    crate::eval::expression::eval_expression(expr, env, in_arrow_function)?;
                let items = get_iterator(&spread_val)?;
                result.extend(items);
            }
            _ => {
                let val = crate::eval::expression::eval_expression(arg, env, in_arrow_function)?;
                result.push(val);
            }
        }
    }
    Ok(result)
}

/// Evaluate a super() call - invokes the parent constructor
fn eval_super_call(
    arguments: &[Expression],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let super_val = get_super_value(env).ok_or_else(|| {
        JsError("ReferenceError: super is only valid in class methods".to_string())
    })?;
    let args = eval_call_arguments(arguments, env, in_arrow_function)?;
    let this_val = get_this_binding(env);

    // Capture the inner Object for field init (after this_val is consumed
    // by the super constructor call below).
    let this_obj = match &this_val {
        Value::Object(o) => Some(Rc::clone(o)),
        _ => None,
    };

    // Per ES §13.2.6.1 SuperCall: invoke the super constructor FIRST (this
    // may run user code that increments counters etc.), THEN check whether
    // `this` was already initialized. The check throws ReferenceError after
    // super() side-effects have already happened.
    let result = match super_val {
        Value::Class(super_class) => {
            crate::eval::class::call_super_constructor(super_class, args, this_val, env)
        }
        Value::Object(o) => {
            if let Some(Value::Function(constructor)) = o.borrow().get("constructor") {
                call_value_with_this(Value::Function(constructor.clone()), args, this_val)
            } else {
                Ok(Value::Undefined)
            }
        }
        Value::NativeConstructor(nc) => {
            call_value_with_this(Value::NativeConstructor(nc.clone()), args, this_val)
        }
        _ => {
            let (_, js_err) = create_js_error_with_type("super is not a constructor", "TypeError");
            return Err(js_err);
        }
    };

    // After super() ran, check the lexical this-binding status. Per ES
    // §8.1.1.3.1 BindThisValue, if `this` was already initialized, throw.
    let mut current: Option<Rc<RefCell<Environment>>> = Some(Rc::clone(env));
    while let Some(e) = current {
        if e.borrow()
            .scopes
            .iter()
            .any(|s| s.borrow().is_this_initialized())
        {
            return Err(JsError(
                "ReferenceError: super() called after `this` was already initialized".to_string(),
            ));
        }
        current = e.borrow().get_parent();
    }

    // Mark `this` as initialized on the current scope now that super()
    // succeeded, per ES §13.2.6.1 SuperCall step 7.
    env.borrow_mut()
        .current_scope()
        .borrow_mut()
        .mark_this_initialized();

    // After super() succeeds, run pending field initializers (for derived
    // classes with instance fields) before returning to the constructor body.
    // Per ES §13.2.6.1 SuperCall, fields are initialized right after super()
    // returns and before the rest of the constructor body.
    // NOTE: separate let-bind from if-let to avoid temporary scope extension
    // keeping the RefMut borrow alive into the body.
    let pending = env.borrow_mut().take_pending_fields();
    if let Some(fields) = pending {
        if let Some(ref obj_rc) = this_obj {
            for (prop_key, expr) in fields {
                let val = crate::eval::expression::eval_expression(&expr, env, in_arrow_function)?;
                let key_str = eval_property_key(&prop_key, env, in_arrow_function)?;
                obj_rc.borrow_mut().set(&key_str, val);
            }
        }
    }

    result
}

/// Evaluate a member access expression (obj.prop or obj[expr])
pub fn eval_member(
    object: &Expression,
    property: &PropertyKey,
    computed: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    // Handle super.property specially
    if let Expression::Identifier(id) = object {
        if id == "super" {
            return eval_super_member(property, computed, env, in_arrow_function);
        }
    }
    let prop_name = eval_property_key(property, env, in_arrow_function)?;

    // For identifiers, use get_shared to preserve function identity for property access
    if let Expression::Identifier(name) = object {
        if let Some(rc) = env.borrow().get_shared(name) {
            let obj_val = (*rc).clone();
            return eval_member_access(&obj_val, &prop_name, env);
        }
    }

    let obj_val = crate::eval::expression::eval_expression(object, env, in_arrow_function)?;
    eval_member_access(&obj_val, &prop_name, env)
}

/// Evaluate super.property - accesses methods on the superclass prototype
fn eval_super_member(
    property: &PropertyKey,
    _computed: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let super_val = get_super_value(env).ok_or_else(|| {
        JsError("ReferenceError: super is only valid in class methods".to_string())
    })?;
    let prop_name = eval_property_key(property, env, in_arrow_function)?;

    // Get the prototype of the superclass
    let proto = match &super_val {
        Value::Class(class) => crate::eval::class::get_or_create_class_prototype(class, env)?,
        Value::Object(o) => {
            let proto_val = o.borrow().get("prototype");
            match proto_val {
                Some(Value::Object(proto_obj)) => proto_obj.clone(),
                _ => {
                    let mut p = Object::new(ObjectKind::Ordinary);
                    p.set("constructor", Value::Object(Rc::clone(o)));
                    Rc::new(RefCell::new(p))
                }
            }
        }
        _ => return Ok(Value::Undefined),
    };

    // Look up property on the prototype chain
    let mut current: Option<Rc<RefCell<Object>>> = Some(proto);
    while let Some(obj_rc) = current {
        let obj = obj_rc.borrow();
        // Check getter first
        if let Some(getter_storage) = obj.get_getter(&prop_name) {
            let getter_clone = getter_storage.clone();
            drop(obj);
            let env = Rc::new(RefCell::new(Environment::new()));
            return crate::eval::object::call_getter(&obj_rc, &getter_clone, &env);
        }
        // Check regular property
        if let Some(val) = obj.properties.get(&prop_name) {
            return Ok(val.clone());
        }
        // Check array elements
        if let Ok(idx) = prop_name.parse::<usize>() {
            if idx < obj.elements.len() {
                return Ok(obj.elements[idx].clone());
            }
        }
        current = obj.prototype.as_ref().map(Rc::clone);
    }

    Ok(Value::Undefined)
}

/// Evaluate a new expression (constructor call)
pub fn eval_new(
    constructor: &Expression,
    arguments: &[Expression],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let constructor_val =
        crate::eval::expression::eval_expression(constructor, env, in_arrow_function)?;

    // Arrow functions are not constructors
    if let Value::Function(ref f) = constructor_val {
        if f.is_arrow {
            let (_, js_err) =
                create_js_error_with_type("function is not a constructor", "TypeError");
            return Err(js_err);
        }
    }

    let args = eval_call_arguments(arguments, env, in_arrow_function)?;

    // Per ES §13.2.6 GetNewTarget: while this constructor runs, `new.target`
    // resolves to the constructor being invoked. Capture the constructor value
    // here and restore on every exit path.
    let prev_new_target = crate::interpreter::get_new_target();
    crate::interpreter::set_new_target(Some(constructor_val.clone()));

    // Handle class instantiation
    if let Value::Class(class) = &constructor_val {
        let r = instantiate_class_from_ast_with_env(class.clone(), args, env);
        crate::interpreter::set_new_target(prev_new_target);
        return r;
    }

    let actual_constructor = match &constructor_val {
        Value::Object(o) => {
            let obj = o.borrow();
            if let Some(constructor) = obj.get("constructor") {
                constructor.clone()
            } else {
                let (_, js_err) =
                    create_js_error_with_type("object is not a constructor", "TypeError");
                return Err(js_err);
            }
        }
        other => other.clone(),
    };

    // NativeFunction without an explicit prototype is not a constructor
    if let Value::NativeFunction(ref nf) = actual_constructor {
        let has_prototype = nf.prototype.borrow().is_some();
        if !has_prototype {
            let (_, js_err) =
                create_js_error_with_type("function is not a constructor", "TypeError");
            return Err(js_err);
        }
    }

    let prototype = get_constructor_prototype(&constructor_val)?;
    let new_obj = if let Some(proto) = prototype {
        Object::with_prototype(ObjectKind::Ordinary, proto)
    } else {
        Object::new(ObjectKind::Ordinary)
    };

    let new_obj_rc = Rc::new(RefCell::new(new_obj));
    let result = call_value_with_this(
        actual_constructor.clone(),
        args,
        Value::Object(Rc::clone(&new_obj_rc)),
    )?;

    // Per ES spec §10.2.2 [[Construct]]: if the constructor returns undefined,
    // set the [[Name]] property of the function as the "name" property on the
    // new object (via CreateMethodProperty). This makes `new CustomError().name`
    // equal "CustomError" for user-defined constructors.
    if let Value::Function(ref f) = actual_constructor {
        if let Some(name) = &f.name {
            new_obj_rc
                .borrow_mut()
                .set("name", Value::String(name.clone()));
        }
    }

    // Check whether to use the constructor result
    let use_constructor_result = match &actual_constructor {
        Value::NativeConstructor(_) => true,
        Value::NativeFunction(_) => true,
        Value::Function(f) => f.body.iter().any(Statement::has_explicit_return),
        _ => false,
    };

    // Per spec, a constructor result is used only if it is an object (functions
    // and native functions are objects too).
    let final_result = if use_constructor_result
        && matches!(
            result,
            Value::Object(_)
                | Value::Function(_)
                | Value::NativeFunction(_)
                | Value::NativeConstructor(_)
                | Value::Class(_)
        ) {
        Ok(result)
    } else {
        Ok(Value::Object(new_obj_rc))
    };

    // Restore outer new.target before returning.
    crate::interpreter::set_new_target(prev_new_target);
    final_result
}

/// Extract a property name from a PropertyKey, evaluating computed keys
pub fn extract_property_name(
    key: PropertyKey,
    computed: bool,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<String, JsError> {
    match key {
        PropertyKey::Ident(name) => {
            if computed {
                let val = crate::eval::expression::eval_expression(
                    &Expression::Identifier(name),
                    env,
                    in_arrow_function,
                )?;
                match &val {
                    Value::Symbol(s) => {
                        Ok(s.desc.clone().map(|d| d.to_string()).unwrap_or_default())
                    }
                    _ => Ok(to_js_string(&val)),
                }
            } else {
                Ok(name)
            }
        }
        PropertyKey::String(s) => Ok(s),
        PropertyKey::Number(n) => Ok(n.to_string()),
        PropertyKey::Computed(expr) => {
            let val = crate::eval::expression::eval_expression(&expr, env, in_arrow_function)?;
            match &val {
                Value::Symbol(s) => Ok(s.desc.clone().map(|d| d.to_string()).unwrap_or_default()),
                _ => Ok(to_js_string(&val)),
            }
        }
    }
}
