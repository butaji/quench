//! JavaScript operators evaluation

use crate::ast::*;
use crate::value::{to_js_string, to_number, to_bool, strict_eq, loose_eq, JsError, Value};

/// Evaluate a binary operator
pub fn eval_binary_op(op: BinaryOp, left: &Value, right: &Value) -> Result<Value, JsError> {
    match op {
        BinaryOp::Add => eval_add(left, right),
        BinaryOp::Sub => Ok(Value::Number(to_number(left) - to_number(right))),
        BinaryOp::Mul => Ok(Value::Number(to_number(left) * to_number(right))),
        BinaryOp::Div => Ok(Value::Number(to_number(left) / to_number(right))),
        BinaryOp::Mod => Ok(Value::Number(to_number(left) % to_number(right))),
        BinaryOp::Eq => Ok(Value::Boolean(loose_eq(left, right))),
        BinaryOp::Neq => Ok(Value::Boolean(!loose_eq(left, right))),
        BinaryOp::In => eval_in_op(left, right),
        BinaryOp::Instanceof => eval_instanceof(left, right),
        BinaryOp::StrictEq => Ok(Value::Boolean(strict_eq(left, right))),
        BinaryOp::StrictNeq => Ok(Value::Boolean(!strict_eq(left, right))),
        BinaryOp::Lt => Ok(Value::Boolean(to_number(left) < to_number(right))),
        BinaryOp::Gt => Ok(Value::Boolean(to_number(left) > to_number(right))),
        BinaryOp::Le => Ok(Value::Boolean(to_number(left) <= to_number(right))),
        BinaryOp::Ge => Ok(Value::Boolean(to_number(left) >= to_number(right))),
        BinaryOp::And => Ok(if to_bool(left) { right.clone() } else { left.clone() }),
        BinaryOp::Or => Ok(if to_bool(left) { left.clone() } else { right.clone() }),
        BinaryOp::NullishCoalescing => eval_nullish(left, right),
        BinaryOp::BitAnd => bit_op(left, right, |a, b| a & b),
        BinaryOp::BitOr => bit_op(left, right, |a, b| a | b),
        BinaryOp::BitXor => bit_op(left, right, |a, b| a ^ b),
        BinaryOp::Shl => shift_op(left, right, |a, b| a << b),
        BinaryOp::Shr => shift_op(left, right, |a, b| a >> b),
        BinaryOp::Ushr => shift_op_u(left, right, |a, b| a >> b),
    }
}

fn eval_add(left: &Value, right: &Value) -> Result<Value, JsError> {
    if matches!(left, Value::String(_)) || matches!(right, Value::String(_)) {
        Ok(Value::String(format!("{}{}", to_js_string(left), to_js_string(right))))
    } else {
        Ok(Value::Number(to_number(left) + to_number(right)))
    }
}

fn eval_in_op(left: &Value, right: &Value) -> Result<Value, JsError> {
    let prop_name = to_js_string(left);
    match right {
        Value::Object(obj) => Ok(Value::Boolean(obj.borrow().has(&prop_name))),
        Value::String(s) => {
            if let Ok(idx) = prop_name.parse::<usize>() {
                Ok(Value::Boolean(idx < s.chars().count()))
            } else {
                Ok(Value::Boolean(false))
            }
        }
        _ => Ok(Value::Boolean(false)),
    }
}

