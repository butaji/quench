//! Expression evaluation
//!
//! This module contains the main expression evaluator and helper functions
//! for evaluating different expression types.

use crate::ast::*;
use crate::env::Environment;
use crate::eval::function::call_value_with_this;
use crate::eval::iteration::{get_enumerable_keys, get_iterator};
use crate::eval::member::eval_member_access;
use crate::eval::object::{assign_to, eval_callee_with_this};
use crate::eval::operators::{eval_binary_op, eval_unary_op};
use crate::eval::statement::eval_statement;
use crate::interpreter::{
    get_this_binding, predeclare_let_const, take_control_flow, ControlFlow,
};
use crate::value::{
    ClassValue, to_js_string, to_number, to_bool, JsError, Object, ObjectKind, Value, ValueFunction,
};
use std::cell::RefCell;
use std::rc::Rc;

pub use crate::eval::statement::eval_statements;

/// Evaluate an expression
pub fn eval_expression(
    expr: &Expression,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match expr {
        Expression::Number(n) => Ok(Value::Number(*n)),
        Expression::String(s) => Ok(Value::String(s.clone())),
        Expression::Boolean(b) => Ok(Value::Boolean(*b)),
        Expression::Null => Ok(Value::Null),
        Expression::Undefined => Ok(Value::Undefined),
        Expression::Identifier(name) => eval_identifier(name, env),
        Expression::Object(props) => eval_object_literal(props, env),
        Expression::Array(elements) => eval_array_literal(elements, env),
        Expression::FunctionExpression { name, params, body } => {
            Ok(Value::Function(crate::value::ValueFunction::new(
                name.clone(),
                params.clone(),
                body.clone(),
                Rc::clone(env),
            )))
        }
        Expression::ArrowFunction { params, body } => {
            Ok(Value::Function(crate::value::ValueFunction::new_arrow(
                params.clone(),
                body.clone(),
                Rc::clone(env),
            )))
        }
        Expression::Binary { op, left, right } => {
            let left_val = eval_expression(left, env)?;
            let right_val = eval_expression(right, env)?;
            eval_binary_op(*op, &left_val, &right_val)
        }
        Expression::Unary { op, argument } => eval_unary_expr(*op, argument, env),
        Expression::Assignment { left, right } => {
            let right_val = eval_expression(right, env)?;
            assign_to(left, &right_val, env)?;
            Ok(right_val)
        }
        Expression::CompoundAssignment { op, left, right } => {
            let left_val = eval_expression(left, env)?;
            let right_val = eval_expression(right, env)?;
            let result = eval_binary_op(op.to_binary(), &left_val, &right_val)?;
            assign_to(left, &result, env)?;
            Ok(result)
        }
        Expression::Call { callee, arguments } => eval_call(callee, arguments, env),
        Expression::Member { object, property, computed } => {
            eval_member(object, property, *computed, env)
        }
        Expression::Conditional { condition, consequent, alternate } => {
            if to_bool(&eval_expression(condition, env)?) {
                eval_expression(consequent, env)
            } else {
                eval_expression(alternate, env)
            }
        }
        Expression::Update { op, argument, prefix } => {
            eval_update(*op, argument, *prefix, env)
        }
        Expression::New { constructor, arguments } => {
            eval_new(constructor, arguments, env)
        }
        Expression::Sequence(exprs) => eval_sequence(exprs, env),
        Expression::BlockExpr(stmts) => eval_block_expr(stmts, env),
        Expression::ArrayPattern(_) => {
            Err(JsError("Array pattern must be used in assignment context".to_string()))
        }
        Expression::ObjectPattern(_) => {
            Err(JsError("Object pattern must be used in assignment context".to_string()))
        }
        Expression::ForOf { variable, iterable, body } => {
            eval_for_of(variable, iterable, body, env)
        }
        Expression::ForIn { variable, object, body } => {
            eval_for_in(variable, object, body, env)
        }
        Expression::OptChain { .. } | Expression::OptChainCall { .. } => {
            Err(JsError("Internal error: optional chaining not lowered".to_string()))
        }
        Expression::JsxElement { tag, props, children } => {
            eval_jsx_element(tag, props, children, env)
        }
        Expression::JsxFragment { children } => {
            eval_jsx_fragment(children, env)
        }
        Expression::Class(class) => {
            eval_class_expr(class, env)
        }
        Expression::Spread(_) => {
            Err(JsError("Spread must be used inside an array literal context".to_string()))
        }
    }
}

