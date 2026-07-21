//! JavaScript operators evaluation

use crate::ast::*;
use crate::value::{
    create_js_error_with_type, get_thrown_value, loose_eq, strict_eq, to_bool, to_js_string,
    to_number, to_primitive, to_uint32, JsError, Value,
};

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
        BinaryOp::Lt => eval_relational(left, right, |a, b| a < b),
        BinaryOp::Gt => eval_relational(left, right, |a, b| a > b),
        BinaryOp::Le => eval_relational(left, right, |a, b| a <= b),
        BinaryOp::Ge => eval_relational(left, right, |a, b| a >= b),
        BinaryOp::And => Ok(if to_bool(left) {
            right.clone()
        } else {
            left.clone()
        }),
        BinaryOp::Or => Ok(if to_bool(left) {
            left.clone()
        } else {
            right.clone()
        }),
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
    // Per ES §7.1.1 ToPrimitive and the + operator spec: if EITHER operand is
    // an Object, both sides are reduced via ToPrimitive. When one side is a
    // Date, the hint is "string" (Date -> toString is what users expect; this
    // also matches ES2015 §21.4.3.2 Date.prototype[@@toPrimitive] behavior).
    // If EITHER primitive side is a string, do string concat; otherwise number.
    let left_is_obj = matches!(
        left,
        Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
            | Value::Generator(_)
            | Value::Class(_)
    );
    let right_is_obj = matches!(
        right,
        Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
            | Value::Generator(_)
            | Value::Class(_)
    );
    let is_date = |v: &Value| matches!(v, Value::Object(o) if o.borrow().kind == crate::value::ObjectKind::Date);
    if left_is_obj || right_is_obj {
        // Date triggers string hint per ES §7.1.1 + Date.prototype[@@toPrimitive]
        // semantics; default hint still lets @@toPrimitive choose the order.
        let hint = if is_date(left) || is_date(right) {
            Some("string")
        } else {
            None
        };
        let lp = to_primitive(left, hint)?;
        let rp = to_primitive(right, hint)?;
        // Both are now primitives.
        // After ToPrimitive: surface any thrown value without consuming it.
        if let Some(thrown) = get_thrown_value() {
            return Err(JsError(to_js_string(&thrown)));
        }
        if matches!(&lp, Value::String(_)) || matches!(&rp, Value::String(_)) {
            if matches!(&lp, Value::Symbol(_)) || matches!(&rp, Value::Symbol(_)) {
                return symbol_conversion_error("string");
            }
            Ok(Value::String(format!(
                "{}{}",
                to_js_string(&lp),
                to_js_string(&rp)
            )))
        } else {
            if matches!(&lp, Value::Symbol(_)) || matches!(&rp, Value::Symbol(_)) {
                return symbol_conversion_error("number");
            }
            let l = to_number(&lp);
            let r = to_number(&rp);
            Ok(Value::Number(l + r))
        }
    } else if matches!(left, Value::String(_)) || matches!(right, Value::String(_)) {
        if matches!(left, Value::Symbol(_)) || matches!(right, Value::Symbol(_)) {
            return symbol_conversion_error("string");
        }
        // Surface any thrown value from earlier evaluation.
        if let Some(thrown) = get_thrown_value() {
            return Err(JsError(to_js_string(&thrown)));
        }
        let l = to_js_string(left);
        let r = to_js_string(right);
        Ok(Value::String(format!("{}{}", l, r)))
    } else {
        if matches!(left, Value::Symbol(_)) || matches!(right, Value::Symbol(_)) {
            return symbol_conversion_error("number");
        }
        // to_number may trigger ToPrimitive(valueOf/toString). Surface any
        // thrown value (even one that was set before eval_add) WITHOUT consuming
        // — eval_try_catch's take will pick it up next.
        let l = to_number(left);
        let r = to_number(right);
        if get_thrown_value().is_some() {
            let thrown = get_thrown_value().unwrap();
            return Err(JsError(to_js_string(&thrown)));
        }
        Ok(Value::Number(l + r))
    }
}

fn symbol_conversion_error(target: &str) -> Result<Value, JsError> {
    let (_, js_err) = create_js_error_with_type(
        &format!("Cannot convert a Symbol value to a {}", target),
        "TypeError",
    );
    Err(js_err)
}

/// Per ES spec §7.2.13 IsLessThan: if both operands are Strings, compare
/// lexicographically; otherwise coerce to Number and compare numerically.
fn eval_relational<F>(left: &Value, right: &Value, num_cmp: F) -> Result<Value, JsError>
where
    F: Fn(f64, f64) -> bool,
{
    if let (Value::String(a), Value::String(b)) = (left, right) {
        let cmp = string_compare(a, b);
        return Ok(Value::Boolean(num_cmp(cmp as f64, 0.0)));
    }
    Ok(Value::Boolean(num_cmp(to_number(left), to_number(right))))
}

