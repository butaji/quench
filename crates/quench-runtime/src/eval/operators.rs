//! JavaScript operators evaluation

use crate::ast::*;
use crate::value::{
    create_js_error_with_type, get_thrown_value, loose_eq, strict_eq, to_bool, to_js_string,
    to_number, to_primitive, to_uint32, JsError, Value,
};
use std::rc::Rc;

/// Evaluate a binary operator
pub fn eval_binary_op(op: BinaryOp, left: &Value, right: &Value) -> Result<Value, JsError> {
    match op {
        BinaryOp::Add => eval_add(left, right),
        BinaryOp::Sub => eval_numeric_arithmetic(left, right, '-'),
        BinaryOp::Mul => eval_numeric_arithmetic(left, right, '*'),
        BinaryOp::Div => eval_numeric_arithmetic(left, right, '/'),
        BinaryOp::Mod => eval_numeric_arithmetic(left, right, '%'),
        BinaryOp::Pow => eval_numeric_arithmetic(left, right, '^'),
        BinaryOp::Eq => Ok(Value::Boolean(loose_eq(left, right))),
        BinaryOp::Neq => Ok(Value::Boolean(!loose_eq(left, right))),
        BinaryOp::LooseEq => Ok(Value::Boolean(loose_eq(left, right))),
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
        BinaryOp::BitAnd => eval_bigint_bitwise(left, right, '&')
            .unwrap_or_else(|| bit_op(left, right, |a, b| a & b)),
        BinaryOp::BitOr => eval_bigint_bitwise(left, right, '|')
            .unwrap_or_else(|| bit_op(left, right, |a, b| a | b)),
        BinaryOp::BitXor => eval_bigint_bitwise(left, right, '^')
            .unwrap_or_else(|| bit_op(left, right, |a, b| a ^ b)),
        BinaryOp::Shl => eval_bigint_shift(left, right, true)
            .unwrap_or_else(|| shift_op(left, right, |a, b| a << b)),
        BinaryOp::Shr => eval_bigint_shift(left, right, false)
            .unwrap_or_else(|| shift_op(left, right, |a, b| a >> b)),
        BinaryOp::Ushr => shift_op_u(left, right, |a, b| a >> b),
    }
}

fn eval_bigint_shift(
    left: &Value,
    right: &Value,
    left_shift: bool,
) -> Option<Result<Value, JsError>> {
    let left = to_primitive(left, Some("number"));
    let right = to_primitive(right, Some("number"));
    let (Ok(Value::BigInt(left)), Ok(Value::BigInt(right))) = (&left, &right) else {
        return None;
    };
    let Ok(count) = right.to_string().parse::<usize>() else {
        return Some(crate::throw!(
            "RangeError",
            "BigInt shift count is too large"
        ));
    };
    let result = if left_shift {
        left.as_ref() << count
    } else {
        left.as_ref() >> count
    };
    Some(Ok(Value::BigInt(Rc::new(result))))
}

fn eval_bigint_bitwise(
    left: &Value,
    right: &Value,
    operator: char,
) -> Option<Result<Value, JsError>> {
    let left = match to_primitive(left, Some("number")) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if matches!(left, Value::Symbol(_)) {
        return Some(crate::throw!(
            "TypeError",
            "Cannot convert Symbol to number"
        ));
    }
    let right = match to_primitive(right, Some("number")) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    if matches!(right, Value::Symbol(_)) {
        return Some(crate::throw!(
            "TypeError",
            "Cannot convert Symbol to number"
        ));
    }
    match (&left, &right) {
        (Value::BigInt(left), Value::BigInt(right)) => {
            let result = match operator {
                '&' => left.as_ref() & right.as_ref(),
                '|' => left.as_ref() | right.as_ref(),
                '^' => left.as_ref() ^ right.as_ref(),
                _ => unreachable!(),
            };
            Some(Ok(Value::BigInt(Rc::new(result))))
        }
        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Some(crate::throw!(
            "TypeError",
            "Cannot mix BigInt and other types"
        )),
        _ => None,
    }
}

