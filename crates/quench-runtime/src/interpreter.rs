// linter-skip
//! JavaScript interpreter - evaluates AST nodes

use std::rc::Rc;
use std::cell::RefCell;
use std::cell::Cell;

use crate::ast::*;
use crate::value::{Value, JsError, Object, ObjectKind, ValueFunction, NativeFunction, to_js_string, to_bool, to_number, strict_eq, loose_eq};
use crate::env::Environment;

/// Control flow for break/continue statements
#[derive(Debug, Clone, Copy)]
enum ControlFlow {
    Break,
    Continue,
}

// Control flow flag for break/continue propagation
thread_local! {
    static CONTROL_FLOW: Cell<Option<ControlFlow>> = const { Cell::new(None) };
}

/// Set the control flow flag
fn set_control_flow(cf: ControlFlow) {
    CONTROL_FLOW.with(|cell| cell.set(Some(cf)));
}

/// Get and clear the control flow flag
fn take_control_flow() -> Option<ControlFlow> {
    CONTROL_FLOW.with(|cell| cell.take())
}

/// Maximum recursion depth to prevent stack overflow
const DEFAULT_MAX_RECURSION_DEPTH: usize = 10000;

// Mutable override for testing (atomic for thread-safety)
static MAX_RECURSION_DEPTH_OVERRIDE: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_RECURSION_DEPTH);

/// Get the effective maximum recursion depth
fn get_max_depth() -> usize {
    MAX_RECURSION_DEPTH_OVERRIDE.load(Ordering::SeqCst)
}

/// Set the maximum recursion depth (for testing only)
#[allow(dead_code)]
pub fn set_max_call_depth(depth: usize) {
    MAX_RECURSION_DEPTH_OVERRIDE.store(depth, Ordering::SeqCst);
}

// Thread-local storage for current "this" binding when calling native functions
thread_local! {
    static CURRENT_THIS: Cell<Option<Value>> = const { Cell::new(None) };
}

// Global counter for recursion depth (simpler than thread-local for this use case)
use std::sync::atomic::{AtomicUsize, Ordering};
static CURRENT_DEPTH: AtomicUsize = AtomicUsize::new(0);

/// Set the current "this" binding for native function calls
pub fn set_native_this(this_val: Value) {
    CURRENT_THIS.with(|cell| cell.set(Some(this_val)));
}

/// Get the current "this" binding for native function calls
pub fn get_native_this() -> Option<Value> {
    CURRENT_THIS.with(|cell| cell.take())
}

/// Increment depth and check against maximum
/// NOTE: This function ALWAYS increments the counter. On failure, the caller
/// MUST call release_depth() to decrement. On success, caller also calls
/// release_depth() when done.
fn check_depth() -> Result<(), JsError> {
    let depth = CURRENT_DEPTH.fetch_add(1, Ordering::SeqCst);
    if depth >= get_max_depth() {
        // DO NOT decrement here - caller must call release_depth()
        // This allows caller to always release on both success and error paths
        Err(JsError("Maximum call stack size exceeded".to_string()))
    } else {
        Ok(())
    }
}

/// Decrement depth when returning from a recursive call
fn release_depth() {
    CURRENT_DEPTH.fetch_sub(1, Ordering::SeqCst);
}

/// Reset depth counter (call before evaluating a new top-level script)
pub fn reset_depth() {
    CURRENT_DEPTH.store(0, Ordering::SeqCst);
}

/// Evaluate a complete program with hoisting
pub fn eval_program(program: &Program, env: &mut Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    match program {
        Program::Script(statements) => {
            // First pass: hoist function declarations
            hoist_functions(statements, env);
            
            // Second pass: execute statements (global scope has undefined as "this")
            set_this_binding(env, Value::Undefined);
            let mut last_value = Value::Undefined;
            for stmt in statements {
                last_value = eval_statement(stmt, env, false)?;
            }
            Ok(last_value)
        }
    }
}

/// Set the "this" binding in the current scope
fn set_this_binding(env: &Rc<RefCell<Environment>>, this_value: Value) {
    // Store "this" in the innermost scope
    env.borrow_mut().current_scope_mut().set_this(this_value);
}

/// Get the current "this" binding
fn get_this_binding(env: &Rc<RefCell<Environment>>) -> Value {
    for scope in env.borrow().scopes.iter().rev() {
        if let Some(this_val) = scope.get_this() {
            return this_val;
        }
    }
    Value::Undefined
}

/// Hoist function declarations to the top of the scope
/// Only creates functions that don't already exist (to avoid overwriting hoisted functions
/// with newly created ones during normal execution).
fn hoist_functions(statements: &[Statement], env: &Rc<RefCell<Environment>>) {
    for stmt in statements {
        match stmt {
            Statement::FunctionDeclaration { name, params, body } => {
                // Only create if not already defined (to avoid overwriting hoisted functions)
                if !env.borrow().has(name) {
                    let func = ValueFunction::new(
                        Some(name.clone()),
                        params.clone(),
                        body.clone(),
                        Rc::clone(env),
                    );
                    env.borrow_mut().define(name.clone(), Value::Function(func));
                } else {
                    // Already defined, skip (was hoisted)
                }
            }
            Statement::Block(stmts) => {
                // Recursively hoist in nested blocks
                hoist_functions(stmts, env);
            }
            Statement::If { consequent, alternate, .. } => {
                hoist_functions(std::slice::from_ref(consequent.as_ref()), env);
                if let Some(alt) = alternate {
                    hoist_functions(std::slice::from_ref(alt.as_ref()), env);
                }
            }
            Statement::While { body, .. } => {
                hoist_functions(std::slice::from_ref(body.as_ref()), env);
            }
            Statement::For { body, .. } => {
                hoist_functions(std::slice::from_ref(body.as_ref()), env);
            }
            _ => {}
        }
    }
}

/// Evaluate a list of statements
pub fn eval_statements(stmts: &[Statement], env: &Rc<RefCell<Environment>>, is_expr_body: bool) -> Result<Value, JsError> {
    let mut last_value = Value::Undefined;
    for stmt in stmts {
        last_value = eval_statement(stmt, env, is_expr_body)?;
        // Check if a break/continue was set - if so, stop executing statements
        // We need to check WITHOUT clearing the flag, so use get()
        if CONTROL_FLOW.with(|cell| cell.get().is_some()) {
            break;
        }
    }
    Ok(last_value)
}

