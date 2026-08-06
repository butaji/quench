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

/// Save the single-char regex cache (realm snapshot support; cached values
/// carry the current realm's RegExp.prototype)
pub(crate) fn save_regex_cache() -> rustc_hash::FxHashMap<char, Value> {
    REGEX_CACHE.with(|c| c.borrow().clone())
}

/// Restore the single-char regex cache (realm snapshot support)
pub(crate) fn restore_regex_cache(saved: rustc_hash::FxHashMap<char, Value>) {
    REGEX_CACHE.with(|c| *c.borrow_mut() = saved);
}

/// eval function implementation - executes JavaScript code in the current context.
/// Per ES spec §19.2.1, eval code inherits strict mode from its calling context.
/// We check for legacy octals here (before parsing the eval string) so that
/// eval in strict mode throws even when the eval string itself has no
/// "use strict" directive.
pub fn eval_impl(args: Vec<Value>, ctx: &mut Context) -> Result<Value, JsError> {
    let argument = args.first().cloned().unwrap_or(Value::Undefined);
    let mut source = match argument {
        Value::String(source) => source,
        value => return Ok(value),
    };
    if source.is_empty() {
        return Ok(Value::Undefined);
    }
    if source.lines().map(str::trim).any(|line| {
        (line.starts_with("export ")
            || line.starts_with("import ") && !line.starts_with("import ("))
            && !line.starts_with("import(")
    }) {
        let (err_val, js_err) = crate::value::error::create_js_error_with_type(
            "module declaration in eval code",
            "SyntaxError",
        );
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    let has_super_binding = crate::interpreter::get_current_eval_env().is_some_and(|env| {
        let mut current = Some(env);
        while let Some(environment) = current {
            if environment.borrow().get_super_class().is_some() {
                return true;
            }
            current = environment.borrow().get_parent();
        }
        false
    });
    let has_super_syntax =
        source.contains("super(") || source.contains("super.") || source.contains("super[");
    let has_super_property = source.contains("super.") || source.contains("super[");
    if crate::interpreter::is_direct_eval()
        && has_super_syntax
        && (source.contains("super(") || !has_super_binding && has_super_property)
    {
        let (err_val, js_err) = crate::value::error::create_js_error_with_type(
            "super call is not valid in this eval context",
            "SyntaxError",
        );
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }

    // Indirect eval does NOT inherit strict mode from the caller (ES §19.2.1).
    // Only direct eval inherits the calling context's strict mode.
    let strict_inherited = crate::interpreter::is_strict_mode()
        || source.trim_start().starts_with("\"use strict\";")
        || source.trim_start().starts_with("'use strict';");
    let eval_new_target_alias = "__quench_eval_new_target__";
    let has_new_target = crate::interpreter::is_direct_eval()
        && source.contains("new.target")
        && crate::interpreter::get_current_eval_env()
            .is_some_and(|env| env.borrow().get("new.target").is_some());
    if has_new_target {
        source = source.replace("new.target", eval_new_target_alias);
    }
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
    let eval_global_names = ctx
        .get_global("globalThis")
        .and_then(|global| match global {
            Value::Object(global) => Some(global.borrow().own_property_names()),
            _ => None,
        });
    // Save label stack depth before parse so we can restore on any exit path.
    let label_depth = crate::interpreter::label_stack_depth();
    // Indirect eval should not inherit strict mode for parsing. Temporarily
    // set strict mode to false so the parser doesn't reject strict-reserved
    // words like `arguments` or `eval`.
    let saved_strict = crate::interpreter::is_strict_mode();
    if !crate::interpreter::is_direct_eval() {
        crate::interpreter::set_strict_mode(false);
    }
    let mut program = match ctx.parse(&source) {
        Ok(program) => {
            crate::interpreter::set_strict_mode(saved_strict);
            program
        }
        Err(e) => {
            crate::interpreter::set_strict_mode(saved_strict);
            CURRENT_CONTEXT.with(|cell| {
                *cell.borrow_mut() = prev_ctx;
            });
            let (err_val, js_err) =
                crate::value::error::create_js_error_with_type(&e.0, "SyntaxError");
            crate::value::set_thrown_value(err_val);
            return Err(js_err);
        }
    };
    let ast::Program::Script(body) = &program;
    let indirect_new_lexical_names = if !crate::interpreter::is_direct_eval() {
        body.iter()
            .filter_map(|statement| match statement {
                ast::Statement::VarDeclaration {
                    kind: ast::VarKind::Let | ast::VarKind::Const,
                    name,
                    ..
                }
                | ast::Statement::ClassDeclaration { name, .. }
                    if !ctx.env.borrow().has(name) =>
                {
                    Some(name.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if let Some(Value::Object(global)) = ctx.get_global("globalThis") {
        if !global.borrow().extensible {
            for statement in body {
                if let ast::Statement::VarDeclaration {
                    kind: ast::VarKind::Var,
                    name,
                    ..
                } = statement
                {
                    if global.borrow().get_own_property(name).is_none() {
                        let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                            "cannot declare global variable",
                            "TypeError",
                        );
                        crate::value::set_thrown_value(err_val);
                        return Err(js_err);
                    }
                }
            }
        }
        for statement in body {
            if let ast::Statement::FunctionDeclaration { name, .. } = statement {
                if global
                    .borrow()
                    .get_own_property(name)
                    .is_some_and(|descriptor| {
                        descriptor.configurable == Some(false) && descriptor.writable != Some(true)
                    })
                {
                    let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                        "cannot redefine global property",
                        "TypeError",
                    );
                    crate::value::set_thrown_value(err_val);
                    return Err(js_err);
                }
            }
        }
    }
    if crate::interpreter::is_direct_eval() {
        if let Some(eval_env) = crate::interpreter::get_current_eval_env() {
            let declared = eval_env.borrow().declared_private_names();
            let ast::Program::Script(body) = &program;
            if let Err(js_err) =
                crate::eval::class::private_names::reject_undeclared_private_names_in_eval(
                    body, &declared,
                )
            {
                CURRENT_CONTEXT.with(|cell| {
                    *cell.borrow_mut() = prev_ctx;
                });
                return Err(js_err);
            }
            if let Some(class_id) = eval_env.borrow().private_class_id() {
                let ast::Program::Script(body) = &mut program;
                crate::eval::class::private_names::scope_script_private_names(body, class_id);
            }
        }
    }
    let eval_strict = strict_inherited
        || source.trim_start().starts_with("\"use strict\"")
        || source.trim_start().starts_with("'use strict'");
    let strict_indirect_eval = !crate::interpreter::is_direct_eval()
        && (source.trim_start().starts_with("\"use strict\"")
            || source.trim_start().starts_with("'use strict'"));
    if !crate::interpreter::is_direct_eval() && !strict_indirect_eval {
        reject_indirect_eval_global_lexical_conflict(&program, ctx)?;
    }
    if !eval_strict && crate::interpreter::is_direct_eval() {
        reject_eval_var_lexical_conflict(&program, ctx)?;
    }
    let in_class_field = crate::interpreter::is_eval_in_class_field()
        || crate::interpreter::get_current_eval_env()
            .is_some_and(|e| e.borrow().is_in_class_field_initializer());
    if in_class_field {
        let ast::Program::Script(body) = &program;
        if let Err(js_err) =
            crate::eval::class::private_elements::reject_class_field_eval_early_errors(
                body,
                crate::interpreter::is_direct_eval(),
            )
        {
            CURRENT_CONTEXT.with(|cell| {
                *cell.borrow_mut() = prev_ctx;
            });
            return Err(js_err);
        }
    }
    // Establish the eval barrier so eval code cannot see outer labels.
    // has_label will only search up to this depth.
    crate::interpreter::set_eval_barrier_depth(label_depth);
    crate::interpreter::push_label_scope();
    // For indirect eval, set strict mode to false during execution (matching
    // strict_inherited) so assignment to undeclared identifiers creates implicit
    // globals instead of throwing ReferenceError.
    if !crate::interpreter::is_direct_eval() {
        crate::interpreter::set_strict_mode(false);
    }
    let saved_new_target = crate::interpreter::get_new_target();
    if crate::interpreter::is_direct_eval() && in_class_field {
        crate::interpreter::set_new_target(None);
    }
    let result = if crate::interpreter::is_direct_eval() {
        if let Some(mut eval_env) = crate::interpreter::get_current_eval_env() {
            if eval_strict {
                eval_env = Rc::new(RefCell::new(Environment::with_parent(eval_env)));
            }
            if in_class_field {
                eval_env
                    .borrow_mut()
                    .define("new.target".to_string(), Value::Undefined);
            }
            if has_new_target {
                let new_target = eval_env
                    .borrow()
                    .get("new.target")
                    .unwrap_or(Value::Undefined);
                eval_env
                    .borrow_mut()
                    .define(eval_new_target_alias.to_string(), new_target);
            }
            let previous_eval_env = crate::interpreter::get_current_eval_env();
            crate::interpreter::set_current_eval_env(Some(Rc::clone(&eval_env)));
            let result =
                crate::interpreter::eval_program(&program, &mut eval_env, Some(&source), false);
            crate::interpreter::set_current_eval_env(previous_eval_env);
            result
        } else {
            let mut eval_env = Rc::clone(&ctx.env);
            if eval_strict {
                eval_env = Rc::new(RefCell::new(Environment::with_parent(eval_env)));
            }
            let this_value = if strict_inherited {
                Value::Undefined
            } else {
                ctx.env
                    .borrow()
                    .get("globalThis")
                    .unwrap_or(Value::Undefined)
            };
            crate::interpreter::set_this_binding(&eval_env, this_value);
            if has_new_target {
                let new_target = eval_env
                    .borrow()
                    .get("new.target")
                    .unwrap_or(Value::Undefined);
                eval_env
                    .borrow_mut()
                    .define(eval_new_target_alias.to_string(), new_target);
            }
            let previous_eval_env = crate::interpreter::get_current_eval_env();
            crate::interpreter::set_current_eval_env(Some(Rc::clone(&eval_env)));
            let result =
                crate::interpreter::eval_program(&program, &mut eval_env, Some(&source), false);
            crate::interpreter::set_current_eval_env(previous_eval_env);
            result
        }
    } else if strict_indirect_eval {
        let this_value = ctx
            .env
            .borrow()
            .get("globalThis")
            .unwrap_or(Value::Undefined);
        let saved_scopes = ctx.env.borrow_mut().scopes.split_off(1);
        let mut eval_env = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&ctx.env))));
        crate::interpreter::set_this_binding(&eval_env, this_value);
        let result =
            crate::interpreter::eval_program(&program, &mut eval_env, Some(&source), false);
        ctx.env.borrow_mut().scopes.extend(saved_scopes);
        result
    } else {
        let this_value = ctx
            .env
            .borrow()
            .get("globalThis")
            .unwrap_or(Value::Undefined);
        crate::interpreter::set_this_binding(&ctx.env, this_value);
        let saved_scopes = ctx.env.borrow_mut().scopes.split_off(1);
        let result = crate::interpreter::eval_program(&program, &mut ctx.env, Some(&source), false);
        ctx.env.borrow_mut().scopes.extend(saved_scopes);
        result
    };
    crate::interpreter::set_new_target(saved_new_target);
    // Restore strict mode after eval (indirect eval may have changed it).
    crate::interpreter::set_strict_mode(saved_strict);
    // Exit eval: restore label stack and clear barrier.
    crate::interpreter::pop_label_scope();
    crate::interpreter::clear_eval_barrier_depth();
    let ast::Program::Script(body) = &program;
    if strict_indirect_eval {
        let mut var_names = Vec::new();
        crate::interpreter::collect_var_names_recursive(body, &mut var_names);
        let before = eval_global_names.as_ref();
        if let Some(Value::Object(global)) = ctx.get_global("globalThis") {
            for name in &var_names {
                if before.is_some_and(|names| names.contains(name)) {
                    continue;
                }
                global.borrow_mut().delete(name);
                ctx.env
                    .borrow_mut()
                    .current_scope()
                    .borrow_mut()
                    .remove_binding(name);
            }
        }
    }
    if let (Some(before), Some(Value::Object(global))) =
        (eval_global_names.as_ref(), ctx.get_global("globalThis"))
    {
        let mut var_names = Vec::new();
        crate::interpreter::collect_var_names_recursive(body, &mut var_names);
        for name in var_names {
            if before.contains(&name) {
                continue;
            }
            let Some(descriptor) = global.borrow().get_own_property(&name) else {
                continue;
            };
            global.borrow_mut().define(
                &name,
                descriptor.value.clone().unwrap_or(Value::Undefined),
                crate::value::PropertyFlags {
                    value: descriptor.value,
                    writable: descriptor.writable.unwrap_or(false),
                    enumerable: descriptor.enumerable.unwrap_or(false),
                    configurable: true,
                },
            );
        }
    }
    if let Some(Value::Object(global)) = ctx.get_global("globalThis") {
        for statement in body {
            if let ast::Statement::FunctionDeclaration { name, .. } = statement {
                if let Some(value) = ctx.env.borrow().get(name) {
                    let descriptor = global.borrow().get_own_property(name);
                    let existed = eval_global_names
                        .as_ref()
                        .is_some_and(|names| names.contains(name));
                    let nonconfigurable = descriptor
                        .as_ref()
                        .is_some_and(|d| d.configurable == Some(false));
                    global.borrow_mut().define(
                        name,
                        value,
                        crate::value::PropertyFlags {
                            value: descriptor.as_ref().and_then(|d| d.value.clone()),
                            writable: if existed && nonconfigurable {
                                descriptor.as_ref().and_then(|d| d.writable).unwrap_or(true)
                            } else {
                                true
                            },
                            enumerable: if existed && nonconfigurable {
                                descriptor
                                    .as_ref()
                                    .and_then(|d| d.enumerable)
                                    .unwrap_or(true)
                            } else {
                                true
                            },
                            configurable: if existed {
                                descriptor
                                    .as_ref()
                                    .and_then(|d| d.configurable)
                                    .unwrap_or(true)
                            } else {
                                true
                            },
                        },
                    );
                }
            }
        }
    }
    if crate::interpreter::is_direct_eval() {
        let ast::Program::Script(body) = &program;
        let eval_env =
            crate::interpreter::get_current_eval_env().unwrap_or_else(|| Rc::clone(&ctx.env));
        for statement in body {
            let name = match statement {
                ast::Statement::VarDeclaration {
                    kind: ast::VarKind::Let | ast::VarKind::Const,
                    name,
                    ..
                }
                | ast::Statement::ClassDeclaration { name, .. } => name,
                ast::Statement::FunctionDeclaration { name, .. } if eval_strict => name,
                _ => continue,
            };
            eval_env
                .borrow()
                .current_scope()
                .borrow_mut()
                .remove_binding(name);
        }
    } else {
        for name in indirect_new_lexical_names {
            ctx.env
                .borrow()
                .current_scope()
                .borrow_mut()
                .remove_binding(&name);
        }
    }
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
        if crate::interpreter::eval_conflict_names()
            .is_some_and(|extra| extra.iter().any(|(extra_name, _)| extra_name == name))
        {
            let (error, js_error) = crate::value::error::create_js_error_with_type(
                &format!("Identifier '{}' has already been declared", name),
                "SyntaxError",
            );
            crate::value::set_thrown_value(error);
            return Err(js_error);
        }
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
        let is_local = eval_env.borrow().has(&name);
        if !is_local {
            eval_env.borrow_mut().declare_eval_var(name.clone());
            eval_env
                .borrow_mut()
                .initialize_declared(&name, Value::Undefined);
        }
    }
    for statement in body {
        if let ast::Statement::FunctionDeclaration { name, .. } = statement {
            let has_global_property = ctx.get_global("globalThis").is_some_and(
                |global| matches!(global, Value::Object(object) if object.borrow().has(name)),
            );
            if !has_global_property {
                eval_env.borrow_mut().declare_eval_var(name.clone());
            }
        }
    }
    Ok(())
}

