//! Number built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

// ============================================================================
// Number
// ============================================================================

pub fn register_number(ctx: &mut Context) {
    let number_proto = Object::new(ObjectKind::Ordinary);
    let number_proto_rc = Rc::new(RefCell::new(number_proto));

    setup_number_prototype(&number_proto_rc);
    setup_number_static(&number_proto_rc, ctx);
}

fn setup_number_prototype(proto: &Rc<RefCell<Object>>) {
    proto.borrow_mut().set(
        "toFixed",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            proto_to_fixed_impl(args)
        }))),
    );

    proto.borrow_mut().set(
        "valueOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            Ok(args.first().cloned().unwrap_or(Value::Number(0.0)))
        }))),
    );
}

fn proto_to_fixed_impl(args: Vec<Value>) -> Result<Value, crate::JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Number(0.0));
    let n = to_number(&this_val);
    let digits = args.first().map(|v| to_number(v) as i32).unwrap_or(0);
    let digits = digits.max(0).min(100) as usize;

    if n.is_nan() {
        return Ok(Value::String("NaN".to_string()));
    }
    if !n.is_finite() {
        return Ok(Value::String(
            if n.is_sign_positive() { "Infinity" } else { "-Infinity" }.to_string(),
        ));
    }

    let n = if n == 0.0 && n.is_sign_negative() { -0.0f64 } else { n };
    Ok(Value::String(format!("{:.prec$}", n, prec = digits)))
}

fn setup_number_static(proto: &Rc<RefCell<Object>>, ctx: &mut Context) {
    let number_proto_clone = Rc::clone(proto);
    let _number_constructor = NativeConstructor::new(
        move |args| {
            let n = args.first().map(to_number).unwrap_or(0.0);
            let number_obj =
                Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&number_proto_clone));
            let number_obj = Rc::new(RefCell::new(number_obj));
            number_obj.borrow_mut().set("_value", Value::Number(n));
            Ok(Value::Number(n))
        },
        proto.clone(),
    );

    let number_obj = Object::new(ObjectKind::Ordinary);
    let number_obj = Rc::new(RefCell::new(number_obj));
    number_obj.borrow_mut().set("prototype", Value::Object(Rc::clone(proto)));
    number_obj.borrow_mut().set("MAX_VALUE", Value::Number(f64::MAX));
    number_obj.borrow_mut().set("MIN_VALUE", Value::Number(f64::MIN_POSITIVE));
    number_obj.borrow_mut().set("NaN", Value::Number(f64::NAN));
    number_obj.borrow_mut().set("NEGATIVE_INFINITY", Value::Number(f64::NEG_INFINITY));
    number_obj.borrow_mut().set("POSITIVE_INFINITY", Value::Number(f64::INFINITY));

    ctx.set_global("Number".to_string(), Value::Object(number_obj));
}
