//! Date built-in and global utility functions.

pub mod helpers;

use std::cell::RefCell;
use std::rc::Rc;

pub use helpers::{spec_parse_float, spec_parse_int};

use crate::value::{
    to_bool, to_js_string, to_number, try_to_number, NativeConstructor, NativeFunction, Object,
    ObjectKind, Value,
};
use crate::Context;

// ============================================================================
// Global utility functions
// ============================================================================

pub fn register_global_functions(ctx: &mut Context) {
    register_timer_functions(ctx);
    register_parse_functions(ctx);
    register_uri_functions(ctx);
    register_type_converters(ctx);
}

fn register_timer_functions(ctx: &mut Context) {
    ctx.register_native("setTimeout", |args| {
        let _callback = args.first().map(to_js_string).unwrap_or_default();
        let _delay = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        Ok(Value::Number(1.0))
    });
    ctx.register_native("setInterval", |args| {
        let _callback = args.first().map(to_js_string).unwrap_or_default();
        let _interval = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        Ok(Value::Number(1.0))
    });
    ctx.register_native("clearTimeout", |_args| Ok(Value::Undefined));
    ctx.register_native("clearInterval", |_args| Ok(Value::Undefined));
}

fn register_parse_functions(ctx: &mut Context) {
    ctx.register_native("parseInt", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        let radix = args.get(1).map(|v| to_number(v) as i32).unwrap_or(0);
        let radix = if radix == 0 { 10 } else { radix };
        if !(2..=36).contains(&radix) {
            return Ok(Value::Number(f64::NAN));
        }
        let n = spec_parse_int(&s, radix as u32);
        Ok(Value::Number(n))
    });
    ctx.register_native("parseFloat", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        let n = spec_parse_float(&s);
        Ok(Value::Number(n))
    });
    ctx.register_native("isNaN", |args| {
        let n = args.first().map(to_number).unwrap_or(f64::NAN);
        Ok(Value::Boolean(n.is_nan()))
    });
    ctx.register_native("isFinite", |args| {
        let n = args.first().map(try_to_number).unwrap_or(Ok(f64::NAN))?;
        Ok(Value::Boolean(n.is_finite()))
    });
}

fn register_uri_functions(ctx: &mut Context) {
    ctx.register_native("encodeURIComponent", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        Ok(Value::String(urlencoding::encode(&s).to_string()))
    });
    ctx.register_native("decodeURIComponent", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        let decoded = urlencoding::decode(&s).map(|d| d.to_string()).unwrap_or(s);
        Ok(Value::String(decoded))
    });
}

fn register_type_converters(ctx: &mut Context) {
    register_string_converter(ctx);
    register_boolean_converter(ctx);
}

fn register_string_converter(ctx: &mut Context) {
    let string_proto = create_string_prototype();
    let string_proto_clone = Rc::clone(&string_proto);
    let string_fn = create_string_constructor_fn(string_proto_clone);

    let string_obj = create_string_constructor_object(string_proto.clone(), string_fn.clone());
    string_proto
        .borrow_mut()
        .set("constructor", Value::Object(Rc::clone(&string_obj)));
    ctx.set_global("String".to_string(), Value::Object(string_obj));
}

fn create_string_prototype() -> Rc<RefCell<Object>> {
    let string_proto = Object::new(ObjectKind::Ordinary);
    let string_proto_rc = Rc::new(RefCell::new(string_proto));
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        string_proto_rc.borrow_mut().prototype = Some(object_proto);
    }
    crate::builtins::string::methods::install_string_methods(&string_proto_rc);
    string_proto_rc
}

fn create_string_constructor_fn(string_proto_clone: Rc<RefCell<Object>>) -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new_with_prototype(
        move |args| {
            let s = args.first().map(to_js_string).unwrap_or_default();
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                this_obj.borrow_mut().set("0", Value::String(s.clone()));
                this_obj
                    .borrow_mut()
                    .set("length", Value::Number(s.len() as f64));
                crate::builtins::object::set_boxed_value(
                    &mut this_obj.borrow_mut(),
                    Value::String(s.clone()),
                );
                this_obj.borrow_mut().exotic_kind = Some(crate::value::kind::ExoticKind::String);
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&string_proto_clone));
                }
                Ok(Value::Object(this_obj))
            } else {
                Ok(Value::String(s))
            }
        },
        Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))),
    )))
}