fn reject_indirect_eval_global_lexical_conflict(
    program: &crate::ast::Program,
    ctx: &Context,
) -> Result<(), JsError> {
    let ast::Program::Script(body) = program;
    let mut names = Vec::new();
    crate::interpreter::collect_var_names_recursive(body, &mut names);
    if names.iter().any(|name| {
        matches!(
            ctx.env
                .borrow()
                .scopes
                .first()
                .and_then(|scope| scope.borrow().get_kind(name)),
            Some(ast::VarKind::Let | ast::VarKind::Const)
        )
    }) {
        let (error, js_error) = crate::value::error::create_js_error_with_type(
            "Identifier has already been declared",
            "SyntaxError",
        );
        crate::value::set_thrown_value(error);
        return Err(js_error);
    }
    Ok(())
}

pub fn reject_global_lexical_declarations(
    ctx: &Context,
    program: &crate::ast::Program,
) -> Result<(), JsError> {
    let ast::Program::Script(body) = program;
    let names = crate::interpreter::collect_let_const_declarations(body);
    let Some(Value::Object(global)) = ctx.get_global("globalThis") else {
        return Ok(());
    };
    for (name, _) in names {
        if crate::context::get_current_env()
            .is_some_and(|env| env.borrow().get_kind(&name) == Some(crate::ast::VarKind::Var))
        {
            continue;
        }
        if global
            .borrow()
            .get_own_property(&name)
            .is_some_and(|descriptor| descriptor.configurable == Some(false))
        {
            let (error, js_error) = crate::value::error::create_js_error_with_type(
                "Identifier conflicts with a restricted global property",
                "SyntaxError",
            );
            crate::value::set_thrown_value(error);
            return Err(js_error);
        }
    }
    Ok(())
}

