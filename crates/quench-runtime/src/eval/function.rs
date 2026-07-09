//! Function calls

use crate::ast::{ArrowBody, Param};
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::statement::eval_statements;
use crate::value::{
    ClassValue, JsError, NativeConstructor, NativeFunction, Object, ObjectKind, Value, ValueFunction,
};
use crate::interpreter::{check_depth, predeclare_var, predeclare_let_const, release_depth};
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
        Value::NativeConstructor(nc) => call_native_constructor(nc, args),
        Value::Object(o) => call_object_as_constructor(o, args, this_val),
        Value::Class(class) => {
            if this_val != Value::Undefined {
                call_class_constructor(class, args, this_val)
            } else {
                // Direct class instantiation: new ClassName()
                instantiate_class(class, args)
            }
        }
        _ => Err(JsError("Value is not a function".to_string())),
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

fn call_js_function_impl(
    f: ValueFunction,
    args: Vec<Value>,
    this_val: Value,
) -> Result<Value, JsError> {
    let closure = Rc::clone(&f.closure);
    let params = f.params.clone();
    let mut call_env = Environment::with_parent(Rc::clone(&closure));
    call_env.current_scope_mut().set_this(this_val);
    let call_env_rc = Rc::new(RefCell::new(call_env));
    for (i, param) in params.iter().enumerate() {
        let arg = args.get(i).cloned();
        let value = match arg {
            Some(Value::Undefined) if param.default.is_some() => {
                eval_expression(&param.default.as_ref().unwrap(), &call_env_rc)?
            }
            Some(v) => v,
            None if param.default.is_some() => eval_expression(&param.default.as_ref().unwrap(), &call_env_rc)?,
            None => Value::Undefined,
        };
        call_env_rc.borrow_mut().define(param.name.clone(), value);
    }
    // Create arguments object for non-arrow functions
    if !f.is_arrow {
        let args_obj = create_arguments_object(&f, args);
        call_env_rc.borrow_mut().define("arguments".to_string(), args_obj);
        predeclare_var(&f.body, &mut call_env_rc.borrow_mut());
        predeclare_let_const(&f.body, &mut call_env_rc.borrow_mut());
    }
    if f.is_arrow {
        call_arrow_body(&f, &call_env_rc)
    } else {
        eval_statements(&f.body, &call_env_rc, false)
    }
}

/// Create the JavaScript arguments object for a function call
fn create_arguments_object(f: &ValueFunction, args: Vec<Value>) -> Value {
    let mut obj = Object::new(ObjectKind::Ordinary);
    // Set indexed arguments (arguments[0], arguments[1], etc.)
    for (i, arg) in args.iter().enumerate() {
        obj.set(&i.to_string(), arg.clone());
    }
    // Set length property
    obj.set("length", Value::Number(args.len() as f64));
    // Set callee property (the function itself)
    obj.set("callee", Value::Function(f.clone()));
    Value::Object(Rc::new(RefCell::new(obj)))
}

fn call_arrow_body(f: &ValueFunction, call_env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    if let Some(arrow_body) = f.arrow_body.as_ref() {
        match arrow_body {
            ArrowBody::Expression(expr) => eval_expression(expr, call_env),
            ArrowBody::Block(stmts) => eval_statements(stmts, call_env, true),
        }
    } else {
        Ok(Value::Undefined)
    }
}

fn call_native_function(
    nf: Rc<NativeFunction>,
    args: Vec<Value>,
    this_val: Value,
) -> Result<Value, JsError> {
    crate::interpreter::set_native_this(this_val);
    nf.call(args)
}

fn call_native_constructor(nc: Rc<NativeConstructor>, args: Vec<Value>) -> Result<Value, JsError> {
    nc.call(args)
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
        let result = call_value_with_this(constructor, args, Value::Object(Rc::clone(&new_obj_rc)))?;
        // If the constructor returns an object, use it; otherwise use new_obj
        if matches!(result, Value::Object(_)) {
            Ok(result)
        } else {
            Ok(Value::Object(new_obj_rc))
        }
    } else {
        Err(JsError("Object is not a constructor".to_string()))
    }
}

