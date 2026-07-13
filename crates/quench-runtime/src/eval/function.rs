//! Function calls

use crate::ast::{ArrowBody, Expression, Statement};
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::statement::eval_function_body;
use crate::interpreter::{check_depth, predeclare_let_const, predeclare_var, release_depth};
use crate::value::{
    JsError, NativeConstructor, NativeFunction, Object, ObjectKind, Value, ValueFunction,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Call a value as a function with an explicit "this" binding
pub fn call_value_with_this(
    func: Value,
    args: Vec<Value>,
    this_val: Value,
) -> Result<Value, JsError> {
    match check_depth() {
        Ok(_) => {}
        Err(e) => {
            release_depth();
            return Err(e);
        }
    }

    let result = match func {
        Value::Function(f) => call_js_function_impl(f, args, this_val),
        Value::NativeFunction(nf) => call_native_function(nf, args, this_val),
        Value::NativeConstructor(nc) => call_native_constructor(nc, args, this_val),
        Value::Object(o) => call_object_as_constructor(o, args, this_val),
        Value::Class(class) => {
            if this_val != Value::Undefined {
                crate::eval::class::call_super_constructor(
                    class,
                    args,
                    this_val,
                    &Rc::new(RefCell::new(Environment::new())),
                )
            } else {
                crate::eval::class::instantiate_class_from_ast(class, args)
            }
        }
        _ => {
            // Create a proper TypeError and set it as the thrown value
            let msg = format!("Value is not a function, got {:?}", func);
            let (err_val, js_err) =
                crate::value::error::create_js_error_with_type(&msg, "TypeError");
            crate::value::set_thrown_value(err_val);
            Err(js_err)
        }
    };
    release_depth();
    result
}

/// Call a value as a function (this defaults to undefined)
pub fn call_value(func: Value, args: Vec<Value>) -> Result<Value, JsError> {
    call_value_with_this(func, args, Value::Undefined)
}

/// Call a JavaScript function with an explicit this binding
/// This is exposed for use by Function.prototype.call/apply
pub fn call_js_function_with_this(
    f: ValueFunction,
    args: Vec<Value>,
    this_val: Value,
) -> Result<Value, JsError> {
    check_depth()?;
    let result = call_js_function_impl(f, args, this_val);
    release_depth();
    result
}

pub(crate) fn call_js_function_impl(
    f: ValueFunction,
    args: Vec<Value>,
    this_val: Value,
) -> Result<Value, JsError> {
    let closure = Rc::clone(&f.closure);
    let params = f.params.clone();

    // Strictness is captured where the function was DEFINED (f.strict), never
    // inherited from the call site: a sloppy function called from strict code
    // still gets the global object as `this` (ES spec 10.2.1.2).
    // Arrow functions are different: their strictness comes from the lexical
    // enclosing scope at definition time. We capture that via f.strict (set
    // at arrow creation in Expression::ArrowFunction).
    let function_is_strict = f.strict;
    // Check if function body starts with "use strict"; directive
    let body_is_strict = check_use_strict(&f.body);
    let in_strict = function_is_strict || body_is_strict;

    // Per ES spec 10.2.1.2: in sloppy mode, a primitive `this` is boxed
    // (ToObject) so the function sees a wrapper object. In strict mode (and
    // arrow functions), the value passes through unchanged.
    let this_val = if in_strict {
        this_val
    } else {
        box_sloppy_this(this_val)
    };

    let mut call_env = Environment::with_parent(Rc::clone(&closure));
    // Per ES §10.2.1, arrow functions capture `this` from their lexical
    // (closure) scope. Setting this on the new scope would override that
    // capture with the caller-supplied this. Skip for arrows.
    if !f.is_arrow {
        call_env.current_scope_mut().set_this(this_val);
    }
    // Per ES §13.2.6 GetNewTarget: bind `new.target` in the function's
    // environment for ordinary (non-arrow) functions so they can reference
    // it directly. Arrow functions inherit `new.target` via lexical scope
    // (the enclosing function's env binding), so we deliberately do NOT
    // bind it in arrow call_envs — that would shadow the captured value.
    if !f.is_arrow {
        let target = crate::interpreter::get_new_target().unwrap_or(Value::Undefined);
        call_env
            .current_scope_mut()
            .define("new.target".to_string(), target);
    }
    let call_env_rc = Rc::new(RefCell::new(call_env));

    // Handle parameters, stopping at rest parameter
    let mut found_rest = false;
    let mut rest_args: Vec<Value> = Vec::new();

    for (i, param) in params.iter().enumerate() {
        if found_rest {
            // After rest parameter, remaining params are ignored (rest collects all remaining args)
            // Just define them as undefined
            call_env_rc
                .borrow_mut()
                .define(param.name.clone(), Value::Undefined);
            continue;
        }

        if param.rest {
            // Collect all remaining arguments into an array
            rest_args = args[i..].to_vec();
            found_rest = true;
            call_env_rc.borrow_mut().define(
                param.name.clone(),
                Value::Object(Rc::new(RefCell::new(Object::new_array_from(rest_args)))),
            );
        } else {
            let arg = args.get(i).cloned();
            let value = match arg {
                Some(Value::Undefined) if param.default.is_some() => {
                    eval_expression(param.default.as_ref().unwrap(), &call_env_rc, f.is_arrow)?
                }
                Some(v) => v,
                None if param.default.is_some() => {
                    eval_expression(param.default.as_ref().unwrap(), &call_env_rc, f.is_arrow)?
                }
                None => Value::Undefined,
            };
            call_env_rc.borrow_mut().define(param.name.clone(), value);
        }
    }

    // Create arguments object for non-arrow functions
    if !f.is_arrow {
        let args_obj = create_arguments_object(&f, args, in_strict);
        call_env_rc
            .borrow_mut()
            .define("arguments".to_string(), args_obj);
        predeclare_var(&f.body, &mut call_env_rc.borrow_mut());
        predeclare_let_const(&f.body, &mut call_env_rc.borrow_mut());
    }

    // Set strict mode for function body evaluation
    let prev_strict = crate::interpreter::is_strict_mode();
    crate::interpreter::set_strict_mode(in_strict);

    let result = if f.is_arrow {
        call_arrow_body(&f, &call_env_rc)
    } else {
        eval_function_body(&f.body, &call_env_rc, false)
    };

    // Restore previous strict mode
    crate::interpreter::set_strict_mode(prev_strict);

    result
}

/// Check if a function body starts with "use strict"; directive
fn check_use_strict(body: &[Statement]) -> bool {
    if let Some(Statement::Expression(expr)) = body.first() {
        if let Expression::String(s) = expr.as_ref() {
            return s.trim() == "use strict";
        }
    }
    false
}

/// Box a primitive `this` value per ES spec 10.2.1.2 (sloppy mode only).
/// Object values pass through unchanged; null/undefined pass through too —
/// the globalThis coercion happens later in `get_this_binding`.
fn box_sloppy_this(this_val: Value) -> Value {
    match &this_val {
        Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::Symbol(_) => {
            crate::value::convert::to_object(&this_val)
        }
        _ => this_val,
    }
}

/// Create the JavaScript arguments object for a function call
fn create_arguments_object(f: &ValueFunction, args: Vec<Value>, strict_mode: bool) -> Value {
    let mut obj = Object::new(ObjectKind::Ordinary);
    // Set indexed arguments (arguments[0], arguments[1], etc.)
    for (i, arg) in args.iter().enumerate() {
        obj.set(&i.to_string(), arg.clone());
    }
    // Set length property
    obj.set("length", Value::Number(args.len() as f64));

    // Set callee property
    if strict_mode {
        // In strict mode, arguments.callee throws TypeError per ESMA-262 10.2.9
        // Throw a string error directly - to_js_string will preserve it as-is
        let throw_body = vec![Statement::Throw(Box::new(Expression::String(
            "TypeError: 'caller' and 'callee' are not allowed in strict mode".to_string(),
        )))];
        obj.set_getter("callee", Rc::new(throw_body), f.closure.clone());
    } else {
        // In non-strict mode, callee is the function itself
        obj.set("callee", Value::Function(f.clone()));
    }

    Value::Object(Rc::new(RefCell::new(obj)))
}

fn call_arrow_body(
    f: &ValueFunction,
    call_env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(arrow_body) = f.arrow_body.as_ref() {
        match arrow_body {
            ArrowBody::Expression(expr) => eval_expression(expr, call_env, true),
            ArrowBody::Block(stmts) => eval_function_body(stmts, call_env, true),
        }
    } else {
        Ok(Value::Undefined)
    }
}

pub(crate) fn call_native_function(
    nf: Rc<NativeFunction>,
    args: Vec<Value>,
    this_val: Value,
) -> Result<Value, JsError> {
    // Set both thread-locals: CURRENT_THIS for builtins to get the caller's 'this',
    // and CALL_THIS as backup. Call the closure directly (not nf.call()).
    crate::interpreter::set_native_this(this_val.clone());
    crate::interpreter::set_this_value(this_val);
    let result = (nf.func)(args);
    crate::interpreter::take_native_this();
    crate::interpreter::take_this_value();
    result
}

fn call_native_constructor(
    nc: Rc<NativeConstructor>,
    args: Vec<Value>,
    this_val: Value,
) -> Result<Value, JsError> {
    // For native constructors called via `new`, this_val is already the new object
    // created by eval_new. We just need to use it as `this`.
    let effective_this = this_val.clone();

    crate::interpreter::set_native_this(effective_this.clone());
    crate::interpreter::set_this_value(effective_this.clone());
    let result = nc.call_func(args);
    crate::interpreter::take_native_this();
    crate::interpreter::take_this_value();

    // Per ES spec: if constructor returns an object, use it; otherwise return primitive
    match result {
        Ok(Value::Object(_))
        | Ok(Value::Function(_))
        | Ok(Value::NativeFunction(_))
        | Ok(Value::NativeConstructor(_))
        | Ok(Value::Class(_)) => result,
        Err(e) => Err(e),
        Ok(other) => Ok(other), // return primitive (Number, String, Boolean, etc.)
    }
}

fn call_object_as_constructor(
    o: Rc<RefCell<Object>>,
    args: Vec<Value>,
    this_val: Value,
) -> Result<Value, JsError> {
    let constructor_opt = {
        let obj = o.borrow();
        if let Some(constructor) = obj.get("constructor") {
            if matches!(
                constructor,
                Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_)
            ) {
                Some(constructor.clone())
            } else {
                None
            }
        } else {
            None
        }
    };
    if let Some(constructor) = constructor_opt {
        // If this_val is undefined, this is a direct function call, not 'new'
        // For built-in constructors like Number, call with undefined 'this'
        if matches!(this_val, Value::Undefined) {
            return call_value_with_this(constructor, args, Value::Undefined);
        }

        // Otherwise, this is a 'new' expression - create a new object
        let new_obj = Object::new(ObjectKind::Ordinary);
        let new_obj_rc = Rc::new(RefCell::new(new_obj));
        {
            let proto = o.borrow().get("prototype");
            if proto.is_some() {
                new_obj_rc
                    .borrow_mut()
                    .set("constructor", Value::Object(Rc::clone(&o)));
            }
        }
        let result =
            call_value_with_this(constructor, args, Value::Object(Rc::clone(&new_obj_rc)))?;
        // If the constructor returns an object, use it; otherwise use new_obj
        if matches!(result, Value::Object(_)) {
            Ok(result)
        } else {
            Ok(Value::Object(new_obj_rc))
        }
    } else {
        // No .constructor — check for callable-object pattern: an `apply` method.
        // This handles harness objects like `assert` which are plain objects but
        // define `apply(thisArg, argsArray)` to be invoked when called directly.
        let apply_opt = {
            let obj = o.borrow();
            obj.get("apply")
        };

        if let Some(apply) = apply_opt {
            // Ignore this_val (use undefined) — the apply method decides what 'this' means.
            call_value_with_this(apply, args, Value::Undefined)
        } else {
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                "object is not a constructor",
                "TypeError",
            );
            Err(js_err)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Context;

    #[test]
    fn sloppy_call_boxes_primitive_this() {
        let mut ctx = Context::new().unwrap();
        let res = ctx
            .eval("function bar() { return typeof this; } bar.call(1);")
            .unwrap();
        assert_eq!(res, crate::value::Value::String("object".to_string()));
    }

    #[test]
    fn strict_body_keeps_primitive_this() {
        let mut ctx = Context::new().unwrap();
        let res = ctx
            .eval("function foo() { 'use strict'; return typeof this; } foo.call(1);")
            .unwrap();
        assert_eq!(res, crate::value::Value::String("number".to_string()));
    }

    #[test]
    fn sloppy_call_object_this_passes_through() {
        let mut ctx = Context::new().unwrap();
        let res = ctx
            .eval("var o = { name: 'x' }; function bar() { return typeof this; } bar.call(o);")
            .unwrap();
        assert_eq!(res, crate::value::Value::String("object".to_string()));
    }
}