/// Initialize built-in globals and functions
pub fn init_builtins(ctx: &mut Context) -> Result<(), JsError> {
    ctx.set_global("__ops__".to_string(), crate::eval::ops::make_ops_object());
    host::register_builtin_functions(ctx)?;
    init_commonjs(ctx)?;
    init_es_module_cache(ctx)?;
    init_js_globals(ctx)?;
    sync_globals_to_global_this(ctx);
    register_eval_function(ctx)?;
    register_dynamic_import(ctx);
    // Register __ops__ — the Rust↔JS bridge for spec abstract operations.
    // JS builtins destructure this at parse time: const { IsCallable, ToObject } = __ops__.
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
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        global_obj.borrow_mut().prototype = Some(object_proto);
    }
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
    ctx.env
        .borrow_mut()
        .define("undefined".to_string(), Value::Undefined);
    ctx.env
        .borrow_mut()
        .define("Infinity".to_string(), Value::Number(f64::INFINITY));
    ctx.env
        .borrow_mut()
        .define("NaN".to_string(), Value::Number(f64::NAN));

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
        let value = value_rc.borrow().clone();
        let mut flags =
            global_obj
                .borrow()
                .get_descriptor(name)
                .unwrap_or(crate::value::PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: false,
                    configurable: true,
                });
        flags.value = Some(value.clone());
        global_obj.borrow_mut().define(name.as_str(), value, flags);
    }
}