fn eval_jsx_element(
    tag: &crate::ast::JsxTagName,
    props: &[crate::ast::JsxProp],
    children: &[crate::ast::JsxChild],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    use crate::ast::{JsxAttrValue, JsxChild, JsxProp, JsxTagName};
    
    // Build the createElement call
    let create_element_fn = Expression::Member {
        object: Box::new(Expression::Identifier("ink".to_string())),
        property: crate::ast::PropertyKey::Ident("createElement".to_string()),
        computed: false,
    };
    
    // Build props object
    let mut prop_entries: Vec<(crate::ast::PropertyKey, crate::ast::PropertyValue)> = Vec::new();
    for prop in props {
        match prop {
            JsxProp::Attr { name, value } => {
                let prop_value = match value {
                    JsxAttrValue::String(s) => crate::ast::PropertyValue::Value(Expression::String(s.clone())),
                    JsxAttrValue::Expression(expr) => crate::ast::PropertyValue::Value(expr.clone()),
                };
                prop_entries.push((crate::ast::PropertyKey::Ident(name.clone()), prop_value));
            }
            JsxProp::Spread(_expr) => {
                // Spread is handled specially - we skip it for now
                // Full spread support would require Object.assign semantics
            }
        }
    }
    
    // Build props object expression
    let props_expr = Expression::Object(prop_entries);
    
    // Arguments: [tag, props, ...children]
    let mut args = vec![Expression::String(match tag {
        JsxTagName::Ident(name) => name.clone(),
        JsxTagName::Member { object, property } => format!("{}.{}", object, property),
        JsxTagName::Namespaced { namespace, name } => format!("{}:{}", namespace, name),
    })];
    args.push(props_expr);
    for child in children {
        match child {
            JsxChild::Text(s) => args.push(Expression::String(s.clone())),
            JsxChild::Expression(expr) => args.push(expr.clone()),
            JsxChild::Spread(_) => {}
            JsxChild::Element(jsx_expr) => args.push((**jsx_expr).clone()),
        }
    }
    
    // Call createElement
    let call_expr = Expression::Call {
        callee: Box::new(create_element_fn),
        arguments: args,
    };
    
    eval_expression(&call_expr, env)
}

fn eval_jsx_fragment(
    children: &[crate::ast::JsxChild],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Fragment is lowered to: createElement(Fragment, null, ...children)
    let create_element_fn = Expression::Member {
        object: Box::new(Expression::Identifier("ink".to_string())),
        property: crate::ast::PropertyKey::Ident("createElement".to_string()),
        computed: false,
    };
    
    let mut args = vec![
        Expression::Member {
            object: Box::new(Expression::Identifier("ink".to_string())),
            property: crate::ast::PropertyKey::Ident("Fragment".to_string()),
            computed: false,
        },
        Expression::Null,
    ];
    
    for child in children {
        match child {
            crate::ast::JsxChild::Text(s) => args.push(Expression::String(s.clone())),
            crate::ast::JsxChild::Expression(expr) => args.push(expr.clone()),
            crate::ast::JsxChild::Spread(_) => {}
            crate::ast::JsxChild::Element(jsx_expr) => args.push((**jsx_expr).clone()),
        }
    }
    
    let call_expr = Expression::Call {
        callee: Box::new(create_element_fn),
        arguments: args,
    };
    
    eval_expression(&call_expr, env)
}

fn eval_identifier(name: &str, env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    if name == "this" {
        return Ok(get_this_binding(env));
    }
    if name == "super" {
        return eval_super(env);
    }
    if env.borrow().is_tdz(name) {
        return Err(JsError(format!(
            "ReferenceError: Cannot access '{}' before initialization",
            name
        )));
    }
    env.borrow()
        .get(name)
        .ok_or_else(|| JsError(format!("ReferenceError: {} is not defined", name)))
}

