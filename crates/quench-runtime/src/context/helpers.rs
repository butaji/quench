//! Private helper functions for the context module.
//! All functions here are internal helpers; public API lives in the parent `mod.rs`.

use crate::ast;
use crate::env::Environment;
use crate::eval;
use crate::host;
use crate::value::{JsError, NativeFunction, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

// Re-export CURRENT_CONTEXT and Context from the parent module
pub use super::{Context, CURRENT_CONTEXT};

// Thread-local cache for single-character regex objects
thread_local! {
    static REGEX_CACHE: std::cell::RefCell<rustc_hash::FxHashMap<char, Value>> =
        std::cell::RefCell::new(rustc_hash::FxHashMap::default());
}

/// eval function implementation - executes JavaScript code in the current context.
/// Per ES spec §19.2.1, eval code inherits strict mode from its calling context.
/// We check for legacy octals here (before parsing the eval string) so that
/// eval in strict mode throws even when the eval string itself has no
/// "use strict" directive.
pub fn eval_impl(args: Vec<Value>, ctx: &mut Context) -> Result<Value, JsError> {
    let source = args
        .first()
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    if source.is_empty() {
        return Ok(Value::Undefined);
    }

    let strict_inherited = crate::interpreter::is_strict_mode();
    let has_octal = crate::interpreter::has_legacy_octal(&source);
    if strict_inherited && has_octal {
        let (err_val, js_err) = crate::value::error::create_js_error_with_type(
            "legacy octal literals are not allowed in strict mode",
            "SyntaxError",
        );
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    let ctx_ptr: *mut Context = ctx;
    let prev_ctx = CURRENT_CONTEXT.with(|cell| {
        let prev = cell.borrow();
        *prev
    });
    CURRENT_CONTEXT.with(|cell| {
        *cell.borrow_mut() = Some(ctx_ptr);
    });
    // Save label stack depth before parse so we can restore on any exit path.
    let label_depth = crate::interpreter::label_stack_depth();
    let program = match ctx.parse(&source) {
        Ok(program) => program,
        Err(e) => {
            CURRENT_CONTEXT.with(|cell| {
                *cell.borrow_mut() = prev_ctx;
            });
            let (err_val, js_err) =
                crate::value::error::create_js_error_with_type(&e.0, "SyntaxError");
            crate::value::set_thrown_value(err_val);
            return Err(js_err);
        }
    };
    reject_eval_var_lexical_conflict(&program, ctx)?;
    // Establish the eval barrier so eval code cannot see outer labels.
    // has_label will only search up to this depth.
    crate::interpreter::set_eval_barrier_depth(label_depth);
    crate::interpreter::push_label_scope();
    let result = if let Some(mut eval_env) = crate::interpreter::get_current_eval_env() {
        crate::interpreter::eval_program(&program, &mut eval_env, Some(&source), false)
    } else {
        let is_direct = crate::interpreter::is_direct_eval();
        let this_value = if is_direct && strict_inherited {
            Value::Undefined
        } else {
            ctx.env
                .borrow()
                .get("globalThis")
                .unwrap_or(Value::Undefined)
        };
        crate::interpreter::set_this_binding(&ctx.env, this_value);
        crate::interpreter::eval_program(&program, &mut ctx.env, Some(&source), false)
    };
    // Exit eval: restore label stack and clear barrier.
    crate::interpreter::pop_label_scope();
    crate::interpreter::clear_eval_barrier_depth();
    CURRENT_CONTEXT.with(|cell| {
        *cell.borrow_mut() = prev_ctx;
    });
    match result {
        Err(_) => {
            if let Some(thrown) = crate::value::get_thrown_value() {
                let msg = crate::value::to_js_string(&thrown);
                Err(JsError(msg))
            } else {
                Err(JsError("unknown eval error".to_string()))
            }
        }
        Ok(v) => Ok(v),
    }
}

pub fn reject_eval_var_lexical_conflict(
    program: &crate::ast::Program,
    ctx: &Context,
) -> Result<(), JsError> {
    let ast::Program::Script(body) = program;
    let mut names = Vec::new();
    crate::interpreter::collect_var_names_recursive(body, &mut names);
    let eval_env =
        crate::interpreter::get_current_eval_env().unwrap_or_else(|| Rc::clone(&ctx.env));
    for name in &names {
        if matches!(
            eval_env.borrow().get_kind(name),
            Some(ast::VarKind::Let | ast::VarKind::Const)
        ) {
            let (error, js_error) = crate::value::error::create_js_error_with_type(
                &format!("Identifier '{}' has already been declared", name),
                "SyntaxError",
            );
            crate::value::set_thrown_value(error);
            return Err(js_error);
        }
    }
    for name in names {
        let is_local = eval_env.borrow().current_scope().borrow().has(&name);
        if !is_local {
            eval_env
                .borrow_mut()
                .declare_var(name.clone(), ast::VarKind::Var);
            eval_env
                .borrow_mut()
                .initialize_declared(&name, Value::Undefined);
        }
    }
    Ok(())
}

/// Initialize built-in globals and functions
pub fn init_builtins(ctx: &mut Context) -> Result<(), JsError> {
    host::register_builtin_functions(ctx);
    init_commonjs(ctx)?;
    init_es_module_cache(ctx)?;
    init_js_globals(ctx)?;
    sync_globals_to_global_this(ctx);
    register_eval_function(ctx)?;
    Ok(())
}

pub fn init_commonjs(ctx: &mut Context) -> Result<(), JsError> {
    let exports = Object::new(ObjectKind::Ordinary);
    let exports_rc = Rc::new(RefCell::new(exports));
    let module_obj = Object::new(ObjectKind::Ordinary);
    let module_obj = Rc::new(RefCell::new(module_obj));
    module_obj
        .borrow_mut()
        .set("exports", Value::Object(Rc::clone(&exports_rc)));
    ctx.set_global("exports".to_string(), Value::Object(Rc::clone(&exports_rc)));
    ctx.set_global("module".to_string(), Value::Object(module_obj));
    Ok(())
}

pub fn init_es_module_cache(ctx: &mut Context) -> Result<(), JsError> {
    let module_cache = Object::new(ObjectKind::Ordinary);
    let module_cache_rc = Rc::new(RefCell::new(module_cache));
    ctx.set_global(
        "__quench_modules__".to_string(),
        Value::Object(Rc::clone(&module_cache_rc)),
    );
    if let Some(Value::Object(global_obj)) = ctx.get_global("globalThis") {
        global_obj.borrow_mut().set(
            "__quench_modules__",
            Value::Object(Rc::clone(&module_cache_rc)),
        );
    }
    Ok(())
}

pub fn init_js_globals(ctx: &mut Context) -> Result<(), JsError> {
    let global_obj = Object::new(ObjectKind::Global);
    let global_obj = Rc::new(RefCell::new(global_obj));
    global_obj.borrow_mut().define(
        "globalThis",
        Value::Object(Rc::clone(&global_obj)),
        crate::value::PropertyFlags {
            value: Some(Value::Object(Rc::clone(&global_obj))),
            writable: true,
            enumerable: false,
            configurable: true,
        },
    );
    ctx.set_global(
        "globalThis".to_string(),
        Value::Object(Rc::clone(&global_obj)),
    );

    let value_flags = crate::value::PropertyFlags {
        value: None,
        writable: false,
        enumerable: false,
        configurable: false,
    };
    ctx.set_global("undefined".to_string(), Value::Undefined);
    ctx.set_global("Infinity".to_string(), Value::Number(f64::INFINITY));
    ctx.set_global("NaN".to_string(), Value::Number(f64::NAN));

    let define_value_prop = |key: &str, val: Value, global_obj: &Rc<RefCell<Object>>| {
        let mut flags = value_flags.clone();
        flags.value = Some(val.clone());
        global_obj.borrow_mut().define(key, val, flags);
    };
    define_value_prop("undefined", Value::Undefined, &global_obj);
    define_value_prop("Infinity", Value::Number(f64::INFINITY), &global_obj);
    define_value_prop("NaN", Value::Number(f64::NAN), &global_obj);

    ctx.env
        .borrow_mut()
        .current_scope()
        .borrow_mut()
        .set_object_binding(Rc::clone(&global_obj));

    Ok(())
}

/// Sync all global bindings from the environment to globalThis.
pub fn sync_globals_to_global_this(ctx: &mut Context) {
    let Some(Value::Object(global_obj)) = ctx.get_global("globalThis") else {
        return;
    };
    let scopes = &ctx.env.borrow().scopes;
    if scopes.is_empty() {
        return;
    }
    let global_scope = scopes[0].borrow();
    for (name, value_rc) in global_scope.bindings() {
        let value = (**value_rc).clone();
        global_obj.borrow_mut().define(
            name.as_str(),
            value.clone(),
            crate::value::PropertyFlags {
                value: Some(value),
                writable: true,
                enumerable: false,
                configurable: true,
            },
        );
    }
}

/// Register the eval function as a global
pub fn register_eval_function(ctx: &mut Context) -> Result<(), JsError> {
    let eval_fn = NativeFunction::new_named("eval", |args: Vec<Value>| {
        let source = args
            .first()
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        if source.is_empty() {
            return Ok(Value::Undefined);
        }

        // Fast path: eval("/x/") or eval("/x/.source") — bypass OXC entirely.
        let sb = source.as_bytes();
        if sb.len() > 1 && sb[0] == b'/' {
            if let Some(es) = sb[1..].iter().position(|&b| b == b'/') {
                let pat = &source[1..][..es];
                let has_named_group = pat.contains("(?<");
                let has_line_term = pat.contains('\n')
                    || pat.contains('\r')
                    || pat.contains('\u{2028}')
                    || pat.contains('\u{2029}');
                let bad_first_char = pat
                    .as_bytes()
                    .first()
                    .is_some_and(|&b| matches!(b, b'*' | b'/' | b'['));
                let after = &source[1..][es + 1..];
                let clean = !has_line_term
                    && !bad_first_char
                    && !has_named_group
                    && (after.is_empty()
                        || after == ".source"
                        || after
                            .bytes()
                            .all(|b| matches!(b, b'g' | b'i' | b'm' | b's' | b'u' | b'y' | b'd'))
                        || {
                            let dot_idx = after.find('.');
                            dot_idx.is_some_and(|i| {
                                after[..i].bytes().all(|b| {
                                    matches!(b, b'g' | b'i' | b'm' | b's' | b'u' | b'y' | b'd')
                                }) && &after[i..] == ".source"
                            })
                        });
                if clean {
                    let flags = if let Some(d) = after.find('.') {
                        &after[..d]
                    } else {
                        after
                    };
                    if flags
                        .bytes()
                        .all(|b| matches!(b, b'g' | b'i' | b'm' | b's' | b'u' | b'y' | b'd'))
                    {
                        if flags.is_empty() && pat.len() == 1 {
                            let ch = pat.as_bytes()[0] as char;
                            let cached = REGEX_CACHE.with(|c| c.borrow().get(&ch).cloned());
                            if let Some(v) = cached {
                                return if after == ".source" {
                                    if let Value::Object(o) = &v {
                                        if let Some(src) = o.borrow().properties.get("source") {
                                            return Ok(src.clone());
                                        }
                                    }
                                    Ok(v)
                                } else {
                                    Ok(v)
                                };
                            }
                        }
                        if after.ends_with(".source") {
                            return Ok(Value::String(pat.to_string()));
                        }
                        if flags.is_empty() {
                            let po = pat.to_string();
                            let mut obj = Object::new(ObjectKind::RegExp);
                            obj.properties
                                .insert("source".to_string(), Value::String(po.clone()));
                            obj.properties
                                .insert("global".to_string(), Value::Boolean(false));
                            obj.properties
                                .insert("ignoreCase".to_string(), Value::Boolean(false));
                            obj.properties
                                .insert("multiline".to_string(), Value::Boolean(false));
                            obj.properties
                                .insert("flags".to_string(), Value::String(String::new()));
                            obj.properties
                                .insert("lastIndex".to_string(), Value::Number(0.0));
                            let orc = Rc::new(RefCell::new(obj));
                            let proto = crate::builtins::regex::get_regexp_prototype();
                            orc.borrow_mut().prototype = Some(proto);
                            let val = Value::Object(orc);
                            if pat.len() == 1 {
                                let ch = pat.as_bytes()[0] as char;
                                REGEX_CACHE.with(|c| {
                                    c.borrow_mut().insert(ch, val.clone());
                                });
                            }
                            return Ok(val);
                        }
                        return crate::eval::literal::eval_regexp_literal(pat, flags);
                    }
                }
            }
        }

        let ctx_ptr = CURRENT_CONTEXT.with(|cell| cell.borrow().unwrap_or_else(std::ptr::null_mut));
        if ctx_ptr.is_null() {
            return Err(JsError("eval called outside of context".to_string()));
        }
        let ctx = unsafe { &mut *ctx_ptr };
        eval_impl(args, ctx)
    });

    ctx.set_global("eval".to_string(), Value::NativeFunction(Rc::new(eval_fn)));
    Ok(())
}

pub fn call_js_function(
    _ctx: &mut Context,
    f: &crate::value::ValueFunction,
    args: Vec<Value>,
) -> Result<Value, JsError> {
    let closure = Rc::clone(&f.closure);
    let call_env_rc = Rc::new(RefCell::new(Environment::with_parent(closure)));
    bind_params(&f.params, &args, &call_env_rc, f.is_arrow)?;

    if f.is_arrow {
        eval_arrow_body(&f.arrow_body, &call_env_rc)
    } else {
        eval::eval_function_body(&f.body, &call_env_rc, false)
    }
}

pub fn bind_params(
    params: &[ast::Param],
    args: &[Value],
    call_env: &Rc<RefCell<Environment>>,
    is_arrow: bool,
) -> Result<(), JsError> {
    for (i, param) in params.iter().enumerate() {
        let value = resolve_param_value(param, args, i, call_env, is_arrow)?;
        call_env.borrow_mut().declare(param.name.clone(), value);
    }
    Ok(())
}

pub fn resolve_param_value(
    param: &ast::Param,
    args: &[Value],
    index: usize,
    call_env: &Rc<RefCell<Environment>>,
    is_arrow: bool,
) -> Result<Value, JsError> {
    match args.get(index).cloned() {
        Some(Value::Undefined) if param.default.is_some() => {
            eval::eval_expression(param.default.as_ref().unwrap(), call_env, is_arrow)
        }
        Some(v) => Ok(v),
        None if param.default.is_some() => {
            eval::eval_expression(param.default.as_ref().unwrap(), call_env, is_arrow)
        }
        None => Ok(Value::Undefined),
    }
}

pub fn eval_arrow_body(
    arrow_body: &Option<ast::ArrowBody>,
    call_env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match arrow_body {
        Some(ast::ArrowBody::Expression(expr)) => eval::eval_expression(expr, call_env, true),
        Some(ast::ArrowBody::Block(stmts)) => eval::eval_function_body(stmts, call_env, true),
        None => Ok(Value::Undefined),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{ArrowBody, Expression, Param};

    fn make_env() -> Rc<RefCell<Environment>> {
        Rc::new(RefCell::new(Environment::new()))
    }

    #[test]
    fn test_bind_params_no_args() {
        let env = make_env();
        let params = vec![Param::new("x")];
        let args: Vec<Value> = vec![];
        bind_params(&params, &args, &env, false).unwrap();
        assert_eq!(env.borrow().get("x"), Some(Value::Undefined));
    }

    #[test]
    fn test_bind_params_with_args() {
        let env = make_env();
        let params = vec![Param::new("x"), Param::new("y")];
        let args = vec![Value::Number(1.0), Value::Number(2.0)];
        bind_params(&params, &args, &env, false).unwrap();
        assert_eq!(env.borrow().get("x"), Some(Value::Number(1.0)));
        assert_eq!(env.borrow().get("y"), Some(Value::Number(2.0)));
    }

    #[test]
    fn test_bind_params_extra_args() {
        let env = make_env();
        let params = vec![Param::new("x")];
        let args = vec![Value::Number(1.0), Value::Number(2.0)];
        bind_params(&params, &args, &env, false).unwrap();
        assert_eq!(env.borrow().get("x"), Some(Value::Number(1.0)));
    }

    #[test]
    fn test_bind_params_arrow_true() {
        let env = make_env();
        let params = vec![Param::new("x")];
        let args = vec![Value::Number(42.0)];
        bind_params(&params, &args, &env, true).unwrap();
        assert_eq!(env.borrow().get("x"), Some(Value::Number(42.0)));
    }

    #[test]
    fn test_resolve_param_value_undefined_uses_default() {
        let env = make_env();
        env.borrow_mut()
            .define("y".to_string(), Value::Number(99.0));
        let mut param = Param::new("x");
        param.default = Some(Box::new(Expression::Identifier("y".to_string())));
        let args: Vec<Value> = vec![Value::Undefined];
        let result = resolve_param_value(&param, &args, 0, &env, false).unwrap();
        assert_eq!(result, Value::Number(99.0));
    }

    #[test]
    fn test_resolve_param_value_provided_value() {
        let env = make_env();
        let param = Param::new("x");
        let args = vec![Value::Number(5.0)];
        let result = resolve_param_value(&param, &args, 0, &env, false).unwrap();
        assert_eq!(result, Value::Number(5.0));
    }

    #[test]
    fn test_resolve_param_value_missing_no_default() {
        let env = make_env();
        let param = Param::new("x");
        let args: Vec<Value> = vec![];
        let result = resolve_param_value(&param, &args, 0, &env, false).unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn test_eval_arrow_body_expression() {
        let env = make_env();
        let expr = ArrowBody::Expression(Expression::Number(42.0));
        let result = eval_arrow_body(&Some(expr), &env).unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_eval_arrow_body_block() {
        let env = make_env();
        let stmts = std::rc::Rc::new(vec![crate::ast::Statement::Return(Some(Box::new(
            Expression::Number(7.0),
        )))]);
        let result = eval_arrow_body(&Some(ArrowBody::Block(stmts)), &env).unwrap();
        assert_eq!(result, Value::Number(7.0));
    }

    #[test]
    fn test_eval_arrow_body_none() {
        let env = make_env();
        let result = eval_arrow_body(&None, &env).unwrap();
        assert_eq!(result, Value::Undefined);
    }
}
