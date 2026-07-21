//! Statement evaluation

use crate::ast::*;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::interpreter::{
    collect_var_names_recursive, predeclare_let_const, set_control_flow, take_control_flow,
    ControlFlow,
};
use crate::value::{
    set_thrown_value, take_thrown_value, to_bool, to_js_string, JsError, Object, ObjectKind, Value,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate a list of statements
pub fn eval_statements(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    is_expr_body: bool,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let mut last_val = Value::Undefined;
    for stmt in stmts {
        let val = eval_statement(stmt, env, is_expr_body, in_arrow_function)?;
        // Per ES spec §8.3.2, empty completions (var/let/const/function declarations)
        // should not replace the previous completion value. Only update last_val
        // when the statement produces a non-empty value (like an expression).
        if !matches!(stmt, Statement::VarDeclaration { .. } | Statement::FunctionDeclaration { .. } | Statement::ClassDeclaration { .. } | Statement::SequenceDecls(_)) {
            last_val = val;
        }
        match take_control_flow() {
            Some(ControlFlow::Return(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            // Propagate break/continue so enclosing loops can observe them.
            Some(cf @ (ControlFlow::Break | ControlFlow::Continue)) => {
                set_control_flow(cf);
                return Ok(last_val);
            }
            None => {}
        }
    }
    Ok(last_val)
}

/// Evaluate a function body: completion value is discarded — only an
/// explicit `return` produces a value, per ES function semantics.
pub fn eval_function_body(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    for stmt in stmts {
        eval_statement(stmt, env, false, in_arrow_function)?;
        match take_control_flow() {
            Some(ControlFlow::Return(val)) => return Ok(val),
            Some(cf @ (ControlFlow::Break | ControlFlow::Continue)) => {
                set_control_flow(cf);
                return Ok(Value::Undefined);
            }
            None => {}
        }
    }
    Ok(Value::Undefined)
}

/// Evaluate a single statement
pub fn eval_statement(
    stmt: &Statement,
    env: &Rc<RefCell<Environment>>,
    _is_expr_body: bool,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    match stmt {
        Statement::VarDeclaration { kind, name, init } => {
            eval_var_decl(kind, name, init, env, in_arrow_function)
        }
        Statement::FunctionDeclaration {
            name,
            params,
            body,
            is_async,
            is_generator,
        } => eval_func_decl(name, params, body, env, *is_async, *is_generator),
        Statement::ClassDeclaration { name, class } => eval_class_decl(name, class, env),
        Statement::If {
            condition,
            consequent,
            alternate,
        } => {
            let cond_val = eval_expression(condition, env, in_arrow_function)?;
            if to_bool(&cond_val) {
                eval_statement(consequent.as_ref(), env, _is_expr_body, in_arrow_function)
            } else if let Some(alt) = alternate {
                eval_statement(alt.as_ref(), env, _is_expr_body, in_arrow_function)
            } else {
                Ok(Value::Undefined)
            }
        }
        Statement::While { condition, body } => eval_while(condition, body, env, in_arrow_function),
        Statement::For {
            init,
            condition,
            update,
            body,
        } => eval_for(init, condition, update, body, env, in_arrow_function),
        Statement::Block(stmts) => eval_block(stmts, env, in_arrow_function),
        Statement::SequenceDecls(stmts) => {
            // Evaluate var declarations in sequence without creating a new scope
            let mut result = Value::Undefined;
            for stmt in stmts {
                result = eval_statement(stmt, env, false, in_arrow_function)?;
            }
            Ok(result)
        }
        Statement::Return(expr) => {
            let val = match expr {
                Some(e) => eval_expression(e, env, in_arrow_function)?,
                None => Value::Undefined,
            };
            set_control_flow(ControlFlow::Return(val));
            Ok(Value::Undefined)
        }
        Statement::Expression(expr) => eval_expression(expr, env, in_arrow_function),
        Statement::Empty => Ok(Value::Undefined),
        Statement::Break(_) => {
            set_control_flow(ControlFlow::Break);
            Ok(Value::Undefined)
        }
        Statement::Continue(_) => {
            set_control_flow(ControlFlow::Continue);
            Ok(Value::Undefined)
        }
        Statement::TryCatch {
            body,
            param,
            handler,
        } => eval_try_catch(body, param, handler, env, in_arrow_function),
        Statement::Throw(expr) => {
            let value = eval_expression(expr, env, in_arrow_function)?;
            let msg = to_js_string(&value);
            // Store the original thrown value for catch blocks to retrieve
            set_thrown_value(value);
            Err(JsError(msg))
        }
        Statement::With { object, body } => {
            // `with (obj) { body }` — push a scope onto the env whose
            // identifier lookup defers to obj's properties. We model this by
            // pushing a fresh scope and pre-populating its bindings with a
            // snapshot of the object's own enumerable properties. A more
            // faithful implementation would track the object and defer each
            // get to it; this approximation is enough for the tests in
            // scope (variable captures of with-scoped names).
            if crate::interpreter::is_strict_mode() {
                return Err(JsError(
                    "SyntaxError: 'with' statements are not allowed in strict mode".to_string(),
                ));
            }
            let obj_val = eval_expression(object, env, in_arrow_function)?;
            let Value::Object(obj_rc) = obj_val else {
                return eval_statement(body, env, _is_expr_body, in_arrow_function);
            };
            env.borrow_mut().push_scope();
            env.borrow()
                .current_scope()
                .borrow_mut()
                .set_object_binding(Rc::clone(&obj_rc));
            {
                let obj_borrowed = obj_rc.borrow();
                // Check Symbol.unscopables (§13.11.7): properties blocked by
                // unscopables are not added to the with-scope.
                let unscopables_val = obj_borrowed
                    .symbol_properties
                    .iter()
                    .find_map(|(k, v)| {
                        if k.starts_with("Symbol(") && k.contains("unscopables") {
                            Some(v.clone())
                        } else {
                            None
                        }
                    })
                    .or_else(|| obj_borrowed.properties.get("unscopables").cloned());
                let blocked: std::collections::HashSet<String> =
                    if let Some(Value::Object(u_obj)) = unscopables_val {
                        let u = u_obj.borrow();
                        u.properties
                            .iter()
                            .filter(|(_, v)| crate::value::to_bool(v))
                            .map(|(k, _)| k.clone())
                            .collect()
                    } else {
                        std::collections::HashSet::new()
                    };
                for (key, value) in &obj_borrowed.properties {
                    if !blocked.contains(key) {
                        env.borrow_mut()
                            .current_scope()
                            .borrow_mut()
                            .define(key.clone(), value.clone());
                    }
                }
            }
            let result = eval_statement(body, env, _is_expr_body, in_arrow_function);
            env.borrow_mut().pop_scope();
            result
        }
        Statement::Export(stmt) => {
            // Export statements wrap other statements (like assignments)
            eval_statement(stmt, env, _is_expr_body, in_arrow_function)
        }
        Statement::Import {
            default,
            named,
            namespace,
            source,
        } => eval_import(default, named, namespace, source, env),
        Statement::ForIn {
            variable,
            object,
            body,
        } => eval_for_in_stmt(variable, object, body, env, in_arrow_function),
    }
}

/// Helper to set a property on globalThis if we're at the top level.
fn set_on_global_this(env: &Rc<RefCell<Environment>>, name: &str, value: Value) {
    // Only set on globalThis if this is the top-level environment
    let is_top_level = env.borrow().get_parent().is_none();
    if is_top_level {
        // Get globalThis outside the mutable borrow to avoid conflict
        let global_this = env.borrow().get("globalThis");
        if let Some(Value::Object(global_obj)) = global_this {
            global_obj.borrow_mut().set(name, value);
        }
    }
}

fn eval_var_decl(
    kind: &VarKind,
    name: &str,
    init: &Option<Expression>,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let already_declared = env.borrow().current_scope().borrow().has(name);
    if !already_declared {
        env.borrow_mut().declare_var(name.to_string(), *kind);
    }
    let mut value = if let Some(expr) = init {
        eval_expression(expr, env, in_arrow_function)?
    } else {
        Value::Undefined
    };
    // Per ES §13.3.3 SetFunctionName: when a VariableDeclaration's
    // initializer evaluates to a function expression that has no name,
    // bind the variable's name as the function's `name`.
    if let Value::Function(ref mut f) = value {
        if f.name.is_none() {
            f.name = Some(name.to_string());
            f.set_property("name", Value::String(name.to_string()));
        }
    }
    env.borrow_mut().initialize_declared(name, value.clone());
    // For top-level var declarations, also set on globalThis
    if *kind == VarKind::Var && env.borrow().get_parent().is_none() {
        set_on_global_this(env, name, value);
    }
    Ok(Value::Undefined)
}

fn eval_func_decl(
    name: &str,
    params: &[Param],
    body: &[Statement],
    env: &Rc<RefCell<Environment>>,
    is_async: bool,
    is_generator: bool,
) -> Result<Value, JsError> {
    let mut func = crate::value::ValueFunction::new(
        Some(name.to_owned()),
        params.to_vec(),
        body.to_vec(),
        Rc::clone(env),
        is_async,
        is_generator,
    );
    func.strict = crate::interpreter::is_strict_mode();
    func.name = Some(name.to_string()); // Set .name property per ES spec SetFunctionName
    let value = Value::Function(func);
    env.borrow_mut().define(name.to_owned(), value.clone());
    // Top-level function declarations are globals (same as var).
    if env.borrow().get_parent().is_none() {
        set_on_global_this(env, name, value);
    }
    Ok(Value::Undefined)
}

fn eval_class_decl(
    name: &str,
    class: &Class,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Evaluate the class expression with the declared name so static field
    // initializers observe `this.name === "<name>"` per ES §14.6.13.
    let class_val = crate::eval::class::eval_class_expr(class, env, Some(name))?;
    env.borrow_mut().define(name.to_owned(), class_val);
    Ok(Value::Undefined)
}

fn eval_while(
    condition: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    while to_bool(&eval_expression(condition, env, in_arrow_function)?) {
        take_control_flow();
        let _ = eval_statement(body, env, false, in_arrow_function)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Return(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    Ok(Value::Undefined)
}

fn eval_for(
    init: &Option<ForInit>,
    condition: &Option<Box<Expression>>,
    update: &Option<Box<Expression>>,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    if let Some(for_init) = init {
        match for_init {
            ForInit::Expression(expr) => {
                let _ = eval_expression(expr, env, in_arrow_function)?;
            }
            ForInit::VarDeclaration { kind, name, init } => {
                env.borrow_mut().declare_var(name.to_string(), *kind);
                let value = init
                    .as_ref()
                    .map(|e| eval_expression(e, env, in_arrow_function))
                    .unwrap_or(Ok(Value::Undefined))?;
                env.borrow_mut().initialize_declared(name, value);
            }
        }
    }
    let check_condition = || -> Result<bool, JsError> {
        if let Some(c) = condition.as_ref() {
            Ok(to_bool(&eval_expression(c, env, in_arrow_function)?))
        } else {
            Ok(true)
        }
    };
    while check_condition()? {
        take_control_flow();
        let _ = eval_statement(body, env, false, in_arrow_function)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Return(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::Continue) => {}
            None => {}
        }
        if let Some(update) = update {
            let _ = eval_expression(update, env, in_arrow_function)?;
        }
    }
    Ok(Value::Undefined)
}

fn eval_block(
    stmts: &[Statement],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    env.borrow_mut().push_scope();
    predeclare_let_const(stmts, &mut env.borrow_mut());
    let result = eval_statements(stmts, env, false, in_arrow_function);
    env.borrow_mut().pop_scope();
    result
}

fn eval_try_catch(
    body: &Statement,
    param: &Option<String>,
    handler: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    // Var declarations inside a try block are hoisted to the try's enclosing
    // scope, so they must be visible to the catch handler too. Predeclare them
    // in the parent env before evaluating the body.
    if let Statement::Block(stmts) = body {
        let mut names = Vec::new();
        collect_var_names_recursive(stmts, &mut names);
        for name in names {
            env.borrow_mut().declare_var(name, VarKind::Var);
        }
    }
    match eval_statement(body, env, false, in_arrow_function) {
        Ok(v) => Ok(v),
        Err(_e) => {
            // Take the thrown value and bind it to the catch param.
            let thrown_value = take_thrown_value();
            let thrown_value = thrown_value.unwrap_or(Value::Undefined);
            if let Some(name) = param {
                env.borrow_mut()
                    .define(name.to_string(), thrown_value.clone());
            }
            eval_statement(handler, env, false, in_arrow_function)
        }
    }
}

/// Evaluate an ES module import statement
/// For CommonJS compatibility, this reads from the global `__quench_modules__` cache
fn eval_import(
    default: &Option<String>,
    named: &[(String, String)],
    namespace: &Option<String>,
    source: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Get the module's exports from our module cache
    let module_exports = get_module_exports(source, env)?;

    // Handle default import: `import x from 'mod'`
    if let Some(name) = default {
        let default_val = module_exports
            .borrow()
            .get("default")
            .unwrap_or(Value::Undefined);
        env.borrow_mut().define(name.clone(), default_val);
    }

    // Handle named imports: `import { x, y as z } from 'mod'`
    for (local_name, exported_name) in named {
        let val = module_exports
            .borrow()
            .get(exported_name)
            .unwrap_or(Value::Undefined);
        env.borrow_mut().define(local_name.clone(), val);
    }

    // Handle namespace import: `import * as ns from 'mod'`
    if let Some(name) = namespace {
        env.borrow_mut()
            .define(name.clone(), Value::Object(module_exports.clone()));
    }

    Ok(Value::Undefined)
}

/// Get exports from a module (CommonJS-style lookup)
fn get_module_exports(
    source: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    // Check if we have a cached module in the global __quench_modules__
    let cache = env.borrow().get("__quench_modules__");

    if let Some(Value::Object(cache_obj)) = &cache {
        let key = normalize_module_path(source);
        if let Some(Value::Object(exports_obj)) = cache_obj.borrow().get(&key) {
            return Ok(exports_obj.clone());
        }
    }

    // Check globalThis.__quench_modules__
    let global = env.borrow().get("globalThis");
    if let Some(Value::Object(global_obj)) = &global {
        if let Some(Value::Object(modules_obj)) = global_obj.borrow().get("__quench_modules__") {
            let key = normalize_module_path(source);
            if let Some(Value::Object(exports_obj)) = modules_obj.borrow().get(&key) {
                return Ok(exports_obj.clone());
            }
        }
    }

    // Create a new empty exports object for this module
    let exports = Object::new(ObjectKind::Ordinary);
    let exports_rc = Rc::new(RefCell::new(exports));

    // Cache it for future imports
    let key = normalize_module_path(source);
    if let Some(Value::Object(cache_obj)) = cache {
        cache_obj
            .borrow_mut()
            .set(&key, Value::Object(exports_rc.clone()));
    }

    Ok(exports_rc)
}

/// Normalize a module path to a cache key
fn normalize_module_path(source: &str) -> String {
    source.to_string()
}

/// Evaluate a for-in statement: for (x in object) { body }
fn eval_for_in_stmt(
    variable: &Expression,
    object: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    use crate::eval::iteration::get_enumerable_keys;
    use crate::eval::object::assign_to;

    let obj_value = eval_expression(object, env, in_arrow_function)?;
    let keys = get_enumerable_keys(&obj_value)?;
    if matches!(variable, Expression::ObjectPattern(_)) {
        return Err(JsError("unsupported pattern in for-in loop".to_string()));
    }
    for key in keys {
        assign_to(variable, &Value::String(key), env)?;
        let _ = eval_statement(body, env, false, in_arrow_function)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Return(val)) => {
                set_control_flow(ControlFlow::Return(val.clone()));
                return Ok(val);
            }
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    Ok(Value::Undefined)
}

#[cfg(test)]
mod tests;