fn eval_object_literal(
    props: &[(PropertyKey, PropertyValue)],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let mut obj = Object::new(ObjectKind::Ordinary);
    if let Some(prototype) = crate::builtins::get_object_prototype() {
        obj.prototype = Some(prototype);
    }
    for (key, value) in props {
        let key_str = eval_property_key(key, env)?;
        match value {
            PropertyValue::Value(expr) => {
                let val = eval_expression(expr, env)?;
                obj.set(&key_str, val);
            }
            PropertyValue::Getter { params: _, body } => {
                obj.set_getter(&key_str, Rc::new(body.clone()), Rc::clone(env));
            }
            PropertyValue::Setter { param, body } => {
                obj.set_setter(&key_str, param.clone(), Rc::new(body.clone()), Rc::clone(env));
            }
        }
    }
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

fn eval_property_key(key: &PropertyKey, env: &Rc<RefCell<Environment>>) -> Result<String, JsError> {
    match key {
        PropertyKey::Ident(s) => Ok(s.clone()),
        PropertyKey::String(s) => Ok(s.clone()),
        PropertyKey::Number(n) => Ok(n.to_string()),
        PropertyKey::Computed(e) => Ok(to_js_string(&eval_expression(e, env)?)),
    }
}

fn eval_array_literal(
    elements: &[Expression],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let mut arr = Object::new_array(0);
    for elem_expr in elements.iter() {
        match elem_expr {
            Expression::Spread(spread_expr) => {
                let spread_val = eval_expression(spread_expr, env)?;
                let items = crate::eval::iteration::get_iterator(&spread_val)?;
                for item in items {
                    let idx = arr.elements.len();
                    arr.set(&idx.to_string(), item);
                }
            }
            _ => {
                let value = eval_expression(elem_expr, env)?;
                let idx = arr.elements.len();
                arr.set(&idx.to_string(), value);
            }
        }
    }
    if let Some(prototype) = crate::builtins::get_array_prototype() {
        arr.prototype = Some(prototype);
    }
    Ok(Value::Object(Rc::new(RefCell::new(arr))))
}

fn eval_unary_expr(
    op: UnaryOp,
    argument: &Expression,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if op == UnaryOp::Typeof {
        if let Expression::Identifier(name) = argument {
            if name != "this" && !env.borrow().has(name) {
                return Ok(Value::String("undefined".to_string()));
            }
        }
    }
    let val = eval_expression(argument, env)?;
    eval_unary_op(op, &val)
}

fn eval_call(
    callee: &Expression,
    arguments: &[Expression],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let (func, this_val) = eval_callee_with_this(callee, env)?;
    let args: Result<Vec<Value>, _> = arguments.iter().map(|a| eval_expression(a, env)).collect();
    call_value_with_this(func, args?, this_val)
}

fn eval_member(
    object: &Expression,
    property: &PropertyKey,
    _computed: bool,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let obj_val = eval_expression(object, env)?;
    let prop_name = eval_property_key(property, env)?;
    eval_member_access(&obj_val, &prop_name, env)
}

fn eval_update(
    op: UpdateOp,
    argument: &Expression,
    prefix: bool,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let current = eval_expression(argument, env)?;
    let current_num = to_number(&current);
    let new_val = match op {
        UpdateOp::Increment => current_num + 1.0,
        UpdateOp::Decrement => current_num - 1.0,
    };
    assign_to(argument, &Value::Number(new_val), env)?;
    if prefix {
        Ok(Value::Number(new_val))
    } else {
        Ok(Value::Number(current_num))
    }
}

fn eval_new(
    constructor: &Expression,
    arguments: &[Expression],
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let constructor_val = eval_expression(constructor, env)?;
    let args: Result<Vec<Value>, _> = arguments.iter().map(|a| eval_expression(a, env)).collect();
    let args = args?;

    // Handle class instantiation
    if let Value::Class(class) = &constructor_val {
        return instantiate_class_from_ast_with_env(class.clone(), args, env);
    }

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

    let prototype = get_constructor_prototype(&constructor_val)?;
    let new_obj = if let Some(proto) = prototype {
        Object::with_prototype(ObjectKind::Ordinary, proto)
    } else {
        Object::new(ObjectKind::Ordinary)
    };
    let new_obj_rc = Rc::new(RefCell::new(new_obj));

    let result = call_value_with_this(
        actual_constructor,
        args,
        Value::Object(Rc::clone(&new_obj_rc)),
    )?;

    let use_constructor_result = match &constructor_val {
        Value::NativeConstructor(_) => true,
        Value::Function(f) => f.body.iter().any(Statement::has_explicit_return),
        _ => false,
    };

    if use_constructor_result && matches!(result, Value::Object(_)) {
        Ok(result)
    } else {
        Ok(Value::Object(new_obj_rc))
    }
}

/// Instantiate a class from its AST representation
/// Takes the class, arguments, and the environment for evaluating superclass
fn instantiate_class_from_ast_with_env(
    class: crate::value::ClassValue,
    args: Vec<Value>,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Get or create the prototype for this class
    let proto_rc = get_or_create_class_prototype(&class, env)?;
    
    // Create the new instance object
    let mut instance = Object::new(ObjectKind::Ordinary);
    instance.prototype = Some(Rc::clone(&proto_rc));
    let instance_rc = Rc::new(RefCell::new(instance));
    
    // Set constructor property on prototype pointing to this class
    // (will be used for instanceof checks)
    proto_rc.borrow_mut().set("constructor", Value::Object(Rc::clone(&instance_rc)));
    
    // Call the constructor
    let params = class.constructor_params.clone();
    let body = class.constructor_body.clone();
    let this_val = Value::Object(Rc::clone(&instance_rc));
    
    let mut call_env = Environment::with_parent(Rc::clone(env));
    call_env.current_scope_mut().set_this(this_val.clone());
    
    for (i, param) in params.iter().enumerate() {
        let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
        call_env.define(param.clone(), arg);
    }
    
    let args_obj = create_arguments_object_simple(args.clone());
    call_env.define("arguments".to_string(), args_obj);
    
    let call_env = Rc::new(RefCell::new(call_env));
    
    let result = if body.is_empty() {
        // For classes with no explicit constructor:
        // - If there's a superclass, call super(...args)
        // - Otherwise, just return this
        if class.super_class.is_some() {
            // Call super constructor with arguments
            let super_class_val = eval_expression(
                class.super_class.as_ref().unwrap(),
                env,
            )?;
            // Get super constructor and call it
            match super_class_val {
                Value::Class(super_class) => {
                    // Instantiate the superclass with the same args
                    instantiate_class_from_ast_with_env(super_class, args, env)?
                }
                Value::Object(o) => {
                    // Call the constructor from the object
                    if let Some(Value::Function(constructor)) = o.borrow().get("constructor") {
                        crate::eval::function::call_value_with_this(
                            Value::Function(constructor.clone()),
                            args,
                            this_val.clone(),
                        )?
                    } else {
                        this_val.clone()
                    }
                }
                _ => this_val.clone(),
            }
        } else {
            this_val.clone()
        }
    } else {
        predeclare_let_const(&body, &mut call_env.borrow_mut());
        eval_statements(&body, &call_env, false)?
    };
    
    // If constructor returns an object, use it; otherwise use the instance
    match result {
        Value::Object(_) | Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => Ok(result),
        _ => Ok(this_val),
    }
}

/// Instantiate a class from its AST representation (legacy signature)
fn instantiate_class_from_ast(
    class: crate::value::ClassValue,
    args: Vec<Value>,
) -> Result<Value, JsError> {
    instantiate_class_from_ast_with_env(class, args, &Rc::new(RefCell::new(Environment::new())))
}

/// Helper to create a simple arguments object
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
    class: &crate::value::ClassValue,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    // Check if prototype is already cached (all ClassValue clones share the cache)
    {
        let cell = class.prototype_cell.borrow();
        if let Some(ref proto) = *cell {
            return Ok(Rc::clone(proto));
        }
    }
    
    // Create the prototype
    let proto_rc = create_class_prototype_helper_with_env(class, env)?;
    
    // Cache it (the set_prototype method handles the nested Rc)
    {
        let mut cell = class.prototype_cell.borrow_mut();
        *cell = Some(Rc::clone(&proto_rc));
    }
    
    Ok(proto_rc)
}

