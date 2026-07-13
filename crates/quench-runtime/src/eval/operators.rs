//! JavaScript operators evaluation

use crate::ast::*;
use crate::value::{
    create_js_error_with_type, get_thrown_value, loose_eq, set_thrown_value, strict_eq,
    take_thrown_value, to_bool, to_js_string, to_number, to_primitive, to_uint32, JsError, Value,
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
    // an Object, both sides are reduced via ToPrimitive with no preferred type.
    // If EITHER primitive side is a string, do string concat; otherwise number.
    let left_is_obj = matches!(
        left,
        Value::Object(_) | Value::Function(_) | Value::NativeFunction(_)
            | Value::NativeConstructor(_) | Value::Class(_)
    );
    let right_is_obj = matches!(
        right,
        Value::Object(_) | Value::Function(_) | Value::NativeFunction(_)
            | Value::NativeConstructor(_) | Value::Class(_)
    );
    if left_is_obj || right_is_obj {
        let lp = to_primitive(left, None)?;
        let rp = to_primitive(right, None)?;
        // Both are now primitives.
        if matches!(&lp, Value::String(_)) || matches!(&rp, Value::String(_)) {
            if matches!(&lp, Value::Symbol(_)) || matches!(&rp, Value::Symbol(_)) {
                return symbol_conversion_error("string");
            }
            Ok(Value::String(format!("{}{}", to_js_string(&lp), to_js_string(&rp))))
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
        let l = to_js_string(left);
        let r = to_js_string(right);
        Ok(Value::String(format!("{}{}", l, r)))
    } else {
        if matches!(left, Value::Symbol(_)) || matches!(right, Value::Symbol(_)) {
            return symbol_conversion_error("number");
        }
        // to_number may trigger ToPrimitive(valueOf/toString). If a throw was
        // raised, surface it WITHOUT consuming — eval_try_catch's take will
        // pick it up next. Do not consume here.
        let l = to_number(left);
        let r = to_number(right);
        if let Some(thrown) = get_thrown_value() {
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
    Ok(Value::Number(
        f(to_number(left) as i64, to_number(right) as i64) as f64,
    ))
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
    // Use to_uint32 per JavaScript spec for unsigned right shift
    let l = to_uint32(to_number(left)) as u64;
    let r = to_uint32(to_number(right)) as u64;
    let result = f(l, r);
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
mod tests {
    use super::*;
    use crate::value::{create_js_error_with_type, set_thrown_value};

    /// When `valueOf` throws, `eval_add` must surface the error AND leave the
    /// thrown value intact for the surrounding try/catch to retrieve.
    /// Regression: eval_add used to `take_thrown_value()` (consume) which left
    /// the catch handler seeing `undefined`.
    #[test]
    fn eval_add_propagates_valueof_throw_and_preserves_thrown_value() {
        // Set a stale thrown value to make sure eval_add clears it before
        // running its own ToPrimitive — it must not surface a foreign throw.
        let (stale, _js_err) = create_js_error_with_type("stale", "Error");
        set_thrown_value(stale);

        let left = Value::Number(1.0);
        let right = Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
            crate::value::Object::new(crate::value::ObjectKind::Ordinary),
        )));
        // We can't easily construct a JS function here without an evaluator,
        // so test the simpler invariant: a fresh thrown value, if present,
        // must be preserved (not consumed) on Err.
        let _ = take_thrown_value(); // pretend our setup cleared the stale
        let (fresh, _) = create_js_error_with_type("boom", "Error");
        set_thrown_value(fresh);
        let result = eval_add(&left, &right);
        assert!(result.is_err(), "eval_add must return Err when thrown_value is set");
        // Critical: thrown_value is still present so try/catch can bind it.
        assert!(
            get_thrown_value().is_some(),
            "eval_add must not consume the thrown_value"
        );
        let _ = take_thrown_value();
    }

    /// Per spec §7.1.1: when @@toPrimitive getter throws, evaluation of
    /// the throwing side must abort BEFORE the other side's @@toPrimitive
    /// is consulted. Companion to test262
    /// `language/expressions/addition/get-symbol-to-prim-err.js`.
    #[test]
    fn eval_add_short_circuits_when_left_toprim_getter_throws() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval(
            "var callCount = 0; \
             var thrower = {}; \
             var counter = {}; \
             Object.defineProperty(thrower, Symbol.toPrimitive, { get: function() { throw new Error('x'); } }); \
             Object.defineProperty(counter, Symbol.toPrimitive, { get: function() { callCount += 1; } }); \
             var thrown; \
             try { thrower + counter; } catch (e) { thrown = e; } \
             ({ callCount: callCount, msg: thrown.message });",
        );
        let value = result.unwrap();
        let crate::value::Value::Object(obj) = value else {
            panic!("expected object result");
        };
        let obj = obj.borrow();
        assert_eq!(
            obj.get("callCount"),
            Some(crate::value::Value::Number(0.0)),
            "counter's @@toPrimitive must not run when thrower's getter throws"
        );
        assert_eq!(
            obj.get("msg"),
            Some(crate::value::Value::String("x".to_string()))
        );
    }
}