fn string_compare(a: &str, b: &str) -> i32 {
    if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
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
        if let Some(ref proto_rc) = obj.prototype {
            // Use pointer comparison first (fast path for matching Rc clones)
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
        (Value::Object(obj), Value::NativeFunction(nf)) => {
            if let Some(Value::Object(proto)) = nf.get_property("prototype") {
                let result = has_prototype_in_chain(&obj.borrow(), &proto);
                Ok(Value::Boolean(result))
            } else {
                Ok(Value::Boolean(false))
            }
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
    // Per ES §7.1.3 ToNumber + §7.2.1 ToInt32: evaluate left first,
    // then right. Avoid calling to_number on both in sequence because
    // to_number swallows thrown values.
    let l = to_number(left);
    if let Some(thrown) = get_thrown_value() {
        return Err(JsError(to_js_string(&thrown)));
    }
    let r = to_number(right);
    if let Some(thrown) = get_thrown_value() {
        return Err(JsError(to_js_string(&thrown)));
    }
    Ok(Value::Number(f(l as i64, r as i64) as f64))
}

fn shift_op<F>(left: &Value, right: &Value, f: F) -> Result<Value, JsError>
where
    F: FnOnce(i64, i64) -> i64,
{
    let l = to_number(left);
    if let Some(thrown) = get_thrown_value() {
        return Err(JsError(to_js_string(&thrown)));
    }
    let r = to_number(right);
    if let Some(thrown) = get_thrown_value() {
        return Err(JsError(to_js_string(&thrown)));
    }
    // Per ES §12.9.3.1 / 12.9.4.1: shift count is masked to 5 bits (0-31).
    // This avoids Rust's panic on shifting by >= bit width.
    let count = (r as i64) & 0x1F;
    Ok(Value::Number(f(l as i64, count) as f64))
}

fn shift_op_u<F>(left: &Value, right: &Value, f: F) -> Result<Value, JsError>
where
    F: FnOnce(u64, u64) -> u64,
{
    // Use to_uint32 per JavaScript spec for unsigned right shift
    let l = to_uint32(to_number(left)) as u64;
    let r = to_uint32(to_number(right)) as u64;
    // Mask shift count to 5 bits (0-31) per ES §12.9.3.1 step 7.
    let count = r & 0x1F;
    let result = f(l, count);
    Ok(Value::Number(result as f64))
}

/// Evaluate a unary operator
/// Note: UnaryOp::Delete is handled specially in eval_unary_expr, not here.
pub fn eval_unary_op(op: UnaryOp, val: &Value) -> Result<Value, JsError> {
    match op {
        UnaryOp::Not => Ok(Value::Boolean(!to_bool(val))),
        UnaryOp::Neg => Ok(Value::Number(-to_number(val))),
        UnaryOp::Plus => Ok(Value::Number(to_number(val))),
        UnaryOp::BitNot => Ok(Value::Number(!(to_number(val) as i64) as f64)),
        UnaryOp::Typeof => eval_typeof(val),
        UnaryOp::Void => Ok(Value::Undefined),
        UnaryOp::Delete => Err(JsError("Delete should be handled specially".to_string())),
    }
}

fn eval_typeof(val: &Value) -> Result<Value, JsError> {
    let type_str = match val {
        Value::Undefined => "undefined",
        Value::Null => "object",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Class(_) => "function",
        Value::Generator(_) => "object",
        Value::BigInt(_) => "bigint",
        Value::Object(_) => "object",
        Value::Symbol(_) => "symbol",
    };
    Ok(Value::String(type_str.to_string()))
}

/// Get the prototype object for instanceof checks with class values
fn get_class_prototype_for_instanceof(
    class: &crate::value::ClassValue,
) -> Result<std::rc::Rc<std::cell::RefCell<crate::value::Object>>, JsError> {
    // Use the shared prototype from ClassValue
    // This ensures that instanceof checks work correctly
    crate::eval::class::get_or_create_class_prototype(
        class,
        &std::rc::Rc::new(std::cell::RefCell::new(crate::env::Environment::new())),
    )
}

/// Get prototype from a class value (used for extends)
#[allow(dead_code)]
fn get_prototype_from_class_val(
    val: &Value,
) -> Option<std::rc::Rc<std::cell::RefCell<crate::value::Object>>> {
    match val {
        Value::Object(o) => {
            let proto = o.borrow().get("prototype");
            if let Some(Value::Object(proto_obj)) = proto {
                Some(proto_obj.clone())
            } else {
                None
            }
        }
        Value::Class(class) => get_class_prototype_for_instanceof(class).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
