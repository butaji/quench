//! Function calls

use crate::ast::{ArrowBody, BindingElement, Expression, Statement};
use crate::builtins::symbol::new_symbol as create_symbol;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::statement::{eval_function_body, take_tail_call_signal};
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
    call_value_impl(func, args, this_val, false)
}

/// Like `call_value_with_this` but with a `force_strict` flag that forces
/// Value::Function calls to run in strict mode (ES §10.4.3: getter/setter call site).
pub(crate) fn call_value_impl(
    func: Value,
    args: Vec<Value>,
    this_val: Value,
    force_strict: bool,
) -> Result<Value, JsError> {
    match check_depth() {
        Ok(_) => {}
        Err(e) => {
            release_depth();
            return Err(e);
        }
    }

    let result = match func {
        Value::Function(f) => {
            if f.is_async && !f.is_generator {
                // For async (non-generator) functions, ANY error (including parameter
                // evaluation) must be caught and returned as a rejected Promise instead
                // of propagating as an uncaught exception.
                let inner = call_js_function_impl(f, args, this_val);
                let proto = crate::builtins::promise::get_promise_proto();
                match inner {
                    Ok(val) => {
                        crate::builtins::promise::promise_resolve_impl_static(vec![val], proto)
                    }
                    Err(err) => crate::builtins::promise::promise_reject_impl_static(
                        vec![Value::String(err.to_string())],
                        proto,
                    ),
                }
            } else if f.is_async && f.is_generator {
                // Async generator: evaluate params eagerly at call time (ES §15.8.1).
                // Errors in default param evaluation must throw synchronously before
                // the generator object is returned.
                // We evaluate params in a throwaway environment just to catch errors.
                let eval_env = Environment::with_parent(Rc::clone(&f.closure));
                let eval_env_rc = Rc::new(RefCell::new(eval_env));
                bind_params(&f, &f.params, &args, &eval_env_rc)?;

                let mut gen_obj = crate::value::GeneratorObject::new(
                    f.body.clone(),
                    f.params.clone(),
                    Rc::clone(&f.closure),
                    f.strict,
                );
                gen_obj.is_async = true;
                // Store evaluated args so they can be bound when generator starts
                gen_obj.args = Some(args);
                Ok(Value::Generator(Rc::new(RefCell::new(gen_obj))))
            } else if f.is_generator {
                // Sync generator: FunctionDeclarationInstantiation (incl. param binding)
                // runs synchronously at [[Call]] before returning the generator object.
                let call_env = Environment::with_parent(Rc::clone(&f.closure));
                if !f.is_arrow {
                    call_env
                        .current_scope()
                        .borrow_mut()
                        .set_this(this_val.clone());
                }
                let call_env_rc = Rc::new(RefCell::new(call_env));
                if !f.is_arrow {
                    let args_obj =
                        crate::eval::class::helpers::create_arguments_object_simple(args.clone());
                    call_env_rc
                        .borrow_mut()
                        .define("arguments".to_string(), args_obj);
                }
                bind_params(&f, &f.params, &args, &call_env_rc)?;

                let mut gen_obj = crate::value::GeneratorObject::new(
                    f.body.clone(),
                    f.params.clone(),
                    Rc::clone(&f.closure),
                    f.strict,
                );
                gen_obj.args = Some(args);
                gen_obj.call_env = Some(call_env_rc);
                Ok(Value::Generator(Rc::new(RefCell::new(gen_obj))))
            } else if force_strict {
                call_js_function_impl_with_strict(f, args, this_val, true)
            } else {
                call_js_function_impl(f, args, this_val)
            }
        }
        Value::NativeFunction(nf) => call_native_function(nf, args, this_val),
        Value::NativeConstructor(nc) => call_native_constructor(nc, args, this_val),
        Value::Object(o) => call_object_as_constructor(o, args, this_val),
        Value::Class(class) => {
            if this_val != Value::Undefined {
                if crate::eval::class::helpers::constructing_class_for_super().is_some() {
                    return crate::eval::class::call_super_constructor(
                        *class,
                        args,
                        this_val,
                        &Rc::new(RefCell::new(Environment::new())),
                    );
                }
                let (_, js_err) = crate::value::error::create_js_error_with_type(
                    "Class constructor cannot be invoked without 'new'",
                    "TypeError",
                );
                return Err(js_err);
            }
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                "Class constructor cannot be invoked without 'new'",
                "TypeError",
            );
            Err(js_err)
        }
        _ => {
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
    call_js_function_impl_with_strict(f, args, this_val, false)
}

/// Like `call_js_function_impl` but with a `force_strict` flag that overrides
/// the function's own strictness. Used by `call_getter` (ES §10.4.3) where
/// getter functions must execute in the strict mode of the call site.
/// Uses a trampoline loop to implement proper tail-call optimization: when
/// `eval_function_body` sets a tail-call signal, this function re-enters
/// with the new function without growing the Rust call stack.
pub(crate) fn call_js_function_impl_with_strict(
    mut f: ValueFunction,
    mut args: Vec<Value>,
    mut this_val: Value,
    mut force_strict: bool,
) -> Result<Value, JsError> {
    // in_tail_chain is set once we enter the loop (first call made it here).
    // The acc_stack is only meaningful within a tail-call chain: it holds
    // the "accumulated" completion value at each call frame. When a function
    // returns from a non-tail position, its completion value is combined
    // with whatever is already in the acc slot (per ES §14.2.1 semantics).
    //
    // Each loop iteration pushes one placeholder onto acc_stack. When the
    // function returns, we pop back to the saved depth — this prevents
    // inner non-tail calls from removing outer placeholders. For example:
    //   f() { var x = g(); return x + 1; }
    // g() pushes/pops its own placeholder; f() then pops its own.
    loop {
        let starting_depth = crate::eval::statement::acc_stack_len();
        crate::eval::statement::acc_stack_push(create_symbol("TCO_PLACEHOLDER"));

        let closure = Rc::clone(&f.closure);
        let params = f.params.clone();

        let function_is_strict = f.strict;
        let body_is_strict = check_use_strict(&f.body);
        let in_strict = function_is_strict || body_is_strict || force_strict;

        this_val = if in_strict {
            this_val
        } else {
            box_sloppy_this(this_val)
        };

        let call_env = Environment::with_parent(closure);
        if !f.is_arrow {
            call_env
                .current_scope()
                .borrow_mut()
                .set_this(this_val.clone());
            let target = crate::interpreter::get_new_target().unwrap_or(Value::Undefined);
            call_env
                .current_scope()
                .borrow_mut()
                .define("new.target".to_string(), target);
        }
        let call_env_rc = Rc::new(RefCell::new(call_env));

        if !f.is_arrow {
            let args_obj = create_arguments_object(&f, args.clone(), in_strict);
            call_env_rc
                .borrow_mut()
                .define("arguments".to_string(), args_obj);
        }

        bind_params(&f, &params, &args, &call_env_rc)?;

        let body_env_rc = function_body_env(&call_env_rc, &f, &this_val, &params);
        body_env_rc.borrow_mut().push_scope();
        predeclare_var(&f.body, &mut body_env_rc.borrow_mut());
        predeclare_let_const(&f.body, &mut body_env_rc.borrow_mut());

        let prev_strict = crate::interpreter::is_strict_mode();
        crate::interpreter::set_strict_mode(in_strict);
        let previous_eval_env = crate::interpreter::get_current_eval_env();
        crate::interpreter::set_current_eval_env(Some(Rc::clone(&body_env_rc)));
        let result = if f.is_arrow {
            call_arrow_body(&f, &body_env_rc)
        } else {
            eval_function_body(&f.body, &body_env_rc, false)
        };
        crate::interpreter::set_current_eval_env(previous_eval_env);
        crate::interpreter::set_strict_mode(prev_strict);

        let Some(tail) = take_tail_call_signal() else {
            // No tail call: pop to starting_depth. After truncation, the slot
            // this frame pushed is gone, and whatever the caller accumulated
            // (result of inner non-tail calls) is now at the top of the stack.
            crate::eval::statement::acc_stack_pop_to(starting_depth);
            // Clear any stale ControlFlow::Return signal so the caller
            // (eval_function_body) doesn't short-circuit on a non-tail result.
            crate::interpreter::take_control_flow();

            let result_val = result?;
            // Push our result after the accumulated value (if any).
            // If starting_depth == 0 (outermost call), the stack is empty
            // and we just return. If starting_depth > 0, the caller
            // will read our result as its accumulated value.
            if starting_depth > 0 {
                crate::eval::statement::acc_stack_push(result_val.clone());
            }
            return Ok(result_val);
        };

        // Tail call: store this frame's result in the acc stack slot,
        // then set up for the next function and loop.
        crate::eval::statement::acc_stack_update_last(result.unwrap_or(Value::Undefined));
        let next_force_strict = tail.function.strict || check_use_strict(&tail.function.body);
        f = tail.function;
        this_val = tail.this_val;
        force_strict = next_force_strict;
        args = tail.arguments;
    }
}

/// True when formal parameters need a separate body environment (ES
/// `hasParameterExpressions`: defaults, destructuring, or rest).
pub(crate) fn has_parameter_expressions(params: &[crate::ast::Param]) -> bool {
    params
        .iter()
        .any(|p| p.default.is_some() || p.pattern.is_some() || p.rest)
}

/// Body lexical environment: child param record when parameter expressions
/// exist, otherwise the same environment used for parameter binding.
pub(crate) fn function_body_env(
    param_env_rc: &Rc<RefCell<Environment>>,
    f: &ValueFunction,
    this_val: &Value,
    params: &[crate::ast::Param],
) -> Rc<RefCell<Environment>> {
    if !has_parameter_expressions(params) {
        return Rc::clone(param_env_rc);
    }
    let body_env = Environment::with_parent(Rc::clone(param_env_rc));
    if !f.is_arrow {
        body_env
            .current_scope()
            .borrow_mut()
            .set_this(this_val.clone());
    }
    Rc::new(RefCell::new(body_env))
}

/// Bind parameters from `args` into the call environment.
/// Params with default expressions are declared in TDZ before evaluating the
/// default so self-references like `f(x = x)` throw ReferenceError.
pub(crate) fn bind_params(
    f: &ValueFunction,
    params: &[crate::ast::Param],
    args: &[Value],
    call_env_rc: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let mut found_rest = false;
    for (i, param) in params.iter().enumerate() {
        if found_rest {
            call_env_rc
                .borrow_mut()
                .define(param.name.clone(), Value::Undefined);
            continue;
        }

        if param.rest {
            let rest_value = Value::Object(Rc::new(RefCell::new(Object::new_array_from(
                args.get(i..).unwrap_or_default().to_vec(),
            ))));
            found_rest = true;
            if let Some(pattern) = &param.pattern {
                declare_pattern_bindings(pattern, call_env_rc);
                let target = binding_pattern_expression(pattern.clone());
                crate::eval::object::assign_to(&target, &rest_value, call_env_rc)?;
            } else {
                call_env_rc
                    .borrow_mut()
                    .define(param.name.clone(), rest_value);
            }
        } else {
            let arg = args.get(i).cloned();
            // Declare params with defaults in TDZ before evaluating default expressions.
            if param.default.is_some() {
                call_env_rc
                    .borrow_mut()
                    .declare_var(param.name.clone(), crate::ast::VarKind::Let);
            }

            let value = match arg {
                Some(Value::Undefined) if param.default.is_some() => {
                    eval_expression(param.default.as_ref().unwrap(), call_env_rc, f.is_arrow)?
                }
                Some(v) => v,
                None if param.default.is_some() => {
                    eval_expression(param.default.as_ref().unwrap(), call_env_rc, f.is_arrow)?
                }
                None => Value::Undefined,
            };

            if param.default.is_some() {
                if let Some(pattern) = &param.pattern {
                    declare_pattern_bindings(pattern, call_env_rc);
                    let target = binding_pattern_expression(pattern.clone());
                    crate::eval::object::assign_to(&target, &value, call_env_rc)?;
                } else {
                    call_env_rc
                        .borrow_mut()
                        .initialize_declared(&param.name, value);
                }
            } else if let Some(pattern) = &param.pattern {
                declare_pattern_bindings(pattern, call_env_rc);
                let target = binding_pattern_expression(pattern.clone());
                crate::eval::object::assign_to(&target, &value, call_env_rc)?;
            } else {
                call_env_rc.borrow_mut().define(param.name.clone(), value);
            }
        }
    }
    Ok(())
}

fn binding_pattern_expression(pattern: BindingElement) -> Expression {
    match pattern {
        BindingElement::Identifier(name) => Expression::Identifier(name),
        BindingElement::ArrayPattern(elements) => Expression::ArrayPattern(elements),
        BindingElement::ObjectPattern(properties) => Expression::ObjectPattern(properties),
        BindingElement::Default(binding, _) => binding_pattern_expression(*binding),
        BindingElement::Rest(binding) => binding_pattern_expression(*binding),
        BindingElement::AssignmentTarget(expr) => expr,
    }
}

fn declare_pattern_bindings(pattern: &BindingElement, env: &Rc<RefCell<Environment>>) {
    match pattern {
        BindingElement::Identifier(name) => env.borrow_mut().define(name.clone(), Value::Undefined),
        BindingElement::ArrayPattern(elements) => {
            for element in elements {
                declare_pattern_bindings(element, env);
            }
        }
        BindingElement::ObjectPattern(properties) => {
            for (_, binding) in properties {
                declare_pattern_bindings(binding, env);
            }
        }
        BindingElement::Default(binding, _) => declare_pattern_bindings(binding, env),
        BindingElement::Rest(binding) => declare_pattern_bindings(binding, env),
        BindingElement::AssignmentTarget(_) => {}
    }
}

/// Check if a function body starts with "use strict"; directive
fn check_use_strict(body: &[Statement]) -> bool {
    for stmt in body {
        match stmt {
            Statement::Expression(expr) => {
                if let Expression::String(s) = expr.as_ref() {
                    if s.trim() == "use strict" {
                        return true;
                    }
                } else {
                    // Non-string expression ends the directive prologue
                    return false;
                }
            }
            _ => return false, // Non-expression ends the directive prologue
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
        obj.set_getter("callee", Rc::new(throw_body), f.closure.clone(), false);
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
mod tests;