/// Get prototype from a class value (used for extends)
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
            // Try to get cached prototype (all ClassValue clones share the cache)
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
    class: &crate::value::ClassValue,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    // Get parent prototype from superclass
    let parent_proto = if let Some(ref super_class) = class.super_class {
        let super_class_val = eval_expression(
            super_class,
            env,
        )?;
        get_prototype_from_class_val(&super_class_val)
    } else {
        crate::builtins::get_object_prototype()
    };
    
    // Create the prototype object inheriting from parent
    let mut proto = if let Some(parent) = parent_proto {
        Object::with_prototype(ObjectKind::Ordinary, parent)
    } else {
        Object::new(ObjectKind::Ordinary)
    };
    
    // Add methods to prototype
    let closure = Rc::clone(env);
    for (name, params, body) in &class.methods {
        let func = ValueFunction::new(
            Some(prop_key_to_string(name)),
            params.clone(),
            body.clone(),
            Rc::clone(&closure),
        );
        proto.set(&prop_key_to_string(name), Value::Function(func));
    }
    
    // Add getters to prototype
    for (name, body) in &class.getters {
        let key = prop_key_to_string(name);
        proto.set_getter(&key, Rc::new(body.clone()), Rc::clone(&closure));
    }
    
    // Add setters to prototype
    for (name, param, body) in &class.setters {
        let key = prop_key_to_string(name);
        proto.set_setter(&key, param.clone(), Rc::new(body.clone()), Rc::clone(&closure));
    }
    
    Ok(Rc::new(RefCell::new(proto)))
}

