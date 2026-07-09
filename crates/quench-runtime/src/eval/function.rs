//! Function calls

use crate::ast::ArrowBody;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::statement::eval_statements;
use crate::value::{
    JsError, NativeConstructor, NativeFunction, Object, ObjectKind, Value, ValueFunction,
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
        Value::Function(f) => call_js_function(f, args, this_val),
        Value::NativeFunction(nf) => call_native_function(nf, args, this_val),
        Value::NativeConstructor(nc) => call_native_constructor(nc, args),
        Value::Object(o) => call_object_as_constructor(o, args, this_val),
        _ => Err(JsError("Value is not a function".to_string())),
    };
    release_depth();
    result
}

/// Call a value as a function (this defaults to undefined)
pub fn call_value(func: Value, args: Vec<Value>) -> Result<Value, JsError> {
    call_value_with_this(func, args, Value::Undefined)
}

fn call_js_function(
    f: ValueFunction,
    args: Vec<Value>,
    this_val: Value,
) -> Result<Value, JsError> {
    let closure = Rc::clone(&f.closure);
    let params = f.params.clone();
    let mut call_env = Environment::with_parent(Rc::clone(&closure));
    call_env.current_scope_mut().set_this(this_val);
    for (i, param) in params.iter().enumerate() {
        let arg = args.get(i).cloned().unwrap_or(Value::Undefined);
        call_env.define(param.clone(), arg);
    }
    // Create arguments object for non-arrow functions
    if !f.is_arrow {
        let args_obj = create_arguments_object(&f, args);
        call_env.define("arguments".to_string(), args_obj);
        predeclare_var(&f.body, &mut call_env);
        predeclare_let_const(&f.body, &mut call_env);
    }
    let call_env = Rc::new(RefCell::new(call_env));
    if f.is_arrow {
        call_arrow_body(&f, &call_env)
    } else {
        eval_statements(&f.body, &call_env, false)
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
    _this_val: Value,
) -> Result<Value, JsError> {
    let constructor_opt = {
        let obj = o.borrow();
        if let Some(constructor) = obj.get("constructor") {
            if matches!(
                constructor,
                Value::Function(_) | Value::NativeFunction(_)
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
        call_value_with_this(constructor, args, Value::Object(Rc::clone(&new_obj_rc)))
    } else {
        Err(JsError("Object is not a constructor".to_string()))
    }
}