/// Evaluate a single statement
#[allow(clippy::single_match)]
pub fn eval_statement(stmt: &Statement, env: &Rc<RefCell<Environment>>, _is_expr_body: bool) -> Result<Value, JsError> {
    // Note: Depth is checked in call_value_with_this, not here.
    // Statements don't consume call stack - only function calls do.
    
    let result = match stmt {
        Statement::VarDeclaration { kind, name, init } => {
            // First, declare the variable (puts it in TDZ for let/const)
            env.borrow_mut().declare_var(name.clone(), kind.clone());
            
            // Then initialize it (for all var kinds)
            let value = if let Some(expr) = init {
                eval_expression(expr, env)?
            } else {
                Value::Undefined
            };
            
            // Check for const reassignment - need to track this differently
            // For now, just initialize
            env.borrow_mut().initialize_declared(name, value);
            Ok(Value::Undefined)
        }

        Statement::FunctionDeclaration { name, params, body } => {
            // Don't overwrite existing function (from hoisting) - this keeps the hoisted
            // function's identity, which is important for prototype caching.
            if !env.borrow().has(name) {
                let func = ValueFunction::new(
                    Some(name.clone()),
                    params.clone(),
                    body.clone(),
                    Rc::clone(env),
                );
                env.borrow_mut().define(name.clone(), Value::Function(func));
            } else {
                // Already defined, skip (was hoisted)
            }
            Ok(Value::Undefined)
        }

        Statement::If { condition, consequent, alternate } => {
            let cond_val = eval_expression(condition, env)?;
            if to_bool(&cond_val) {
                eval_statement(consequent.as_ref(), env, _is_expr_body)
            } else if let Some(alt) = alternate {
                eval_statement(alt.as_ref(), env, _is_expr_body)
            } else {
                Ok(Value::Undefined)
            }
        }

        Statement::While { condition, body } => {
            let mut last = Value::Undefined;
            let mut iter_count = 0;
            while to_bool(&eval_expression(condition, env)?) {
                iter_count += 1;
                if iter_count > 10 {
                    return Err(JsError("while loop ran too many times".to_string()));
                }
                // Clear any previous control flow
                let _ = take_control_flow();
                
                last = eval_statement(body.as_ref(), env, _is_expr_body)?;
                
                // Check for break or continue
                let cf = take_control_flow();
                match cf {
                    Some(ControlFlow::Break) => break,
                    Some(ControlFlow::Continue) => {}
                    None => {}
                }
            }
            Ok(last)
        }

        Statement::For { init, condition, update, body } => {
            // Initialize
            if let Some(for_init) = init {
                match for_init {
                    ForInit::Expression(expr) => {
                        let _ = eval_expression(expr, env)?;
                    }
                    ForInit::VarDeclaration { kind, name, init } => {
                        // First declare (puts in TDZ for let/const)
                        env.borrow_mut().declare_var(name.clone(), kind.clone());
                        let value = init.as_ref().map(|e| eval_expression(e, env)).unwrap_or(Ok(Value::Undefined))?;
                        env.borrow_mut().initialize_declared(name, value);
                    }
                }
            }

            // Loop
            let check_condition = || -> bool {
                if let Some(c) = condition.as_ref() {
                    to_bool(&eval_expression(c, env).unwrap_or(Value::Undefined))
                } else {
                    true
                }
            };
            while check_condition() {
                // Clear any previous control flow
                take_control_flow();
                
                let _ = eval_statement(body.as_ref(), env, _is_expr_body)?;
                
                // Check for break or continue
                match take_control_flow() {
                    Some(ControlFlow::Break) => {
                        // Break out of the loop
                        break;
                    }
                    Some(ControlFlow::Continue) => {
                        // Continue - execute update and loop again
                    }
                    None => {}
                }
                
                if let Some(update) = update {
                    let _ = eval_expression(update, env)?;
                }
            }

            Ok(Value::Undefined)
        }

        Statement::Block(stmts) => {
            env.borrow_mut().push_scope();
            let result = eval_statements(stmts, env, _is_expr_body);
            env.borrow_mut().pop_scope();
            result
        }

        Statement::Return(expr) => {
            if let Some(e) = expr {
                Ok(eval_expression(e, env)?)
            } else {
                Ok(Value::Undefined)
            }
        }

        Statement::Expression(expr) => {
            eval_expression(expr, env)
        }

        Statement::Empty => Ok(Value::Undefined),

        Statement::Break(_) => {
            set_control_flow(ControlFlow::Break);
            Ok(Value::Undefined)
        }
        Statement::Continue(_) => {
            set_control_flow(ControlFlow::Continue);
            Ok(Value::Undefined)
        }

        Statement::TryCatch { body, param, handler } => {
            match eval_statement(body.as_ref(), env, _is_expr_body) {
                Ok(v) => Ok(v),
                Err(e) => {
                    if let Some(name) = param {
                        env.borrow_mut().define(name.clone(), Value::String(e.to_string()));
                    }
                    eval_statement(handler.as_ref(), env, _is_expr_body)
                }
            }
        }

        Statement::Throw(expr) => {
            let msg = to_js_string(&eval_expression(expr, env)?);
            Err(JsError(msg))
        }
    };
    
    result
}