/// Legacy helper for creating prototype without environment (for operators.rs)
fn create_class_prototype_helper(class: &crate::value::ClassValue) -> Result<Rc<RefCell<Object>>, JsError> {
    create_class_prototype_helper_with_env(class, &Rc::new(RefCell::new(Environment::new())))
}

/// Helper to convert PropertyKey to string
fn prop_key_to_string(key: &crate::ast::PropertyKey) -> String {
    match key {
        crate::ast::PropertyKey::Ident(s) => s.clone(),
        crate::ast::PropertyKey::String(s) => s.clone(),
        crate::ast::PropertyKey::Number(n) => n.to_string(),
        crate::ast::PropertyKey::Computed(_) => "[computed]".to_string(),
    }
}

/// Create a class marker for instanceof checks
fn constructor_val_to_class_marker(class: &crate::value::ClassValue) -> Value {
    // Return the class itself as a marker - used for instanceof checks
    Value::Class(class.clone())
}

fn get_constructor_prototype(val: &Value) -> Result<Option<Rc<RefCell<Object>>>, JsError> {
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

fn eval_sequence(exprs: &[Expression], env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    let mut last = Value::Undefined;
    for e in exprs {
        last = eval_expression(e, env)?;
    }
    Ok(last)
}

fn eval_block_expr(stmts: &[Statement], env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    let mut last = Value::Undefined;
    for stmt in stmts {
        last = eval_statement(stmt, env, false)?;
    }
    Ok(last)
}

fn eval_for_of(
    variable: &Expression,
    iterable: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let iter_value = eval_expression(iterable, env)?;
    let items = get_iterator(&iter_value)?;
    let mut last = Value::Undefined;
    for item in items {
        assign_to(variable, &item, env)?;
        last = eval_statement(body, env, false)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    Ok(last)
}

fn eval_for_in(
    variable: &Expression,
    object: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let obj_value = eval_expression(object, env)?;
    let keys = get_enumerable_keys(&obj_value)?;
    let mut last = Value::Undefined;
    for key in keys {
        assign_to(variable, &Value::String(key), env)?;
        last = eval_statement(body, env, false)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    Ok(last)
}

fn eval_super(_env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    Err(JsError("ReferenceError: super is only valid in class methods".to_string()))
}

// Cache for ClassValue instances to ensure the same instance is reused
use std::collections::HashMap;

// Use thread-local storage for the cache
thread_local! {
    static CLASS_VALUE_CACHE: std::cell::RefCell<HashMap<usize, ClassValue>> = std::cell::RefCell::new(HashMap::new());
}

fn eval_class_expr(class: &Class, env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    // Use pointer address of the Class AST node as cache key
    let class_ptr = class as *const Class as usize;
    
    // Check if we already have a ClassValue for this AST node
    let class_value = CLASS_VALUE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(cached) = cache.get(&class_ptr) {
            cached.clone()
        } else {
            let new_value = ClassValue::from_ast(class);
            cache.insert(class_ptr, new_value.clone());
            new_value
        }
    });
    
    // Eagerly create the prototype for this class
    // This ensures that when a subclass extends this class, the parent prototype
    // is already available for building the inheritance chain
    let _ = get_or_create_class_prototype(&class_value, env)?;
    
    Ok(Value::Class(class_value))
}