fn eval_numeric_arithmetic(left: &Value, right: &Value, operator: char) -> Result<Value, JsError> {
    let left = to_primitive(left, None)?;
    let right = to_primitive(right, None)?;
    match (&left, &right) {
        (Value::BigInt(left), Value::BigInt(right)) => {
            eval_bigint_arithmetic(left, right, operator)
        }
        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
            crate::throw!("TypeError", "Cannot mix BigInt and other types")
        }
        _ => {
            let left = to_number(&left);
            let right = to_number(&right);
            let value = match operator {
                '-' => left - right,
                '*' => left * right,
                '/' => left / right,
                '%' => left % right,
                '^' => left.powf(right),
                _ => unreachable!(),
            };
            Ok(Value::Number(value))
        }
    }
}

fn eval_bigint_arithmetic(
    left: &num_bigint::BigInt,
    right: &num_bigint::BigInt,
    operator: char,
) -> Result<Value, JsError> {
    if matches!(operator, '/' | '%') && right == &0.into() {
        return crate::throw!("RangeError", "Division by zero");
    }
    if operator == '^' && right < &0.into() {
        return crate::throw!("RangeError", "BigInt negative exponent");
    }
    let result = match operator {
        '-' => left - right,
        '*' => left * right,
        '/' => left / right,
        '%' => left % right,
        '^' => match right.to_string().parse::<u32>() {
            Ok(power) => left.pow(power),
            Err(_) => return crate::throw!("RangeError", "BigInt exponent is too large"),
        },
        _ => unreachable!(),
    };
    Ok(Value::BigInt(Rc::new(result)))
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
            // BigInt/Number mixed check
            let l_is_bigint = matches!(&lp, Value::BigInt(_));
            let r_is_bigint = matches!(&rp, Value::BigInt(_));
            if l_is_bigint != r_is_bigint {
                let (_, js_err) = crate::value::error::create_js_error_with_type(
                    "Cannot mix BigInt and other types",
                    "TypeError",
                );
                return Err(js_err);
            }
            if l_is_bigint && r_is_bigint {
                // BigInt addition
                let lb = crate::builtins::bigint::to_bigint_value(&lp)?;
                let rb = crate::builtins::bigint::to_bigint_value(&rp)?;
                return Ok(crate::builtins::bigint::bigint_to_value(lb + rb));
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
    } else if let (Value::BigInt(left), Value::BigInt(right)) = (left, right) {
        Ok(crate::builtins::bigint::bigint_to_value(
            left.as_ref() + right.as_ref(),
        ))
    } else {
        if matches!(left, Value::Symbol(_)) || matches!(right, Value::Symbol(_)) {
            return symbol_conversion_error("number");
        }
        // Per ES2025 §7.2.6 ApplyStringOrNumericBinaryOperator: if one operand
        // is BigInt and the other is not (Number, Symbol), throw TypeError.
        let left_is_bigint = matches!(left, Value::BigInt(_));
        let right_is_bigint = matches!(right, Value::BigInt(_));
        if left_is_bigint != right_is_bigint {
            let (_, js_err) = crate::value::error::create_js_error_with_type(
                "Cannot mix BigInt and other types",
                "TypeError",
            );
            return Err(js_err);
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
    let left = to_primitive(left, None)?;
    let right = to_primitive(right, None)?;
    if let (Value::String(a), Value::String(b)) = (&left, &right) {
        let cmp = string_compare(a, b);
        return Ok(Value::Boolean(num_cmp(cmp as f64, 0.0)));
    }
    Ok(Value::Boolean(num_cmp(to_number(&left), to_number(&right))))
}

fn string_compare(a: &str, b: &str) -> i32 {
    let a_units: Vec<u16> = a.encode_utf16().collect();
    let b_units: Vec<u16> = b.encode_utf16().collect();
    if a_units < b_units {
        -1
    } else if a_units > b_units {
        1
    } else {
        0
    }
}

fn eval_in_op(left: &Value, right: &Value) -> Result<Value, JsError> {
    let prop_name = to_js_string(left);
    match right {
        Value::Object(obj) => Ok(Value::Boolean(obj.borrow().has(&prop_name))),
        Value::Function(function) => Ok(Value::Boolean(
            function
                .own_property_names()
                .iter()
                .any(|key| key == &prop_name),
        )),
        _ => crate::throw!("TypeError", "right-hand side of 'in' is not an object"),
    }
}

fn has_prototype_in_chain(
    obj: &crate::value::Object,
    target_proto: &std::rc::Rc<std::cell::RefCell<crate::value::Object>>,
) -> bool {
    if let Some(ref proto_rc) = obj.prototype {
        let proto_ptr: *const std::cell::RefCell<crate::value::Object> = &**proto_rc;
        let target_ptr: *const std::cell::RefCell<crate::value::Object> = &**target_proto;
        if proto_ptr == target_ptr {
            return true;
        }
        let proto_borrowed = proto_rc.borrow();
        if has_prototype_in_chain(&proto_borrowed, target_proto) {
            return true;
        }
    }
    false
}

fn function_instanceof(
    func: &crate::value::ValueFunction,
    target_proto: &std::rc::Rc<std::cell::RefCell<crate::value::Object>>,
) -> bool {
    if let Some(ip) = func.instance_proto() {
        if std::rc::Rc::ptr_eq(&ip, target_proto) {
            return true;
        }
        return has_prototype_in_chain(&ip.borrow(), target_proto);
    }
    // Per ES spec §12.9.4 (OrdinaryHasInstance): walk the instance's own
    // [[Prototype]] chain looking for target_proto.
    // For function objects, [[Prototype]] depends on the function kind:
    //   normal/arrow → %FunctionPrototype%
    //   generator    → %GeneratorFunctionPrototype%
    //   async        → %AsyncFunctionPrototype%
    //   async gen    → %AsyncGeneratorFunctionPrototype%
    let func_proto = if func.is_generator && !func.is_async {
        crate::builtins::function::get_generator_function_prototype()
    } else if func.is_async && !func.is_generator {
        crate::builtins::function::get_async_function_prototype()
    } else if func.is_async {
        crate::builtins::function::get_async_generator_function_prototype()
    } else {
        crate::builtins::function::get_function_prototype()
    };
    if let Some(ref fp) = func_proto {
        let fp_ptr: *const std::cell::RefCell<crate::value::Object> = &**fp;
        let tp_ptr: *const std::cell::RefCell<crate::value::Object> = &**target_proto;
        if fp_ptr == tp_ptr {
            return true;
        }
        return has_prototype_in_chain(&fp.borrow(), target_proto);
    }
    false
}

fn eval_instanceof(left: &Value, right: &Value) -> Result<Value, JsError> {
    match right {
        Value::Undefined | Value::Null => {
            return crate::throw!("TypeError", "Right-hand side of instanceof is not callable")
        }
        Value::Object(ctor) => {
            if ctor.borrow().get("prototype").is_none() {
                return crate::throw!("TypeError", "Right-hand side of instanceof is not callable");
            }
        }
        Value::String(_) | Value::Number(_) | Value::Boolean(_) | Value::BigInt(_) => {
            return crate::throw!("TypeError", "Right-hand side of instanceof is not callable")
        }
        _ => {}
    }
    match (left, right) {
        (Value::Object(obj), Value::Function(ctor)) => {
            let ctor_proto = match ctor
                .get_property("prototype")
                .or_else(|| Some(Value::Object(ctor.get_prototype())))
            {
                Some(Value::Object(proto)) => proto,
                _ => return crate::throw!("TypeError", "Function has non-object prototype"),
            };
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
                crate::throw!("TypeError", "Function has non-object prototype")
            }
        }
        (Value::Function(func), Value::NativeConstructor(ctor)) => {
            let result = function_instanceof(func, &ctor.prototype);
            Ok(Value::Boolean(result))
        }
        (Value::Function(func), Value::NativeFunction(nf)) => {
            if let Some(Value::Object(proto)) = nf.get_property("prototype") {
                let result = function_instanceof(func, &proto);
                Ok(Value::Boolean(result))
            } else {
                crate::throw!("TypeError", "Function has non-object prototype")
            }
        }
        (Value::Function(func), Value::Function(ctor)) => {
            let ctor_proto = match ctor
                .get_property("prototype")
                .or_else(|| Some(Value::Object(ctor.get_prototype())))
            {
                Some(Value::Object(proto)) => proto,
                _ => return crate::throw!("TypeError", "Function has non-object prototype"),
            };
            let result = function_instanceof(func, &ctor_proto);
            Ok(Value::Boolean(result))
        }
        (Value::Object(obj), Value::Object(ctor)) => {
            let ctor_ref = ctor.borrow();
            if let Some(Value::Object(proto)) = ctor_ref.get("prototype") {
                drop(ctor_ref);
                let result = has_prototype_in_chain(&obj.borrow(), &proto);
                Ok(Value::Boolean(result))
            } else {
                crate::throw!("TypeError", "Right-hand side of instanceof is not callable")
            }
        }
        // Handle class instances: obj instanceof Class
        (Value::Object(obj), Value::Class(class)) => {
            let class_proto = get_class_prototype_for_instanceof(class)?;
            let result = has_prototype_in_chain(&obj.borrow(), &class_proto);
            Ok(Value::Boolean(result))
        }
        (Value::Function(func), Value::Class(class)) => {
            let class_proto = get_class_prototype_for_instanceof(class)?;
            let result = function_instanceof(func, &class_proto);
            Ok(Value::Boolean(result))
        }
        // Generator objects as left operand: check prototype chain.
        // The generator's `prototype` field stores the function's `.prototype`,
        // so we check if the function's prototype is self (direct match) or
        // an ancestor of the stored prototype.
        (Value::Generator(gen), Value::Function(ctor)) => {
            let ctor_proto = ctor.get_prototype();
            let result = gen.borrow().prototype.as_ref().is_some_and(|gen_proto| {
                Rc::ptr_eq(gen_proto, &ctor_proto)
                    || has_prototype_in_chain(&gen_proto.borrow(), &ctor_proto)
            });
            Ok(Value::Boolean(result))
        }
        (Value::Generator(gen), Value::NativeConstructor(ctor)) => {
            let result = gen.borrow().prototype.as_ref().is_some_and(|gen_proto| {
                Rc::ptr_eq(gen_proto, &ctor.prototype)
                    || has_prototype_in_chain(&gen_proto.borrow(), &ctor.prototype)
            });
            Ok(Value::Boolean(result))
        }
        (Value::Generator(gen), Value::NativeFunction(nf)) => {
            if let Some(Value::Object(proto)) = nf.get_property("prototype") {
                let result = gen.borrow().prototype.as_ref().is_some_and(|gen_proto| {
                    Rc::ptr_eq(gen_proto, &proto)
                        || has_prototype_in_chain(&gen_proto.borrow(), &proto)
                });
                Ok(Value::Boolean(result))
            } else {
                Ok(Value::Boolean(false))
            }
        }
        (Value::Generator(gen), Value::Class(class)) => {
            let class_proto = get_class_prototype_for_instanceof(class)?;
            let result =
                gen.borrow().prototype.as_ref().is_some_and(|gen_proto| {
                    has_prototype_in_chain(&gen_proto.borrow(), &class_proto)
                });
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
    let left = to_uint32(l) as i32 as i64;
    Ok(Value::Number((f(left, count) as i32) as f64))
}

fn shift_op_u<F>(left: &Value, right: &Value, f: F) -> Result<Value, JsError>
where
    F: FnOnce(u64, u64) -> u64,
{
    let left_primitive = to_primitive(left, Some("number"))?;
    let right_primitive = to_primitive(right, Some("number"))?;
    if matches!(left_primitive, Value::BigInt(_)) || matches!(right_primitive, Value::BigInt(_)) {
        return crate::throw!("TypeError", "Cannot mix BigInt with unsigned right shift");
    }
    // Use to_uint32 per JavaScript spec for unsigned right shift
    let l = to_uint32(to_number(&left_primitive)) as u64;
    let r = to_uint32(to_number(&right_primitive)) as u64;
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
        UnaryOp::Neg => match to_primitive(val, Some("number"))? {
            Value::BigInt(value) => Ok(Value::BigInt(Rc::new(-value.as_ref()))),
            primitive => Ok(Value::Number(-to_number(&primitive))),
        },
        UnaryOp::Plus => match to_primitive(val, Some("number"))? {
            Value::BigInt(_) => crate::throw!("TypeError", "Cannot convert BigInt to number"),
            primitive => Ok(Value::Number(to_number(&primitive))),
        },
        UnaryOp::BitNot => match to_primitive(val, Some("number"))? {
            Value::BigInt(value) => Ok(Value::BigInt(Rc::new(!value.as_ref()))),
            primitive => Ok(Value::Number(
                (!(to_uint32(to_number(&primitive)) as i32)) as f64,
            )),
        },
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
        Value::Object(object) => {
            if object.borrow().kind == crate::value::ObjectKind::Function {
                "function"
            } else {
                "object"
            }
        }
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
        Value::NativeConstructor(nc) => Some(std::rc::Rc::clone(&nc.prototype)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