/// Evaluate an expression
pub fn eval_expression(expr: &Expression, env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    // Note: Depth is checked in call_value_with_this, not here.
    // Expressions don't consume call stack - only function calls do.
    
    let result = match expr {
        Expression::Number(n) => Ok(Value::Number(*n)),
        Expression::String(s) => Ok(Value::String(s.clone())),
        Expression::Boolean(b) => Ok(Value::Boolean(*b)),
        Expression::Null => Ok(Value::Null),
        Expression::Undefined => Ok(Value::Undefined),

        Expression::Identifier(name) => {
            // Special handling for "this"
            if name == "this" {
                return Ok(get_this_binding(env));
            }
            
            // Check for TDZ in the scope chain
            for scope in env.borrow().scopes.iter().rev() {
                if scope.is_tdz(name) {
                    return Err(JsError(format!("ReferenceError: Cannot access '{}' before initialization", name)));
                }
            }
            
            let val = env.borrow().get(name)
                .ok_or_else(|| JsError(format!("ReferenceError: {} is not defined", name)))?;
            Ok(val)
        }

        Expression::Object(props) => {
            let mut obj = Object::new(ObjectKind::Ordinary);
            // Set prototype to Object.prototype if available
            if let Some(prototype) = crate::builtins::get_object_prototype() {
                obj.prototype = Some(prototype);
            }
            for (key, value) in props {
                let key_str = match key {
                    PropertyKey::Ident(s) => s.clone(),
                    PropertyKey::String(s) => s.clone(),
                    PropertyKey::Number(n) => n.to_string(),
                    PropertyKey::Computed(e) => to_js_string(&eval_expression(e, env)?),
                };
                match value {
                    PropertyValue::Value(expr) => {
                        let val = eval_expression(expr, env)?;
                        obj.set(&key_str, val);
                    }
                    PropertyValue::Getter { params: _, body } => {
                        // Store the getter body for later evaluation
                        obj.set_getter(&key_str, body.clone());
                    }
                    PropertyValue::Setter { param, body } => {
                        // Store the setter body for later evaluation, with the current closure
                        obj.set_setter(&key_str, param.clone(), body.clone(), Rc::clone(env));
                    }
                }
            }
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }

        Expression::Array(elements) => {
            let mut arr = Object::new_array(elements.len());
            for (i, elem_expr) in elements.iter().enumerate() {
                let value = eval_expression(elem_expr, env)?;
                arr.set(&i.to_string(), value);
            }
            // Set prototype to Array.prototype if available
            if let Some(prototype) = crate::builtins::get_array_prototype() {
                arr.prototype = Some(prototype);
            }
            Ok(Value::Object(Rc::new(RefCell::new(arr))))
        }

        Expression::FunctionExpression { name, params, body } => {
            let func = ValueFunction::new(
                name.clone(),
                params.clone(),
                body.clone(),
                Rc::clone(env),
            );
            Ok(Value::Function(func))
        }

        Expression::ArrowFunction { params, body } => {
            let func = ValueFunction::new_arrow(
                params.clone(),
                body.clone(),
                Rc::clone(env),
            );
            Ok(Value::Function(func))
        }

        Expression::Binary { op, left, right } => {
            let left_val = eval_expression(left, env)?;
            let right_val = eval_expression(right, env)?;
            eval_binary_op(*op, &left_val, &right_val)
        }

        Expression::Unary { op, argument } => {
            // Special handling for typeof on undeclared variables
            if *op == UnaryOp::Typeof {
                if let Expression::Identifier(name) = argument.as_ref() {
                    if name != "this" && !env.borrow().has(name) {
                        // typeof on undeclared variable returns "undefined"
                        return Ok(Value::String("undefined".to_string()));
                    }
                }
            }
            let val = eval_expression(argument, env)?;
            eval_unary_op(*op, &val)
        }

        Expression::Assignment { left, right } => {
            let right_val = eval_expression(right, env)?;
            let result = assign_to(left, &right_val, env);
            result?;
            Ok(right_val)
        }

        Expression::CompoundAssignment { op, left, right } => {
            let left_val = eval_expression(left, env)?;
            let right_val = eval_expression(right, env)?;
            let result = eval_binary_op(op.to_binary(), &left_val, &right_val)?;
            assign_to(left, &result, env)?;
            Ok(result)
        }

        Expression::Call { callee, arguments } => {
            // Extract the function and "this" binding from the callee
            let (func, this_val) = eval_callee_with_this(callee, env)?;
            let args: Result<Vec<Value>, _> = arguments.iter()
                .map(|a| eval_expression(a, env))
                .collect();
            let args = args?;
            call_value_with_this(func, args, this_val)
        }

        Expression::Member { object, property, computed } => {
            let obj_val = eval_expression(object, env)?;
            let prop_name = if *computed {
                let prop_expr = match property {
                    PropertyKey::Computed(e) => e.as_ref(),
                    _ => return Err(JsError("Invalid computed property".to_string())),
                };
                to_js_string(&eval_expression(prop_expr, env)?)
            } else {
                match property {
                    PropertyKey::Ident(s) => s.clone(),
                    PropertyKey::String(s) => s.clone(),
                    PropertyKey::Number(n) => n.to_string(),
                    PropertyKey::Computed(e) => to_js_string(&eval_expression(e, env)?),
                }
            };
            
            // Property access
            match obj_val {
                Value::Object(o) => {
                    // First check if there's a getter - getters take precedence
                    {
                        let obj = o.borrow();
                        if let Some(getter_storage) = obj.get_getter(&prop_name) {
                            // Clone the getter storage so we can release the borrow
                            let getter_clone = getter_storage.clone();
                            drop(obj);
                            return call_getter(&o, &getter_clone, env);
                        }
                    }
                    
                    // Check regular properties
                    {
                        let obj = o.borrow();
                        if let Some(val) = obj.get(&prop_name) {
                            return Ok(val);
                        }
                    }
                    
                    // Check if this is a global access that should fall back to environment
                    {
                        let obj = o.borrow();
                        if obj.kind == ObjectKind::Global {
                            drop(obj);
                            // get returns Option<Value>, so we can use it directly
                            if let Some(val) = env.borrow().get(&prop_name) {
                                return Ok(val);
                            }
                            return Ok(Value::Undefined);
                        }
                    }
                    // Handle Date.prototype - Date is an Object but should have a prototype
                    {
                        let obj = o.borrow();
                        if obj.kind == ObjectKind::Date && prop_name == "prototype" {
                            let mut proto = Object::new(ObjectKind::Ordinary);
                            // Add constructor pointing to Date
                            let date_constructor = Value::Object(Rc::clone(&o));
                            proto.set("constructor", date_constructor);
                            return Ok(Value::Object(Rc::new(RefCell::new(proto))));
                        }
                    }
                    // In JavaScript, accessing a non-existent property returns undefined
                    // This is different from strict mode where it throws
                    Ok(Value::Undefined)
                }
                Value::String(s) => {
                    // String prototype methods
                    let s_clone = s.clone();
                    let prop_name_clone = prop_name.clone();
                    match prop_name.as_str() {
                        "length" => Ok(Value::Number(s.len() as f64)),
                        "charAt" | "charCodeAt" | "indexOf" | "substring" | "slice" 
                        | "toUpperCase" | "toLowerCase" | "trim" | "split" 
                        | "includes" | "startsWith" | "endsWith" | "replace" | "match" 
                        | "search" | "concat" => {
                            Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
                                let s = s_clone.clone();
                                match prop_name_clone.as_str() {
                                    "length" => Ok(Value::Number(s.len() as f64)),
                                    "charAt" => {
                                        let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                                        Ok(Value::String(s.chars().nth(idx).map(|c| c.to_string()).unwrap_or_default()))
                                    }
                                    "indexOf" => {
                                        let needle = args.first().map(to_js_string).unwrap_or_default();
                                        Ok(Value::Number(s.find(&needle).map(|i| i as f64).unwrap_or(-1.0)))
                                    }
                                    "toUpperCase" => Ok(Value::String(s.to_uppercase())),
                                    "toLowerCase" => Ok(Value::String(s.to_lowercase())),
                                    "trim" => Ok(Value::String(s.trim().to_string())),
                                    "includes" => {
                                        let needle = args.first().map(to_js_string).unwrap_or_default();
                                        Ok(Value::Boolean(s.contains(&needle)))
                                    }
                                    "startsWith" => {
                                        let needle = args.first().map(to_js_string).unwrap_or_default();
                                        Ok(Value::Boolean(s.starts_with(&needle)))
                                    }
                                    "endsWith" => {
                                        let needle = args.first().map(to_js_string).unwrap_or_default();
                                        Ok(Value::Boolean(s.ends_with(&needle)))
                                    }
                                    "concat" => {
                                        let sep = args.iter().map(to_js_string).collect::<Vec<_>>().join("");
                                        Ok(Value::String(format!("{}{}", s, sep)))
                                    }
                                    "split" => {
                                        let sep = args.first().map(to_js_string).unwrap_or_default();
                                        let parts: Vec<Value> = if sep.is_empty() {
                                            s.chars().map(|c| Value::String(c.to_string())).collect()
                                        } else {
                                            s.split(&sep).map(|p| Value::String(p.to_string())).collect()
                                        };
                                        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(parts.len())))))
                                    }
                                    "substring" => {
                                        let start = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                                        let end = args.get(1).map(|v| to_number(v) as usize).unwrap_or(s.len());
                                        let start = start.min(s.len());
                                        let end = end.min(s.len());
                                        let start = start.min(end);
                                        Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
                                    }
                                    "slice" => {
                                        let start = args.first().map(|v| to_number(v) as i64).unwrap_or(0) as isize;
                                        let end = args.get(1).map(|v| to_number(v) as i64).unwrap_or(s.len() as i64) as isize;
                                        let len = s.len() as isize;
                                        let start = if start < 0 { (len + start).max(0) as usize } else { start as usize }.min(len as usize);
                                        let end = if end < 0 { (len + end).max(0) as usize } else { end as usize }.min(len as usize);
                                        let end = end.max(start);
                                        Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
                                    }
                                    "match" => {
                                        let pattern = args.first().map(to_js_string).unwrap_or_default();
                                        // Simple regex matching - just check if pattern is in string
                                        Ok(Value::Boolean(s.contains(&pattern)))
                                    }
                                    "search" => {
                                        let pattern = args.first().map(to_js_string).unwrap_or_default();
                                        Ok(Value::Number(s.find(&pattern).map(|i| i as f64).unwrap_or(-1.0)))
                                    }
                                    _ => Ok(Value::Undefined),
                                }
                            }))))
                        }
                        // Unknown properties return undefined (JavaScript semantics)
                        _ => Ok(Value::Undefined),
                    }
                }
                Value::Function(ref f) => {
                    if prop_name == "name" {
                        Ok(Value::String(f.name.clone().unwrap_or_default()))
                    } else if prop_name == "prototype" {
                        // Return the cached prototype (creates if needed using interior mutability)
                        let proto = f.get_prototype();
                        Ok(Value::Object(proto))
                    } else {
                        // Other function properties return undefined
                        Ok(Value::Undefined)
                    }
                }
                Value::NativeFunction(ref nf) => {
                    // Native functions (like console.log) can have prototype
                    if prop_name == "name" {
                        Ok(Value::String("anonymous".to_string()))
                    } else if prop_name == "prototype" {
                        // Create a prototype object for the native function
                        let mut proto = Object::new(ObjectKind::Ordinary);
                        // Add a constructor property pointing back to this function
                        proto.set("constructor", Value::NativeFunction(Rc::clone(nf)));
                        Ok(Value::Object(Rc::new(RefCell::new(proto))))
                    } else if prop_name == "length" {
                        Ok(Value::Number(0.0))
                    } else if prop_name == "call" {
                        // NativeFunction.call - returns the function itself
                        // (In JavaScript, func.call(thisArg, ...args) calls func with thisArg as this)
                        Ok(Value::NativeFunction(Rc::clone(nf)))
                    } else if prop_name == "apply" {
                        // NativeFunction.apply - returns the function itself
                        // (In JavaScript, func.apply(thisArg, argsArray) calls func with thisArg as this)
                        Ok(Value::NativeFunction(Rc::clone(nf)))
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                Value::NativeConstructor(ref nc) => {
                    // Native constructors (Date, Error, etc.) have a prototype property
                    if prop_name == "prototype" {
                        // Return the prototype object stored in the constructor
                        Ok(Value::Object(Rc::clone(&nc.prototype)))
                    } else if prop_name == "length" {
                        Ok(Value::Number(0.0))
                    } else if prop_name == "name" {
                        Ok(Value::String("anonymous".to_string()))
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                Value::Number(_) => {
                    // Number primitives need to look up Number.prototype
                    // Try to get Number.prototype from global scope
                    if let Some(Value::Object(ref num_obj)) = env.borrow().get("Number") {
                        let num_obj = num_obj.borrow();
                        if let Some(Value::Object(ref proto)) = num_obj.get("prototype") {
                            // Look up the property on Number.prototype
                            let proto_obj = proto.borrow();
                            if let Some(val) = proto_obj.get(&prop_name) {
                                return Ok(val);
                            }
                        }
                    }
                    // Fallback: return undefined for unknown properties
                    Ok(Value::Undefined)
                }
                _ => {
                    // For other value types (Boolean, etc.), return undefined
                    // In JavaScript, accessing properties on primitives returns undefined
                    Ok(Value::Undefined)
                }
            }
        }

        Expression::Conditional { condition, consequent, alternate } => {
            if to_bool(&eval_expression(condition, env)?) {
                eval_expression(consequent, env)
            } else {
                eval_expression(alternate, env)
            }
        }

        Expression::Update { op, argument, prefix } => {
            let current = eval_expression(argument, env)?;
            let current_num = to_number(&current);
            let new_val = match op {
                UpdateOp::Increment => current_num + 1.0,
                UpdateOp::Decrement => current_num - 1.0,
            };
            assign_to(argument, &Value::Number(new_val), env)?;
            if *prefix {
                Ok(Value::Number(new_val))
            } else {
                Ok(Value::Number(current_num))
            }
        }

        Expression::New { constructor, arguments } => {
            let constructor_val = eval_expression(constructor, env)?;
            let args: Result<Vec<Value>, _> = arguments.iter()
                .map(|a| eval_expression(a, env))
                .collect();
            let args = args?;
            
            // If the constructor is an Object, look up its "constructor" property
            let actual_constructor = match &constructor_val {
                Value::Object(o) => {
                    let obj = o.borrow();
                    if let Some(constructor) = obj.get("constructor") {
                        constructor.clone()
                    } else {
                        return Err(JsError("Object is not a constructor".to_string()));
                    }
                }
                other => other.clone(),
            };
            
            // Get the prototype from the constructor
            let prototype: Option<Rc<RefCell<Object>>> = match &constructor_val {
                Value::Object(o) => {
                    // Look up "prototype" property on the object
                    let proto = o.borrow().get("prototype");
                    if let Some(Value::Object(proto_obj)) = proto {
                        // Clone the Rc
                        Some(proto_obj.clone())
                    } else {
                        None
                    }
                }
                Value::Function(ref f) => {
                    // Use the function's built-in prototype getter (interior mutability)
                    Some(f.get_prototype())
                }
                _ => None,
            };
            
            // Create a new object with the prototype
            let new_obj = if let Some(proto) = prototype {
                Object::with_prototype(ObjectKind::Ordinary, proto)
            } else {
                Object::new(ObjectKind::Ordinary)
            };
            let new_obj_rc = Rc::new(RefCell::new(new_obj));
            
            // Call the constructor with "this" bound to the new object
            let result = call_value_with_this(actual_constructor, args, Value::Object(Rc::clone(&new_obj_rc)))?;
            
            // If the constructor returns an object, return that; otherwise return the new object
            match result {
                Value::Undefined => Ok(Value::Object(new_obj_rc)),
                Value::Object(_) => Ok(result),
                _ => Ok(Value::Object(new_obj_rc)),
            }
        }

        Expression::Sequence(exprs) => {
            let mut last = Value::Undefined;
            for e in exprs {
                last = eval_expression(e, env)?;
            }
            Ok(last)
        }
        
        Expression::BlockExpr(stmts) => {
            // Block expression returns the value of the last statement
            let mut last = Value::Undefined;
            for stmt in stmts {
                last = eval_statement(stmt, env, false)?;
            }
            Ok(last)
        }
        
        Expression::ArrayPattern(_) => {
            // Array patterns should be handled during assignment
            // This is reached when evaluating the pattern expression itself
            Err(JsError("Array pattern must be used in assignment context".to_string()))
        }
        
        Expression::ObjectPattern(_) => {
            // Object patterns should be handled during assignment
            // This is reached when evaluating the pattern expression itself
            Err(JsError("Object pattern must be used in assignment context".to_string()))
        }
        
        Expression::ForOf { variable, iterable, body } => {
            // for-of loop
            let iter_value = eval_expression(iterable, env)?;
            let items = get_iterator(&iter_value)?;
            let mut last = Value::Undefined;
            
            for item in items {
                // Assign the item to the loop variable
                assign_to(variable, &item, env)?;
                
                // Execute body
                last = eval_statement(body, env, false)?;
                
                // Check for break/continue
                match take_control_flow() {
                    Some(ControlFlow::Break) => {
                        break;
                    }
                    Some(ControlFlow::Continue) => {
                        // Continue - proceed to next iteration
                    }
                    None => {}
                }
            }
            Ok(last)
        }
        
        Expression::ForIn { variable, object, body } => {
            // for-in loop
            let obj_value = eval_expression(object, env)?;
            let keys = get_enumerable_keys(&obj_value)?;
            let mut last = Value::Undefined;
            
            for key in keys {
                // Assign the key to the loop variable
                assign_to(variable, &Value::String(key), env)?;
                
                // Execute body
                last = eval_statement(body, env, false)?;
                
                // Check for break/continue
                match take_control_flow() {
                    Some(ControlFlow::Break) => {
                        break;
                    }
                    Some(ControlFlow::Continue) => {
                        // Continue - proceed to next iteration
                    }
                    None => {}
                }
            }
            Ok(last)
        }
        
        // Optional chaining expressions - should not be reached as they are lowered to Conditional
        Expression::OptChain { .. } | Expression::OptChainCall { .. } => {
            Err(JsError("Internal error: optional chaining not lowered".to_string()))
        }
    };
    
    result
}