fn create_string_constructor_object(
    string_proto: Rc<RefCell<Object>>,
    string_fn: Value,
) -> Rc<RefCell<Object>> {
    let string_obj = Object::new(ObjectKind::Ordinary);
    let string_obj_rc = Rc::new(RefCell::new(string_obj));
    string_obj_rc
        .borrow_mut()
        .set("prototype", Value::Object(string_proto));
    string_obj_rc
        .borrow_mut()
        .set("fromCharCode", create_from_char_code_fn());
    string_obj_rc
        .borrow_mut()
        .set("fromCodePoint", create_from_code_point_fn());
    string_obj_rc.borrow_mut().set("constructor", string_fn);
    string_obj_rc
}

fn create_from_char_code_fn() -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let chars: String = args
            .iter()
            .map(|v| {
                let code = crate::value::to_number(v) as u16;
                std::char::from_u32(code as u32).unwrap_or('\u{FFFD}')
            })
            .collect();
        Ok(Value::String(chars))
    })))
}

fn create_from_code_point_fn() -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let chars: String = args
            .iter()
            .map(|v| {
                let code = crate::value::to_number(v) as u32;
                std::char::from_u32(code).unwrap_or('\u{FFFD}')
            })
            .collect();
        Ok(Value::String(chars))
    })))
}

fn register_boolean_converter(ctx: &mut Context) {
    let boolean_proto = Object::new(ObjectKind::Ordinary);
    let boolean_proto_rc = Rc::new(RefCell::new(boolean_proto));
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        boolean_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    boolean_proto_rc.borrow_mut().set(
        "valueOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(obj) = &this_val {
                if let Some(Value::Boolean(b)) = obj.borrow().get("_value") {
                    return Ok(Value::Boolean(b));
                }
            }
            match this_val {
                Value::Boolean(b) => Ok(Value::Boolean(b)),
                _ => Err(crate::JsError::new(
                    "TypeError: Boolean.prototype.valueOf requires a Boolean receiver",
                )),
            }
        }))),
    );

    let boolean_proto_clone = Rc::clone(&boolean_proto_rc);
    let boolean_fn = Value::NativeFunction(Rc::new(NativeFunction::new_with_prototype(
        move |args| {
            let b = args.first().map(to_bool).unwrap_or(false);
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                crate::builtins::object::set_boxed_value(
                    &mut this_obj.borrow_mut(),
                    Value::Boolean(b),
                );
                this_obj.borrow_mut().exotic_kind = Some(crate::value::kind::ExoticKind::Boolean);
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&boolean_proto_clone));
                }
                Ok(Value::Object(this_obj))
            } else {
                Ok(Value::Boolean(b))
            }
        },
        boolean_proto_rc.clone(),
    )));

    let boolean_obj = Object::new(ObjectKind::Ordinary);
    let boolean_obj_rc = Rc::new(RefCell::new(boolean_obj));
    boolean_obj_rc
        .borrow_mut()
        .set("prototype", Value::Object(boolean_proto_rc));
    boolean_obj_rc.borrow_mut().set("constructor", boolean_fn);
    ctx.set_global("Boolean".to_string(), Value::Object(boolean_obj_rc));
}

// ============================================================================
// Date
// ============================================================================

fn chrono_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn chrono_to_timestamp(year: i32, month: i32, day: i32, hour: i32, min: i32, sec: i32) -> i64 {
    let days = days_from_ymd(year, month, day);
    (days * 86400) + (hour as i64 * 3600) + (min as i64 * 60) + sec as i64
}

fn days_from_ymd(year: i32, month: i32, day: i32) -> i64 {
    let total_months = year as i64 * 12 + (month as i64 - 1);
    let year = total_months.div_euclid(12) as i32;
    let month = total_months.rem_euclid(12) as i32 + 1;

    let mut days = 0i64;
    if year >= 1970 {
        for y in 1970..year {
            days += if is_leap_year(y) { 366 } else { 365 };
        }
    } else {
        for y in year..1970 {
            days -= if is_leap_year(y) { 366 } else { 365 };
        }
    }
    for m in 1..month {
        days += days_in_month(year, m);
    }
    days + (day - 1) as i64
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: i32) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

