//! JavaScript interpreter - evaluates AST nodes

use std::rc::Rc;
use std::cell::RefCell;
use std::cell::Cell;

use crate::ast::*;
use crate::value::{Value, JsError, Object, ObjectKind, ValueFunction, NativeFunction, to_js_string, to_bool, to_number, strict_eq};
use crate::env::Environment;

// Thread-local storage for current "this" binding when calling native functions
thread_local! {
    static CURRENT_THIS: Cell<Option<Value>> = Cell::new(None);
}

/// Set the current "this" binding for native function calls
pub fn set_native_this(this_val: Value) {
    CURRENT_THIS.with(|cell| cell.set(Some(this_val)));
}

/// Get the current "this" binding for native function calls
pub fn get_native_this() -> Option<Value> {
    CURRENT_THIS.with(|cell| cell.take())
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
    }
    Ok(last_value)
}

/// Evaluate a single statement
#[allow(clippy::single_match)]
pub fn eval_statement(stmt: &Statement, env: &Rc<RefCell<Environment>>, _is_expr_body: bool) -> Result<Value, JsError> {
    match stmt {
        Statement::VarDeclaration { kind: _, name, init } => {
            let value = if let Some(expr) = init {
                eval_expression(expr, env)?
            } else {
                Value::Undefined
            };
            env.borrow_mut().define(name.clone(), value);
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
            while to_bool(&eval_expression(condition, env)?) {
                last = eval_statement(body.as_ref(), env, _is_expr_body)?;
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
                    ForInit::VarDeclaration { kind: _, name, init } => {
                        let value = init.as_ref().map(|e| eval_expression(e, env)).unwrap_or(Ok(Value::Undefined))?;
                        env.borrow_mut().define(name.clone(), value);
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
                let _ = eval_statement(body.as_ref(), env, _is_expr_body)?;
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

        Statement::Break(_) => Ok(Value::Undefined),
        Statement::Continue(_) => Ok(Value::Undefined),

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
    }
}

/// Evaluate an expression
pub fn eval_expression(expr: &Expression, env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    match expr {
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
            let val = env.borrow().get(name)
                .ok_or_else(|| JsError(format!("ReferenceError: {} is not defined", name)))?;
            // Debug: print function pointers
            Ok(val)
        }

        Expression::Object(props) => {
            let obj = Object::new(ObjectKind::Ordinary);
            let mut obj = obj;
            for (key, value_expr) in props {
                let value = eval_expression(value_expr, env)?;
                let key_str = match key {
                    PropertyKey::Ident(s) => s.clone(),
                    PropertyKey::String(s) => s.clone(),
                    PropertyKey::Number(n) => n.to_string(),
                    PropertyKey::Computed(e) => to_js_string(&eval_expression(e, env)?),
                };
                obj.set(&key_str, value);
            }
            Ok(Value::Object(Rc::new(RefCell::new(obj))))
        }

        Expression::Array(elements) => {
            let mut arr = Object::new_array(elements.len());
            for (i, elem_expr) in elements.iter().enumerate() {
                let value = eval_expression(elem_expr, env)?;
                arr.set(&i.to_string(), value);
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
                    let obj = o.borrow();
                    if let Some(val) = obj.get(&prop_name) {
                        Ok(val)
                    } else {
                        // Check if this is a global access that should fall back to environment
                        if obj.kind == ObjectKind::Global {
                            drop(obj);
                            // get returns Option<Value>, so we can use it directly
                            if let Some(val) = env.borrow().get(&prop_name) {
                                return Ok(val);
                            }
                            return Ok(Value::Undefined);
                        }
                        // Handle Date.prototype - Date is an Object but should have a prototype
                        if obj.kind == ObjectKind::Date && prop_name == "prototype" {
                            let mut proto = Object::new(ObjectKind::Ordinary);
                            // Add constructor pointing to Date
                            let date_constructor = Value::Object(Rc::clone(&o));
                            proto.set("constructor", date_constructor);
                            return Ok(Value::Object(Rc::new(RefCell::new(proto))));
                        }
                        // In JavaScript, accessing a non-existent property returns undefined
                        // This is different from strict mode where it throws
                        Ok(Value::Undefined)
                    }
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
                                        let needle = args.first().map(|v| to_js_string(v)).unwrap_or_default();
                                        Ok(Value::Number(s.find(&needle).map(|i| i as f64).unwrap_or(-1.0)))
                                    }
                                    "toUpperCase" => Ok(Value::String(s.to_uppercase())),
                                    "toLowerCase" => Ok(Value::String(s.to_lowercase())),
                                    "trim" => Ok(Value::String(s.trim().to_string())),
                                    "includes" => {
                                        let needle = args.first().map(|v| to_js_string(v)).unwrap_or_default();
                                        Ok(Value::Boolean(s.contains(&needle)))
                                    }
                                    "startsWith" => {
                                        let needle = args.first().map(|v| to_js_string(v)).unwrap_or_default();
                                        Ok(Value::Boolean(s.starts_with(&needle)))
                                    }
                                    "endsWith" => {
                                        let needle = args.first().map(|v| to_js_string(v)).unwrap_or_default();
                                        Ok(Value::Boolean(s.ends_with(&needle)))
                                    }
                                    "concat" => {
                                        let sep = args.iter().map(|v| to_js_string(v)).collect::<Vec<_>>().join("");
                                        Ok(Value::String(format!("{}{}", s, sep)))
                                    }
                                    "split" => {
                                        let sep = args.first().map(|v| to_js_string(v)).unwrap_or_default();
                                        let parts: Vec<Value> = if sep.is_empty() {
                                            s.chars().map(|c| Value::String(c.to_string())).collect()
                                        } else {
                                            s.split(&sep).map(|p| Value::String(p.to_string())).collect()
                                        };
                                        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(parts.len())))))
                                    }
                                    "substring" => {
                                        let start = args.get(0).map(|v| to_number(v) as usize).unwrap_or(0);
                                        let end = args.get(1).map(|v| to_number(v) as usize).unwrap_or(s.len());
                                        let start = start.min(s.len());
                                        let end = end.min(s.len());
                                        let start = start.min(end);
                                        Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
                                    }
                                    "slice" => {
                                        let start = args.get(0).map(|v| to_number(v) as i64).unwrap_or(0) as isize;
                                        let end = args.get(1).map(|v| to_number(v) as i64).unwrap_or(s.len() as i64) as isize;
                                        let len = s.len() as isize;
                                        let start = if start < 0 { (len + start).max(0) as usize } else { start as usize }.min(len as usize);
                                        let end = if end < 0 { (len + end).max(0) as usize } else { end as usize }.min(len as usize);
                                        let end = end.max(start);
                                        Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
                                    }
                                    "match" => {
                                        let pattern = args.first().map(|v| to_js_string(v)).unwrap_or_default();
                                        // Simple regex matching - just check if pattern is in string
                                        Ok(Value::Boolean(s.contains(&pattern)))
                                    }
                                    "search" => {
                                        let pattern = args.first().map(|v| to_js_string(v)).unwrap_or_default();
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
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => {
                    // For other value types (Number, Boolean, etc.), return undefined
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
            let result = call_value_with_this(constructor_val, args, Value::Object(Rc::clone(&new_obj_rc)))?;
            
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
    }
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

        BinaryOp::Eq => Ok(Value::Boolean(left == right)),
        BinaryOp::Neq => Ok(Value::Boolean(left != right)),
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
                Value::Function(_) | Value::NativeFunction(_) => "function",
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
                                        let needle = args.first().map(|v| to_js_string(v)).unwrap_or_default();
                                        Ok(Value::Number(s.find(&needle).map(|i| i as f64).unwrap_or(-1.0)))
                                    }
                                    "toUpperCase" => Ok(Value::String(s.to_uppercase())),
                                    "toLowerCase" => Ok(Value::String(s.to_lowercase())),
                                    "trim" => Ok(Value::String(s.trim().to_string())),
                                    "includes" => {
                                        let needle = args.first().map(|v| to_js_string(v)).unwrap_or_default();
                                        Ok(Value::Boolean(s.contains(&needle)))
                                    }
                                    "startsWith" => {
                                        let needle = args.first().map(|v| to_js_string(v)).unwrap_or_default();
                                        Ok(Value::Boolean(s.starts_with(&needle)))
                                    }
                                    "endsWith" => {
                                        let needle = args.first().map(|v| to_js_string(v)).unwrap_or_default();
                                        Ok(Value::Boolean(s.ends_with(&needle)))
                                    }
                                    _ => Ok(Value::Undefined),
                                }
                            })))
                        }
                        _ => Value::Undefined,
                    }
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
fn call_value_with_this(func: Value, args: Vec<Value>, this_val: Value) -> Result<Value, JsError> {
    match func {
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
    }
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
        Value::Object(o) => {
            // Try to call as constructor
            let obj = o.borrow();
            if let Some(constructor) = obj.get("constructor") {
                if matches!(constructor, Value::Function(_) | Value::NativeFunction(_)) {
                    drop(obj);
                    return call_value(constructor, args);
                }
            }
            Err(JsError("Object is not a function".to_string()))
        }
        _ => Err(JsError("Value is not a function".to_string())),
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