/// Evaluate a binary operator
fn eval_binary_op(op: BinaryOp, left: &Value, right: &Value) -> Result<Value, JsError> {
    match op {
        BinaryOp::Add => {
            if matches!(left, Value::String(_)) || matches!(right, Value::String(_)) {
                Ok(Value::String(format!("{}{}", to_js_string(left), to_js_string(right))))
            } else {
                Ok(Value::Number(to_number(left) + to_number(right)))
            }
        }
        BinaryOp::Sub => Ok(Value::Number(to_number(left) - to_number(right))),
        BinaryOp::Mul => Ok(Value::Number(to_number(left) * to_number(right))),
        BinaryOp::Div => Ok(Value::Number(to_number(left) / to_number(right))),
        BinaryOp::Mod => Ok(Value::Number(to_number(left) % to_number(right))),

        BinaryOp::Eq => Ok(Value::Boolean(loose_eq(left, right))),
        BinaryOp::Neq => Ok(Value::Boolean(!loose_eq(left, right))),

        BinaryOp::In => {
            // The `in` operator: left is property name, right is object
            let prop_name = to_js_string(left);
            match right {
                Value::Object(obj) => Ok(Value::Boolean(obj.borrow().has(&prop_name))),
                Value::String(s) => {
                    // String has indexed properties
                    if let Ok(idx) = prop_name.parse::<usize>() {
                        Ok(Value::Boolean(idx < s.chars().count()))
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                _ => Ok(Value::Boolean(false)),
            }
        }

        BinaryOp::Instanceof => {
            // The `instanceof` operator: left is object, right is constructor
            // Walk the prototype chain of left, checking if any prototype equals constructor's prototype
            
            // Helper to walk prototype chain and check for match
            fn check_instanceof(obj: &Rc<RefCell<Object>>, target_proto: &Rc<RefCell<Object>>) -> bool {
                let mut current: Option<Rc<RefCell<Object>>> = Some(Rc::clone(obj));
                while let Some(obj_rc) = current {
                    if Rc::ptr_eq(&obj_rc, target_proto) {
                        return true;
                    }
                    let obj_ref = obj_rc.borrow();
                    current = obj_ref.prototype.as_ref().map(Rc::clone);
                }
                false
            }
            
            match (left, right) {
                (_, Value::Undefined) | (_, Value::Null) => Ok(Value::Boolean(false)),
                (Value::Object(obj), Value::Function(ctor)) => {
                    // Get constructor's prototype
                    let ctor_proto = ctor.get_prototype();
                    let result = check_instanceof(obj, &ctor_proto);
                    Ok(Value::Boolean(result))
                }
                (Value::Object(obj), Value::NativeConstructor(ctor)) => {
                    // NativeConstructor: check if object's prototype chain contains ctor.prototype
                    let result = check_instanceof(obj, &ctor.prototype);
                    Ok(Value::Boolean(result))
                }
                (Value::Function(func), Value::NativeConstructor(ctor)) => {
                    // ValueFunction vs NativeConstructor: check if function's prototype 
                    // is in the constructor's prototype chain
                    let func_proto = func.get_prototype();
                    let result = check_instanceof(&func_proto, &ctor.prototype);
                    Ok(Value::Boolean(result))
                }
                (Value::Object(obj), Value::Object(ctor)) => {
                    // Try to get prototype from constructor object
                    let ctor_ref = ctor.borrow();
                    if let Some(Value::Object(proto)) = ctor_ref.get("prototype") {
                        drop(ctor_ref);
                        let result = check_instanceof(obj, &proto);
                        Ok(Value::Boolean(result))
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                _ => Ok(Value::Boolean(false)),
            }
        }
        BinaryOp::StrictEq => Ok(Value::Boolean(strict_eq(left, right))),
        BinaryOp::StrictNeq => Ok(Value::Boolean(!strict_eq(left, right))),

        BinaryOp::Lt => Ok(Value::Boolean(to_number(left) < to_number(right))),
        BinaryOp::Gt => Ok(Value::Boolean(to_number(left) > to_number(right))),
        BinaryOp::Le => Ok(Value::Boolean(to_number(left) <= to_number(right))),
        BinaryOp::Ge => Ok(Value::Boolean(to_number(left) >= to_number(right))),

        BinaryOp::And => {
            if to_bool(left) {
                Ok(right.clone())
            } else {
                Ok(left.clone())
            }
        }
        BinaryOp::Or => {
            if to_bool(left) {
                Ok(left.clone())
            } else {
                Ok(right.clone())
            }
        }
        BinaryOp::NullishCoalescing => {
            // Nullish coalescing: returns right if left is null or undefined
            match left {
                Value::Undefined | Value::Null => Ok(right.clone()),
                _ => Ok(left.clone()),
            }
        }

        BinaryOp::BitAnd => Ok(Value::Number((to_number(left) as i64 & to_number(right) as i64) as f64)),
        BinaryOp::BitOr => Ok(Value::Number((to_number(left) as i64 | to_number(right) as i64) as f64)),
        BinaryOp::BitXor => Ok(Value::Number((to_number(left) as i64 ^ to_number(right) as i64) as f64)),
        BinaryOp::Shl => Ok(Value::Number(((to_number(left) as i64) << (to_number(right) as i64)) as f64)),
        BinaryOp::Shr => Ok(Value::Number(((to_number(left) as i64) >> (to_number(right) as i64)) as f64)),
        BinaryOp::Ushr => Ok(Value::Number(((to_number(left) as u64) >> (to_number(right) as u64)) as f64)),
    }
}

/// Evaluate a unary operator
fn eval_unary_op(op: UnaryOp, val: &Value) -> Result<Value, JsError> {
    match op {
        UnaryOp::Not => Ok(Value::Boolean(!to_bool(val))),
        UnaryOp::Neg => Ok(Value::Number(-to_number(val))),
        UnaryOp::BitNot => Ok(Value::Number(!(to_number(val) as i64) as f64)),
        UnaryOp::Typeof => {
            let type_str = match val {
                Value::Undefined => "undefined",
                Value::Null => "object",
                Value::Boolean(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => "function",
                Value::Object(_) => "object",
                Value::Symbol(_) => "symbol",
            };
            Ok(Value::String(type_str.to_string()))
        }
        UnaryOp::Void => Ok(Value::Undefined),
    }
}

/// Assign a value to a target (variable or member)
fn assign_to(target: &Expression, value: &Value, env: &Rc<RefCell<Environment>>) -> Result<(), JsError> {
    match target {
        Expression::Identifier(name) => {
            if env.borrow().has(name) {
                env.borrow_mut().set(name, value.clone());
            } else {
                env.borrow_mut().define(name.clone(), value.clone());
            }
            Ok(())
        }
        Expression::Member { object, property, computed } => {
            let obj_val = eval_expression(object, env)?;
            let prop_name = if *computed {
                match property {
                    PropertyKey::Computed(e) => to_js_string(&eval_expression(e, env)?),
                    _ => return Err(JsError("Invalid computed property".to_string())),
                }
            } else {
                match property {
                    PropertyKey::Ident(s) => s.clone(),
                    PropertyKey::String(s) => s.clone(),
                    PropertyKey::Number(n) => n.to_string(),
                    PropertyKey::Computed(e) => to_js_string(&eval_expression(e, env)?),
                }
            };

            if let Value::Object(o) = obj_val {
                // Check if there's a setter
                let has_setter = {
                    let obj_ref = o.borrow();
                    obj_ref.get_setter(&prop_name).is_some()
                };
                
                if has_setter {
                    // Get the setter storage
                    let setter_clone = {
                        let obj_ref = o.borrow();
                        obj_ref.get_setter(&prop_name).map(|s| s.clone())
                    };
                    if let Some(setter_storage) = setter_clone {
                        // Call the setter with the value
                        call_setter(&o, &setter_storage, value.clone(), env)?;
                        return Ok(());
                    }
                }
                // No setter, just set the property
                o.borrow_mut().set(&prop_name, value.clone());
                Ok(())
            } else {
                Err(JsError(format!("Cannot assign to property of non-object, got {:?}", obj_val)))
            }
        }
        _ => Err(JsError("Invalid assignment target".to_string())),
    }
}

/// Evaluate a callee expression and extract the function and "this" binding.
/// For member calls like obj.method(), returns (method_function, obj).
/// For regular calls like foo(), returns (foo_function, undefined).
fn eval_callee_with_this(callee: &Expression, env: &Rc<RefCell<Environment>>) -> Result<(Value, Value), JsError> {
    match callee {
        // Method call: obj.method() - "this" is obj
        Expression::Member { object, property, computed } => {
            let obj_val = eval_expression(object, env)?;
            let prop_name = if *computed {
                match property {
                    PropertyKey::Computed(e) => to_js_string(&eval_expression(e, env)?),
                    _ => return Err(JsError("Invalid computed property".to_string())),
                }
            } else {
                match property {
                    PropertyKey::Ident(s) => s.clone(),
                    PropertyKey::String(s) => s.clone(),
                    PropertyKey::Number(n) => n.to_string(),
                    PropertyKey::Computed(e) => to_js_string(&eval_expression(e, env)?),
                }
            };
            
            // Look up the property on the object
            let func = match &obj_val {
                Value::Object(o) => {
                    let obj = o.borrow();
                    obj.get(&prop_name).unwrap_or(Value::Undefined)
                }
                Value::String(s) => {
                    // String prototype methods
                    let s_clone = s.clone();
                    let prop_name_clone = prop_name.clone();
                    match prop_name_clone.as_str() {
                        "length" => Value::Number(s.len() as f64),
                        "charAt" | "charCodeAt" | "indexOf" | "substring" | "slice" 
                        | "toUpperCase" | "toLowerCase" | "trim" | "split" 
                        | "includes" | "startsWith" | "endsWith" | "replace" => {
                            Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
                                let s = s_clone.clone();
                                match prop_name_clone.as_str() {
                                    "length" => Ok(Value::Number(s.len() as f64)),
                                    "charAt" => {
                                        let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                                        Ok(Value::String(s.chars().nth(idx).map(|c| c.to_string()).unwrap_or_default()))
                                    }
                                    "indexOf" => {
                                        let needle = args.first().map(to_js_string).unwrap_or_default();
                                        Ok(Value::Number(s.find(&needle).map(|i| i as f64).unwrap_or(-1.0)))
                                    }
                                    "toUpperCase" => Ok(Value::String(s.to_uppercase())),
                                    "toLowerCase" => Ok(Value::String(s.to_lowercase())),
                                    "trim" => Ok(Value::String(s.trim().to_string())),
                                    "includes" => {
                                        let needle = args.first().map(to_js_string).unwrap_or_default();
                                        Ok(Value::Boolean(s.contains(&needle)))
                                    }
                                    "startsWith" => {
                                        let needle = args.first().map(to_js_string).unwrap_or_default();
                                        Ok(Value::Boolean(s.starts_with(&needle)))
                                    }
                                    "endsWith" => {
                                        let needle = args.first().map(to_js_string).unwrap_or_default();
                                        Ok(Value::Boolean(s.ends_with(&needle)))
                                    }
                                    _ => Ok(Value::Undefined),
                                }
                            })))
                        }
                        _ => Value::Undefined,
                    }
                }
                Value::Number(_) => {
                    // Number prototype methods - look up Number.prototype from global scope
                    let _num_val = obj_val.clone();
                    if let Some(Value::Object(ref num_obj)) = env.borrow().get("Number") {
                        let num_obj = num_obj.borrow();
                        if let Some(Value::Object(ref proto)) = num_obj.get("prototype") {
                            let proto_obj = proto.borrow();
                            if let Some(val) = proto_obj.get(&prop_name) {
                                return Ok((val, obj_val));
                            }
                        }
                    }
                    Value::Undefined
                }
                _ => Value::Undefined,
            };
            
            // Return the function and the object as "this"
            Ok((func, obj_val))
        }
        // Regular call: foo() - "this" is undefined
        _ => {
            let func = eval_expression(callee, env)?;
            Ok((func, Value::Undefined))
        }
    }
}

/// Call a value as a function with an explicit "this" binding
pub fn call_value_with_this(func: Value, args: Vec<Value>, this_val: Value) -> Result<Value, JsError> {
    // Check recursion depth (function calls add to call stack)
    // If check fails, we MUST release the depth we just incremented before returning
    match check_depth() {
        Ok(_) => {}
        Err(e) => {
            release_depth(); // Counter was incremented by check_depth even on failure
            return Err(e);
        }
    }
    
    let result = match func {
        Value::Function(f) => {
            let closure = Rc::clone(&f.closure);
            let params = f.params.clone();
            
            // Create new scope
            let mut call_env = Environment::with_parent(Rc::clone(&closure));
            
            // Set "this" binding
            call_env.current_scope_mut().set_this(this_val);
            
            // Bind arguments to parameters
            for (i, param) in params.iter().enumerate() {
                let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
                call_env.define(param.clone(), arg);
            }
            
            let call_env = Rc::new(RefCell::new(call_env));
            
            if f.is_arrow {
                // Arrow functions don't change "this"
                if let Some(arrow_body) = &f.arrow_body {
                    match arrow_body.as_ref() {
                        ArrowBody::Expression(expr) => {
                            eval_expression(expr, &call_env)
                        }
                        ArrowBody::Block(stmts) => {
                            eval_statements(stmts, &call_env, true)
                        }
                    }
                } else {
                    Ok(Value::Undefined)
                }
            } else {
                eval_statements(&f.body, &call_env, false)
            }
        }
        Value::NativeFunction(nf) => {
            // Set the "this" binding for this native function call
            set_native_this(this_val);
            nf.call(args)
        }
        Value::NativeConstructor(nc) => {
            // NativeConstructor ignores the "this" binding - it's for new Foo() calls
            nc.call(args)
        }
        Value::Object(o) => {
            // Constructor call: new Foo()
            // In this case, "this" should be bound to the new object
            // Check if this object has a constructor
            let constructor_opt = {
                let obj = o.borrow();
                if let Some(constructor) = obj.get("constructor") {
                    if matches!(constructor, Value::Function(_) | Value::NativeFunction(_)) {
                        Some(constructor.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            
            if let Some(constructor) = constructor_opt {
                // For constructor calls, "this" is the new object being constructed
                let new_obj = Object::new(ObjectKind::Ordinary);
                let new_obj_rc = Rc::new(RefCell::new(new_obj));
                // Set prototype
                {
                    let proto = o.borrow().get("prototype");
                    if proto.is_some() {
                        new_obj_rc.borrow_mut().set("constructor", Value::Object(Rc::clone(&o)));
                    }
                }
                // Call the constructor
                call_value_with_this(constructor, args, Value::Object(Rc::clone(&new_obj_rc)))
            } else {
                Err(JsError("Object is not a constructor".to_string()))
            }
        }
        _ => Err(JsError("Value is not a function".to_string())),
    };
    
    // Release depth on exit
    release_depth();
    
    result
}

/// Call a value as a function
pub fn call_value(func: Value, args: Vec<Value>) -> Result<Value, JsError> {
    match func {
        Value::Function(f) => {
            let closure = Rc::clone(&f.closure);
            let params = f.params.clone();
            
            // Create new scope
            let mut call_env = Environment::with_parent(Rc::clone(&closure));
            
            // Bind arguments to parameters
            for (i, param) in params.iter().enumerate() {
                let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
                call_env.define(param.clone(), arg);
            }
            
            let call_env = Rc::new(RefCell::new(call_env));
            
            if f.is_arrow {
                if let Some(arrow_body) = &f.arrow_body {
                    match arrow_body.as_ref() {
                        ArrowBody::Expression(expr) => {
                            eval_expression(expr, &call_env)
                        }
                        ArrowBody::Block(stmts) => {
                            eval_statements(stmts, &call_env, true)
                        }
                    }
                } else {
                    Ok(Value::Undefined)
                }
            } else {
                eval_statements(&f.body, &call_env, false)
            }
        }
        Value::NativeFunction(nf) => {
            nf.call(args)
        }
        Value::NativeConstructor(nc) => {
            nc.call(args)
        }
        Value::Object(o) => {
            // Try to call as constructor
            let obj = o.borrow();
            if let Some(constructor) = obj.get("constructor") {
                if matches!(constructor, Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_)) {
                    drop(obj);
                    return call_value(constructor, args);
                }
            }
            Err(JsError("Object is not a function".to_string()))
        }
        _ => Err(JsError("Value is not a function".to_string())),
    }
}

/// Call a getter function with the object as "this"
fn call_getter(
    obj: &Rc<RefCell<Object>>,
    getter_storage: &crate::value::GetterStorage,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Use the current environment as the closure base
    let closure = Rc::clone(env);
    let body = getter_storage.body.clone();
    
    // Create new scope with the closure
    let mut call_env = Environment::with_parent(Rc::clone(&closure));
    // Set "this" to the object
    call_env.current_scope_mut().set_this(Value::Object(Rc::clone(obj)));
    
    let call_env = Rc::new(RefCell::new(call_env));
    
    // Execute the getter body
    if body.is_empty() {
        Ok(Value::Undefined)
    } else {
        eval_statements(&body, &call_env, true)
    }
}

/// Call a setter function with the object as "this" and the value as the parameter
fn call_setter(
    obj: &Rc<RefCell<Object>>,
    setter_storage: &crate::value::SetterStorage,
    value: Value,
    _env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Use the closure environment stored with the setter (captured at object creation time)
    let closure = Rc::clone(&setter_storage.closure);
    let body = setter_storage.body.clone();
    let param = setter_storage.param.clone();
    
    // Create new scope with the closure
    let mut call_env = Environment::with_parent(Rc::clone(&closure));
    // Set "this" to the object
    call_env.current_scope_mut().set_this(Value::Object(Rc::clone(obj)));
    // Set the parameter
    call_env.define(param, value);
    
    let call_env = Rc::new(RefCell::new(call_env));
    
    // Execute the setter body
    if body.is_empty() {
        Ok(Value::Undefined)
    } else {
        eval_statements(&body, &call_env, false)
    }
}

/// Get an iterator for for-of/for-in loops
fn get_iterator(value: &Value) -> Result<Vec<Value>, JsError> {
    match value {
        Value::Object(o) => {
            // Check if it's an array
            {
                let obj = o.borrow();
                if obj.kind == ObjectKind::Array {
                    // Return array elements
                    let mut result = Vec::new();
                    for elem in &obj.elements {
                        result.push(elem.clone());
                    }
                    return Ok(result);
                }
            }
            // Try to get Symbol.iterator
            {
                let obj = o.borrow();
                if let Some(Value::Object(symbol_rc)) = obj.get("Symbol") {
                    let iter_val: Option<Value> = {
                        let symbol_obj = symbol_rc.borrow();
                        symbol_obj.get("iterator").map(|v| v.clone())
                    };
                    if let Some(Value::Object(iter_fn)) = iter_val {
                        drop(obj); // Release borrow
                        let result = call_value(Value::Object(Rc::clone(&iter_fn)), vec![])?;
                        // For simplicity, just iterate over the result
                        return get_iterator(&result);
                    }
                }
            }
            // Fall back to iterating over numeric indices
            {
                let obj = o.borrow();
                let mut result = Vec::new();
                for elem in &obj.elements {
                    result.push(elem.clone());
                }
                Ok(result)
            }
        }
        Value::String(s) => {
            // Iterate over characters
            Ok(s.chars().map(|c| Value::String(c.to_string())).collect())
        }
        _ => Err(JsError("Value is not iterable".to_string())),
    }
}

/// Get enumerable property keys for for-in loop
fn get_enumerable_keys(value: &Value) -> Result<Vec<String>, JsError> {
    match value {
        Value::Object(o) => {
            let obj = o.borrow();
            let mut keys: Vec<String> = Vec::new();
            
            // Collect own property keys (for simplicity, not using proper enumeration)
            for key in obj.properties.keys() {
                keys.push(key.clone());
            }
            for i in 0..obj.elements.len() {
                keys.push(i.to_string());
            }
            
            Ok(keys)
        }
        Value::String(s) => {
            // for-in on string iterates over indices
            Ok((0..s.len()).map(|i| i.to_string()).collect())
        }
        _ => Ok(vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_function_creation() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let func = ValueFunction::new(
            Some("test".to_string()),
            vec!["x".to_string()],
            vec![],
            env,
        );
        assert_eq!(func.name, Some("test".to_string()));
        assert_eq!(func.params, vec!["x"]);
    }

    #[test]
    fn test_arrow_function_creation() {
        let env = Rc::new(RefCell::new(Environment::new()));
        let func = ValueFunction::new_arrow(
            vec!["x".to_string()],
            Box::new(ArrowBody::Expression(Expression::Number(42.0))),
            env,
        );
        assert!(func.is_arrow);
    }
}