/// Instantiate a class with new ClassName()
fn instantiate_class(class: ClassValue, args: Vec<Value>) -> Result<Value, JsError> {
    // Create the prototype object with methods
    let prototype = create_class_prototype(&class, None)?;
    
    // Create the new instance object
    let mut instance = Object::new(ObjectKind::Ordinary);
    instance.prototype = Some(prototype);
    let instance_rc = Rc::new(RefCell::new(instance));
    
    // Call the constructor with the instance as 'this'
    let result = call_class_constructor(class, args, Value::Object(Rc::clone(&instance_rc)))?;
    
    // If constructor returns an object, use it; otherwise use the instance
    match result {
        Value::Object(_) | Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) | Value::Class(_) => Ok(result),
        _ => Ok(Value::Object(instance_rc)),
    }
}

/// Call a class constructor with 'this' bound to the instance
fn call_class_constructor(
    class: ClassValue,
    args: Vec<Value>,
    this_val: Value,
) -> Result<Value, JsError> {
    let params = class.constructor_params;
    let body = class.constructor_body;
    
    // Clone this_val for potential return
    let this_clone = this_val.clone();
    
    // For ES6 classes, the constructor body should be evaluated in a new scope
    // but with 'this' bound to the instance
    let mut call_env = Environment::with_parent(Rc::new(RefCell::new(Environment::new())));
    call_env.current_scope_mut().set_this(this_val);
    
    // Bind constructor parameters
    for (i, param) in params.iter().enumerate() {
        let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
        call_env.define(param.clone(), arg);
    }
    
    // Set up super binding if there's a superclass
    if class.super_class.is_some() {
        let super_proto = crate::builtins::get_object_prototype();
        if let Some(proto) = super_proto {
            call_env.define("__super__".to_string(), Value::Object(proto));
        }
    }
    
    // Create arguments object
    let args_obj = create_arguments_object_simple(args.clone());
    call_env.define("arguments".to_string(), args_obj);
    
    let call_env = Rc::new(RefCell::new(call_env));
    
    if body.is_empty() {
        // ES6 class constructors implicitly return 'this' if no explicit return
        Ok(this_clone)
    } else {
        predeclare_let_const(&body, &mut call_env.borrow_mut());
        eval_statements(&body, &call_env, false)
    }
}

/// Create a simple arguments object without needing ValueFunction
fn create_arguments_object_simple(args: Vec<Value>) -> Value {
    let mut obj = Object::new(ObjectKind::Ordinary);
    for (i, arg) in args.iter().enumerate() {
        obj.set(&i.to_string(), arg.clone());
    }
    obj.set("length", Value::Number(args.len() as f64));
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Create the prototype object for a class with methods
fn create_class_prototype(
    class: &ClassValue,
    _parent_proto: Option<Rc<RefCell<Object>>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    // Get parent prototype from superclass
    let parent_proto = if let Some(ref super_class) = class.super_class {
        // Evaluate the superclass expression to get its prototype
        let super_class_val = eval_expression(
            super_class,
            &Rc::new(RefCell::new(Environment::new())),
        )?;
        get_prototype_from_value(&super_class_val)
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
    let closure = Rc::new(RefCell::new(Environment::new()));
    for (name, params, body) in &class.methods {
        let params_vec: Vec<Param> = params.iter().map(|p| Param::new(p)).collect();
        let func = ValueFunction::new(
            Some(prop_key_to_string(name)),
            params_vec,
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
    
    let proto_rc = Rc::new(RefCell::new(proto));
    
    // Set constructor property pointing to the class (will be fixed up)
    // Note: The actual constructor will be set when we have the class object
    
    Ok(proto_rc)
}

/// Get the prototype object from a class value
fn get_prototype_from_value(val: &Value) -> Option<Rc<RefCell<Object>>> {
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
            // Create prototype for the class if not cached
            create_class_prototype(class, None).ok()
        }
        _ => None,
    }
}

/// Convert PropertyKey to string for property names
fn prop_key_to_string(key: &crate::ast::PropertyKey) -> String {
    match key {
        crate::ast::PropertyKey::Ident(s) => s.clone(),
        crate::ast::PropertyKey::String(s) => s.clone(),
        crate::ast::PropertyKey::Number(n) => n.to_string(),
        crate::ast::PropertyKey::Computed(_) => "[computed]".to_string(),
    }
}