/// Register the eval function as a global
fn register_dynamic_import(ctx: &mut Context) {
    let function = NativeFunction::new(|args| {
        let source_value = args.first().cloned().unwrap_or(crate::Value::Undefined);
        let primitive = match crate::value::to_primitive(&source_value, Some("string")) {
            Ok(value) => value,
            Err(error) => {
                let reason = crate::value::take_thrown_value()
                    .unwrap_or_else(|| crate::Value::String(error.to_string()));
                return crate::builtins::promise::create_rejected_promise(reason)
                    .map(crate::Value::Object);
            }
        };
        let source = crate::value::to_js_string(&primitive);
        let source_phase = matches!(args.get(2), Some(crate::Value::Boolean(true)))
            || matches!(args.get(1), Some(crate::Value::Boolean(true)));
        let deferred = matches!(args.get(2), Some(crate::Value::String(marker)) if marker == "__defer__")
            || matches!(args.get(1), Some(crate::Value::String(marker)) if marker == "__defer__");
        let options =
            if deferred || (source_phase && args.get(1) == Some(&crate::Value::Boolean(true))) {
                None
            } else {
                args.get(1)
            };
        let env = crate::context::get_current_env()
            .ok_or_else(|| JsError::new("TypeError: no current context"))?;
        crate::eval::statement::dynamic_import(&source, &env, options, source_phase, deferred)
    });
    ctx.set_global(
        "__dynamic_import__".to_string(),
        Value::NativeFunction(Rc::new(function)),
    );
}

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

    // Per spec: eval.length is 1, non-writable, configurable, non-enumerable
    eval_fn.define_property(
        "length",
        Value::Number(1.0),
        crate::value::object::helpers::PropertyFlags {
            writable: false,
            enumerable: false,
            configurable: true,
            value: Some(Value::Number(1.0)),
        },
    );
    // eval.name is "eval", non-writable, configurable, non-enumerable
    eval_fn.define_property(
        "name",
        Value::String("eval".to_string()),
        crate::value::object::helpers::PropertyFlags {
            writable: false,
            enumerable: false,
            configurable: true,
            value: Some(Value::String("eval".to_string())),
        },
    );

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

    #[test]
    fn eval_rejects_module_declarations() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx.eval("eval('export default null;')").is_err());
        assert!(ctx.eval("eval('import x from \\\"x\\\";')").is_err());
    }

    #[test]
    fn eval_rejects_function_declaration_for_non_definable_global() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx.eval("eval('function NaN(){}')").is_err());
    }

    #[test]
    fn strict_indirect_eval_does_not_create_global_var() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert_eq!(
            ctx.eval(
                "(0, eval)('\"use strict\"; var strictEvalOnly = 88;'); 'strictEvalOnly' in this"
            ),
            Ok(Value::Boolean(false))
        );
    }

    #[test]
    fn strict_indirect_eval_uses_global_lexical_environment() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert_eq!(
            ctx.eval("let x = 'outside'; { let x = 'inside'; (0, eval)('\"use strict\"; x;') }"),
            Ok(Value::String("outside".to_string()))
        );
    }

    #[test]
    fn double_register_builtins_preserves_object_static_methods() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        crate::builtins::bootstrap::bootstrap_js_builtins(&mut ctx).unwrap();
        // Mimic the test262 runner's initialize_test_context path
        crate::builtins::register_builtins(&mut ctx);
        crate::builtins::bootstrap::bootstrap_js_builtins(&mut ctx).unwrap();
        // Now load the test262 harness to mirror the full path
        let r = ctx
            .eval("[typeof Object.getOwnPropertyDescriptor, typeof Object.getPrototypeOf, typeof Object.keys, typeof Object.assign].join('|')")
            .unwrap();
        assert_eq!(
            r,
            crate::Value::String("function|function|function|function".into())
        );
    }

    #[test]
    fn compare_array_formats_mismatch_without_type_error() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        crate::builtins::bootstrap::bootstrap_js_builtins(&mut ctx).unwrap();
        let value = ctx
            .eval("[typeof String.call, typeof (function(x) { return x; }).call].join('|')")
            .unwrap();
        assert_eq!(value, crate::Value::String("function|function".into()));
    }

    #[test]
    fn direct_eval_lexical_bindings_do_not_leak() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx
            .eval("eval('let evalOnly = 3'); typeof evalOnly")
            .is_ok_and(|value| crate::value::to_js_string(&value) == "undefined"));
        assert!(ctx
            .eval("eval('class EvalOnly {}'); typeof EvalOnly")
            .is_ok_and(|value| crate::value::to_js_string(&value) == "undefined"));
    }

    #[test]
    fn strict_eval_block_function_declaration_does_not_leak() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx.eval("'use strict'; var err; eval('{ function f() {} }'); try { f; } catch (e) { err = e; } err instanceof ReferenceError").is_ok_and(|value| matches!(value, Value::Boolean(true))));
    }

    #[test]
    fn strict_eval_var_does_not_update_caller_binding() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert_eq!(
            ctx.eval("'use strict'; function f() { var x = 0; eval('var x = 1'); return x; } f()"),
            Ok(Value::Number(0.0))
        );
    }

    #[test]
    fn indirect_eval_var_ignores_lower_lexical_binding() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx.eval("{ let x; { (0, eval)('var x;'); } }").is_ok());
    }

    #[test]
    fn eval_returns_non_string_argument_unchanged() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx
            .eval("let x = {}; eval(x) === x")
            .is_ok_and(|value| { matches!(value, Value::Boolean(true)) }));
    }

    #[test]
    fn eval_global_function_declaration_has_configurable_property() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx.eval("eval('function evalDescriptorOnly() {}'); Object.getOwnPropertyDescriptor(this, 'evalDescriptorOnly').configurable").is_ok_and(|value| {
            matches!(value, Value::Boolean(true))
        }));
    }

    #[test]
    fn direct_eval_super_property_outside_method_is_syntax_error() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx
            .eval("try { eval('super.property;'); false } catch (e) { e instanceof SyntaxError }")
            .is_ok_and(|value| matches!(value, Value::Boolean(true))));
    }

    #[test]
    fn direct_eval_super_call_outside_constructor_is_syntax_error() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx
            .eval("try { ({ method() { eval('super();'); } }).method(); false } catch (e) { e instanceof SyntaxError }")
            .is_ok_and(|value| matches!(value, Value::Boolean(true))));
    }

    #[test]
    fn indirect_eval_uses_global_scope_not_current_block() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx
            .eval("let x = 'outside'; { let x = 'inside'; (0, eval)('x;') === 'outside' }")
            .is_ok_and(|value| { matches!(value, Value::Boolean(true)) }));
    }

    #[test]
    fn eval_arguments_var_in_arrow_default_parameter() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx.eval("let count = 0; const f = (p = eval(\"var arguments = 'param'\")) => { function arguments() {}; count++; }; f(); count").is_ok_and(|value| {
            matches!(value, Value::Number(number) if number == 1.0)
        }));
    }

    #[test]
    fn indirect_eval_lexical_declarations_do_not_leak() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx
            .eval("(0, eval)('let indirectOnly = 1'); typeof indirectOnly")
            .is_ok_and(|value| { crate::value::to_js_string(&value) == "undefined" }));
    }

    #[test]
    fn strict_eval_function_declaration_does_not_leak() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx.eval("function check() { 'use strict'; eval('function localOnly() {}'); return typeof localOnly; } check()").is_ok_and(|value| {
            crate::value::to_js_string(&value) == "undefined"
        }));
    }

    #[test]
    fn eval_var_rejects_new_binding_on_non_extensible_global() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx.eval("Object.preventExtensions(this); try { eval('var evalNewOnly'); false } catch (e) { e instanceof TypeError }").is_ok_and(|value| {
            matches!(value, Value::Boolean(true))
        }));
    }

    #[test]
    fn script_rejects_lexical_restricted_global() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        assert!(ctx.eval("let undefined;").is_err());
    }
}