fn eval_instanceof(left: &Value, right: &Value) -> Result<Value, JsError> {
    // Walk prototype chain - check if target prototype is in the chain
    fn has_prototype_in_chain(
        obj: &crate::value::Object,
        target_proto: &std::rc::Rc<std::cell::RefCell<crate::value::Object>>,
    ) -> bool {
        // Check if this object's prototype is the target
        // Use pointer comparison on the underlying RefCell to handle Rc clones correctly
        if let Some(ref proto_rc) = obj.prototype {
            // Compare the underlying RefCell pointers
            let proto_ptr: *const std::cell::RefCell<crate::value::Object> = &**proto_rc;
            let target_ptr: *const std::cell::RefCell<crate::value::Object> = &**target_proto;
            if proto_ptr == target_ptr {
                return true;
            }
            // Walk up the prototype chain
            let proto_borrowed = proto_rc.borrow();
            if has_prototype_in_chain(&proto_borrowed, target_proto) {
                return true;
            }
        }
        false
    }

    match (left, right) {
        (_, Value::Undefined) | (_, Value::Null) => Ok(Value::Boolean(false)),
        (Value::Object(obj), Value::Function(ctor)) => {
            let ctor_proto = ctor.get_prototype();
            let result = has_prototype_in_chain(&obj.borrow(), &ctor_proto);
            Ok(Value::Boolean(result))
        }
        (Value::Object(obj), Value::NativeConstructor(ctor)) => {
            let result = has_prototype_in_chain(&obj.borrow(), &ctor.prototype);
            Ok(Value::Boolean(result))
        }
        (Value::Function(func), Value::NativeConstructor(ctor)) => {
            let func_proto = func.get_prototype();
            let result = has_prototype_in_chain(&func_proto.borrow(), &ctor.prototype);
            Ok(Value::Boolean(result))
        }
        (Value::Object(obj), Value::Object(ctor)) => {
            let ctor_ref = ctor.borrow();
            if let Some(Value::Object(proto)) = ctor_ref.get("prototype") {
                drop(ctor_ref);
                let result = has_prototype_in_chain(&obj.borrow(), &proto);
                Ok(Value::Boolean(result))
            } else {
                Ok(Value::Boolean(false))
            }
        }
        // Handle class instances: obj instanceof Class
        (Value::Object(obj), Value::Class(class)) => {
            // For instanceof with a class, check if the object's prototype chain
            // contains the class's prototype
            let class_proto = get_class_prototype_for_instanceof(class)?;
            let result = has_prototype_in_chain(&obj.borrow(), &class_proto);
            Ok(Value::Boolean(result))
        }
        _ => Ok(Value::Boolean(false)),
    }
}

fn eval_nullish(left: &Value, right: &Value) -> Result<Value, JsError> {
    match left {
        Value::Undefined | Value::Null => Ok(right.clone()),
        _ => Ok(left.clone()),
    }
}

fn bit_op<F>(left: &Value, right: &Value, f: F) -> Result<Value, JsError>
where
    F: FnOnce(i64, i64) -> i64,
{
    Ok(Value::Number(f(to_number(left) as i64, to_number(right) as i64) as f64))
}

fn shift_op<F>(left: &Value, right: &Value, f: F) -> Result<Value, JsError>
where
    F: FnOnce(i64, i64) -> i64,
{
    Ok(Value::Number(
        f(to_number(left) as i64, to_number(right) as i64) as f64,
    ))
}

fn shift_op_u<F>(left: &Value, right: &Value, f: F) -> Result<Value, JsError>
where
    F: FnOnce(u64, u64) -> u64,
{
    Ok(Value::Number(
        f(to_number(left) as u64, to_number(right) as u64) as f64,
    ))
}

/// Evaluate a unary operator
pub fn eval_unary_op(op: UnaryOp, val: &Value) -> Result<Value, JsError> {
    match op {
        UnaryOp::Not => Ok(Value::Boolean(!to_bool(val))),
        UnaryOp::Neg => Ok(Value::Number(-to_number(val))),
        UnaryOp::Plus => Ok(Value::Number(to_number(val))),
        UnaryOp::BitNot => Ok(Value::Number(!(to_number(val) as i64) as f64)),
        UnaryOp::Typeof => eval_typeof(val),
        UnaryOp::Void => Ok(Value::Undefined),
    }
}

fn eval_typeof(val: &Value) -> Result<Value, JsError> {
    let type_str = match val {
        Value::Undefined => "undefined",
        Value::Null => "object",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) | Value::Class(_) => "function",
        Value::Object(_) | Value::ObjectId(_) => "object",
        Value::Symbol(_) => "symbol",
    };
    Ok(Value::String(type_str.to_string()))
}

/// Get the prototype object for instanceof checks with class values
fn get_class_prototype_for_instanceof(class: &crate::value::ClassValue) -> Result<std::rc::Rc<std::cell::RefCell<crate::value::Object>>, JsError> {
    // Use the shared prototype from ClassValue
    // This ensures that instanceof checks work correctly
    crate::eval::expression::get_or_create_class_prototype(class, &std::rc::Rc::new(std::cell::RefCell::new(crate::env::Environment::new())))
}

/// Get prototype from a class value (used for extends)
fn get_prototype_from_class_val(val: &Value) -> Option<std::rc::Rc<std::cell::RefCell<crate::value::Object>>> {
    use crate::value::Object;
    use std::cell::RefCell;
    use std::rc::Rc;
    
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
            get_class_prototype_for_instanceof(class).ok()
        }
        _ => None,
    }
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