pub fn register_date(ctx: &mut Context) {
    let date_proto = Object::new(ObjectKind::Date);
    let date_proto_rc = Rc::new(RefCell::new(date_proto));

    date_proto_rc.borrow_mut().set(
        "toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            Ok(Value::String(format!("Date @ {}", chrono_now())))
        }))),
    );
    date_proto_rc.borrow_mut().set(
        "valueOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(obj_rc) = this_val {
                if let Some(ts) = obj_rc.borrow().get("_timestamp") {
                    return Ok(ts);
                }
            }
            Ok(Value::Number(chrono_now() as f64))
        }))),
    );
    date_proto_rc.borrow_mut().set(
        "getTimezoneOffset",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| Ok(Value::Number(0.0))))),
    );
    date_proto_rc.borrow_mut().set(
        "getTime",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(obj_rc) = this_val {
                if let Some(ts) = obj_rc.borrow().get("_timestamp") {
                    return Ok(ts);
                }
            }
            Ok(Value::Number(chrono_now() as f64))
        }))),
    );
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        date_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    let date_proto_clone = Rc::clone(&date_proto_rc);
    let date_constructor = NativeConstructor::new(
        move |args| {
            let timestamp = if args.is_empty() {
                chrono_now() as f64
            } else if args.len() == 1 {
                crate::value::to_number(&args[0])
            } else {
                let year = crate::value::to_number(&args[0]) as i32;
                let month = crate::value::to_number(&args[1]) as i32;
                let day = args
                    .get(2)
                    .map(|v| crate::value::to_number(v) as i32)
                    .unwrap_or(1);
                let hour = args
                    .get(3)
                    .map(|v| crate::value::to_number(v) as i32)
                    .unwrap_or(0);
                let min = args
                    .get(4)
                    .map(|v| crate::value::to_number(v) as i32)
                    .unwrap_or(0);
                let sec = args
                    .get(5)
                    .map(|v| crate::value::to_number(v) as i32)
                    .unwrap_or(0);
                let ms = args
                    .get(6)
                    .map(|v| crate::value::to_number(v) as i32)
                    .unwrap_or(0);

                let total_secs = chrono_to_timestamp(year, month + 1, day, hour, min, sec);
                (total_secs * 1000) as f64 + ms as f64
            };

            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(obj_rc) = this_val {
                obj_rc.borrow_mut().prototype = Some(Rc::clone(&date_proto_clone));
                obj_rc.borrow_mut().kind = ObjectKind::Date;
                obj_rc
                    .borrow_mut()
                    .set("_timestamp", Value::Number(timestamp));
                Ok(Value::Object(obj_rc))
            } else {
                let date_obj =
                    Object::with_prototype(ObjectKind::Date, Rc::clone(&date_proto_clone));
                let date = Rc::new(RefCell::new(date_obj));
                date.borrow_mut()
                    .set("_timestamp", Value::Number(timestamp));
                Ok(Value::Object(date))
            }
        },
        date_proto_rc.clone(),
    );

    let date_wrapper = Object::new(ObjectKind::Ordinary);
    let date_wrapper_rc = Rc::new(RefCell::new(date_wrapper));
    date_wrapper_rc.borrow_mut().set(
        "constructor",
        Value::NativeConstructor(Rc::new(date_constructor)),
    );
    date_wrapper_rc
        .borrow_mut()
        .set("prototype", Value::Object(Rc::clone(&date_proto_rc)));
    date_wrapper_rc.borrow_mut().set(
        "now",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            Ok(Value::Number(chrono_now() as f64))
        }))),
    );
    ctx.set_global("Date".to_string(), Value::Object(date_wrapper_rc));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_days_from_ymd_before_1970_is_negative() {
        assert_eq!(days_from_ymd(1969, 1, 1), -365);
        assert_eq!(days_from_ymd(1968, 1, 1), -(365 + 366));
        assert_eq!(days_from_ymd(1970, 1, 1), 0);
    }

    #[test]
    fn test_days_from_ymd_normalizes_month_overflow() {
        assert_eq!(days_from_ymd(2024, 14, 1), days_from_ymd(2025, 2, 1));
        assert_eq!(days_from_ymd(2024, 0, 1), days_from_ymd(2023, 12, 1));
    }

    #[test]
    fn test_date_before_1970_has_negative_timestamp() {
        use crate::Context;
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("new Date(1969, 0, 1).getTime()").unwrap();
        match result {
            Value::Number(n) => {
                assert!(n < 0.0, "Date(1969,0,1).getTime() must be < 0, got {}", n)
            }
            other => panic!("expected Number, got {:?}", other),
        }
    }

    #[test]
    fn test_date_month_overflow_normalizes() {
        use crate::Context;
        let mut ctx = Context::new().unwrap();
        let overflow = ctx.eval("new Date(2024, 13, 1).getTime()").unwrap();
        let expected = ctx.eval("new Date(2025, 1, 1).getTime()").unwrap();
        assert_eq!(overflow, expected);
    }

    fn eval_num(src: &str) -> f64 {
        use crate::Context;
        let mut ctx = Context::new().unwrap();
        match ctx.eval(src).unwrap() {
            Value::Number(n) => n,
            other => panic!("expected Number from {:?}, got {:?}", src, other),
        }
    }

    #[test]
    fn test_parse_float_accepts_infinity_literal() {
        assert!(eval_num("parseFloat(Infinity)").is_infinite());
        assert!(eval_num("parseFloat(Infinity)") > 0.0);
        assert!(eval_num("parseFloat(-Infinity)") < 0.0);
        assert!(eval_num("parseFloat('Infinity')").is_infinite());
        assert!(eval_num("parseFloat('-Infinity')").is_infinite());
        assert!(eval_num("parseFloat('-Infinity')") < 0.0);
        assert!(eval_num("parseFloat('infinity')").is_nan());
    }

    #[test]
    fn test_boolean_new_boxed() {
        use crate::Context;
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("new Boolean(true).valueOf()").unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_boolean_super_check() {
        use crate::Context;
        let mut ctx = Context::new().unwrap();
        // Check that extends Boolean works (no explicit constructor — default ctor)
        let r1 = ctx
            .eval(r#"class B extends Boolean {}; new B() instanceof Boolean"#)
            .unwrap();
        assert_eq!(r1, Value::Boolean(true));
        // Check that super(true) works with explicit constructor
        let r2 = ctx.eval(
            r#"class B extends Boolean { constructor() { super(true); } }; new B() instanceof Boolean"#,
        ).unwrap();
        assert_eq!(r2, Value::Boolean(true));
    }

    #[test]
    fn test_boolean_subclassing_default_ctor() {
        use crate::Context;
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                r#"
            class MyBoolean extends Boolean {}
            let b = new MyBoolean();
            b instanceof MyBoolean;
        "#,
            )
            .unwrap();
        assert!(
            matches!(result, Value::Boolean(true)),
            "expected true for no-ctor extends Boolean, got {:?}",
            result
        );
    }

    #[test]
    fn test_boolean_subclassing_via_extends() {
        use crate::Context;
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval(
                r#"
            class MyBoolean extends Boolean {
                constructor() {
                    super(true);
                }
                getValue() { return this.valueOf(); }
            }
            let b = new MyBoolean();
            [
                b instanceof MyBoolean,
                b instanceof Boolean,
                b.getValue(),
                Object.getPrototypeOf(b) === MyBoolean.prototype,
            ];
        "#,
            )
            .unwrap();
        match result {
            Value::Object(arr_rc) => {
                let arr = arr_rc.borrow();
                assert!(
                    matches!(arr.get("0"), Some(Value::Boolean(true))),
                    "expected true for instanceof MyBoolean, got {:?}",
                    arr.get("0")
                );
                assert!(
                    matches!(arr.get("1"), Some(Value::Boolean(true))),
                    "expected true for instanceof Boolean, got {:?}",
                    arr.get("1")
                );
                assert!(
                    matches!(arr.get("2"), Some(Value::Boolean(true))),
                    "expected true for getValue(), got {:?}",
                    arr.get("2")
                );
                assert!(
                    matches!(arr.get("3"), Some(Value::Boolean(true))),
                    "expected true for Object.getPrototypeOf(b) === MyBoolean.prototype, got {:?}",
                    arr.get("3")
                );
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_float_decimal_then_exponent() {
        assert_eq!(eval_num("parseFloat('.01e+2')"), 1.0);
        assert_eq!(eval_num("parseFloat('.5e1')"), 5.0);
        let expected = eval_num("3.14");
        assert!((eval_num("parseFloat('3.14')") - expected).abs() < 1e-10);
        assert_eq!(eval_num("parseFloat('.01')"), 0.01);
    }
}
