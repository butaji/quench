//! Date built-in and global utility functions.

pub mod helpers;

use std::cell::RefCell;
use std::rc::Rc;

pub use helpers::{spec_parse_float, spec_parse_int};

use crate::value::{
    to_bool, to_js_string, to_number, try_to_number, NativeConstructor, NativeFunction, Object,
    ObjectKind, PropertyFlags, Value,
};
use crate::Context;

// ============================================================================
// Global utility functions
// ============================================================================

pub fn register_global_functions(ctx: &mut Context) {
    register_timer_functions(ctx);
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

fn register_type_converters(ctx: &mut Context) {
    register_string_converter(ctx);
    register_boolean_converter(ctx);
}

fn register_string_converter(ctx: &mut Context) {
    let string_proto = create_string_prototype();
    crate::builtins::string::set_string_prototype(Rc::clone(&string_proto));
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
    for flags in string_proto_rc.borrow_mut().descriptors.values_mut() {
        flags.enumerable = false;
    }
    string_proto_rc
        .borrow_mut()
        .set("length", Value::Number(0.0));
    crate::builtins::string::register_string_iterator(&string_proto_rc);
    string_proto_rc
}

fn create_string_constructor_fn(string_proto_clone: Rc<RefCell<Object>>) -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new_with_prototype(
        move |args| {
            let s = match args.first() {
                Some(value) => crate::value::to_primitive(value, Some("string"))
                    .map(|primitive| to_js_string(&primitive))?,
                None => String::new(),
            };
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                for (index, character) in s.chars().enumerate() {
                    this_obj
                        .borrow_mut()
                        .set(&index.to_string(), Value::String(character.to_string()));
                }
                this_obj.borrow_mut().define(
                    "length",
                    Value::Number(s.len() as f64),
                    PropertyFlags {
                        writable: false,
                        enumerable: false,
                        configurable: false,
                        ..Default::default()
                    },
                );
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
    {
        let mut object = string_obj_rc.borrow_mut();
        object.callable = true;
        object.prototype = crate::builtins::get_function_prototype();
    }
    string_obj_rc
        .borrow_mut()
        .set("prototype", Value::Object(string_proto));
    let from_char_code = create_from_char_code_fn();
    let from_code_point = create_from_code_point_fn();
    // The self-hosted JS String builtin layer wraps `String.fromCharCode`
    // and `String.fromCodePoint` with JS functions that call the underscore
    // variants via `.apply`. Both variants must exist on the global String
    // object so the JS wrappers do not throw
    // "Cannot read property 'apply' of undefined".
    string_obj_rc
        .borrow_mut()
        .set("__fromCharCode", from_char_code.clone());
    string_obj_rc
        .borrow_mut()
        .set("__fromCodePoint", from_code_point.clone());
    string_obj_rc
        .borrow_mut()
        .set("fromCharCode", from_char_code);
    string_obj_rc
        .borrow_mut()
        .set("fromCodePoint", from_code_point);
    string_obj_rc.borrow_mut().set("constructor", string_fn);
    string_obj_rc
}

fn create_from_char_code_fn() -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let mut chars = String::new();
        for v in args {
            let code = crate::value::to_number(&v) as u16;
            if (0xd800..=0xdfff).contains(&code) {
                chars.push('\u{FFFD}');
                chars.push_str(&format!("{code:04x}"));
            } else {
                chars.push(std::char::from_u32(code as u32).unwrap_or('\u{FFFD}'));
            }
        }
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
    crate::builtins::object::set_boxed_value(
        &mut boolean_proto_rc.borrow_mut(),
        Value::Boolean(false),
    );

    let boolean_proto_clone = Rc::clone(&boolean_proto_rc);
    let mut boolean_native = NativeFunction::new_with_prototype(
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
    );
    boolean_native.name = "Boolean".into();
    boolean_native.set_constructable(true);
    let boolean_fn = Value::NativeFunction(Rc::new(boolean_native));

    // Set Boolean.prototype.constructor after boolean_fn exists
    boolean_proto_rc
        .borrow_mut()
        .set("constructor", boolean_fn.clone());

    // Set Boolean.prototype as the "prototype" property of boolean_fn
    // so constructor_prototype("Boolean") can find it.
    if let Value::NativeFunction(ref bf) = boolean_fn {
        bf.define_property(
            "prototype",
            Value::Object(Rc::clone(&boolean_proto_rc)),
            PropertyFlags {
                value: Some(Value::Object(Rc::clone(&boolean_proto_rc))),
                writable: false,
                enumerable: false,
                configurable: false,
            },
        );
        bf.define_property(
            "length",
            Value::Number(1.0),
            PropertyFlags {
                value: Some(Value::Number(1.0)),
                writable: false,
                enumerable: false,
                configurable: true,
            },
        );
    }

    // Register Boolean as a NativeFunction (callable), not a plain Object.
    ctx.set_global("Boolean".to_string(), boolean_fn);
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

fn date_timestamp(this_val: &Value) -> f64 {
    if let Value::Object(obj_rc) = this_val {
        if let Some(Value::Number(n)) = obj_rc.borrow().get("_timestamp") {
            return n;
        }
    }
    chrono_now() as f64
}

fn date_parts_from_timestamp(ms: f64) -> (i32, i32, i32) {
    use chrono::{Datelike, TimeZone, Utc};
    let secs = (ms / 1000.0).trunc() as i64;
    let nsecs = ((ms.fract() * 1_000_000_000.0).max(0.0) as u32).min(999_999_999);
    let dt = Utc
        .timestamp_opt(secs, nsecs)
        .single()
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).single().unwrap());
    (dt.year(), dt.month() as i32 - 1, dt.day() as i32)
}

fn install_date_getter(proto: &Rc<RefCell<Object>>, name: &str, component: fn(f64) -> i32) {
    proto.borrow_mut().set(
        name,
        Value::NativeFunction(Rc::new(NativeFunction::new(move |_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            let ms = date_timestamp(&this_val);
            Ok(Value::Number(component(ms) as f64))
        }))),
    );
}

fn install_date_placeholder(proto: &Rc<RefCell<Object>>, name: &str) {
    proto.borrow_mut().set(
        name,
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| Ok(Value::Undefined)))),
    );
}

pub fn register_date(ctx: &mut Context) {
    let date_proto = Object::new(ObjectKind::Date);
    let date_proto_rc = Rc::new(RefCell::new(date_proto));

    date_proto_rc.borrow_mut().set(
        "__toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            Ok(Value::String(format!(
                "Date @ {}",
                date_timestamp(&this_val)
            )))
        }))),
    );
    date_proto_rc.borrow_mut().set(
        "__valueOf",
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
            Ok(Value::Number(date_timestamp(&this_val)))
        }))),
    );
    install_date_getter(&date_proto_rc, "getFullYear", |ms| {
        date_parts_from_timestamp(ms).0
    });
    install_date_getter(&date_proto_rc, "getMonth", |ms| {
        date_parts_from_timestamp(ms).1
    });
    install_date_getter(&date_proto_rc, "getDate", |ms| {
        date_parts_from_timestamp(ms).2
    });
    install_date_getter(&date_proto_rc, "getUTCFullYear", |ms| {
        date_parts_from_timestamp(ms).0
    });
    install_date_getter(&date_proto_rc, "getUTCMonth", |ms| {
        date_parts_from_timestamp(ms).1
    });
    install_date_getter(&date_proto_rc, "getUTCDate", |ms| {
        date_parts_from_timestamp(ms).2
    });
    for name in [
        "getDay",
        "getHours",
        "getMilliseconds",
        "getMinutes",
        "getSeconds",
        "getUTCDay",
        "getUTCHours",
        "getUTCMilliseconds",
        "getUTCMinutes",
        "getUTCSeconds",
        "setDate",
        "setFullYear",
        "setHours",
        "setMilliseconds",
        "setMinutes",
        "setMonth",
        "setSeconds",
        "setTime",
        "setUTCDate",
        "setUTCFullYear",
        "setUTCHours",
        "setUTCMilliseconds",
        "setUTCMinutes",
        "setUTCMonth",
        "setUTCSeconds",
        "toLocaleString",
        "toUTCString",
    ] {
        install_date_placeholder(&date_proto_rc, name);
    }
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        date_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    let date_proto_clone = Rc::clone(&date_proto_rc);
    let date_constructor = NativeConstructor::new(
        move |args| {
            if matches!(
                crate::builtins::get_native_this(),
                Some(Value::Undefined) | None
            ) {
                return Ok(Value::String(format!("Date @ {}", chrono_now())));
            }
            let timestamp = if args.is_empty() {
                chrono_now() as f64
            } else if args.len() == 1 {
                try_to_number(&args[0])?
            } else {
                let year = try_to_number(&args[0])? as i32;
                let month = try_to_number(&args[1])? as i32;
                let day = args
                    .get(2)
                    .map(try_to_number)
                    .transpose()?
                    .map(|v| v as i32)
                    .unwrap_or(1);
                let hour = args
                    .get(3)
                    .map(try_to_number)
                    .transpose()?
                    .map(|v| v as i32)
                    .unwrap_or(0);
                let min = args
                    .get(4)
                    .map(try_to_number)
                    .transpose()?
                    .map(|v| v as i32)
                    .unwrap_or(0);
                let sec = args
                    .get(5)
                    .map(try_to_number)
                    .transpose()?
                    .map(|v| v as i32)
                    .unwrap_or(0);
                let ms = args
                    .get(6)
                    .map(try_to_number)
                    .transpose()?
                    .map(|v| v as i32)
                    .unwrap_or(0);

                let total_secs = chrono_to_timestamp(year, month + 1, day, hour, min, sec);
                (total_secs * 1000) as f64 + ms as f64
            };

            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(obj_rc) = this_val {
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
    date_proto_rc.borrow_mut().set(
        "constructor",
        Value::NativeConstructor(Rc::new(date_constructor.clone())),
    );

    let date_wrapper = Object::new(ObjectKind::Ordinary);
    let date_wrapper_rc = Rc::new(RefCell::new(date_wrapper));
    date_wrapper_rc.borrow_mut().callable = true;
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
    date_wrapper_rc.borrow_mut().set(
        "parse",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let text = args.first().map(to_js_string).unwrap_or_default();
            let timestamp = chrono::DateTime::parse_from_rfc3339(&text)
                .map(|value| value.timestamp_millis() as f64)
                .unwrap_or(f64::NAN);
            Ok(Value::Number(timestamp))
        }))),
    );
    date_wrapper_rc.borrow_mut().set(
        "UTC",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let year = args.first().map(to_number).unwrap_or(f64::NAN) as i32;
            let month = args.get(1).map(to_number).unwrap_or(0.0) as i32;
            let day = args.get(2).map(to_number).unwrap_or(1.0) as i32;
            let hour = args.get(3).map(to_number).unwrap_or(0.0) as i32;
            let minute = args.get(4).map(to_number).unwrap_or(0.0) as i32;
            let second = args.get(5).map(to_number).unwrap_or(0.0) as i32;
            let millisecond = args.get(6).map(to_number).unwrap_or(0.0);
            Ok(Value::Number(
                (chrono_to_timestamp(year, month + 1, day, hour, minute, second) * 1000) as f64
                    + millisecond,
            ))
        }))),
    );
    for flags in date_proto_rc.borrow_mut().descriptors.values_mut() {
        flags.enumerable = false;
    }
    for flags in date_wrapper_rc.borrow_mut().descriptors.values_mut() {
        flags.enumerable = false;
    }
    ctx.set_global("Date".to_string(), Value::Object(date_wrapper_rc));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_called_without_new_returns_string() {
        let mut context = Context::new().unwrap();
        assert_eq!(
            context.eval("typeof Date() ").unwrap(),
            Value::String("string".to_string())
        );
    }

    #[test]
    fn boolean_constructor_has_length_and_is_constructable() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval("[Boolean.length, __ops__.IsConstructor(Boolean), (() => { try { Reflect.construct(function(){}, [], Boolean); return true; } catch (e) { return false; } })()].join('|')")
            .unwrap();
        assert_eq!(result, crate::Value::String("1|true|true".to_string()));
    }

    #[test]
    fn boolean_prototype_methods_and_descriptors_follow_boolean_spec() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                "[Boolean.prototype.valueOf(), Boolean.prototype.toString(), \
                 Object.getOwnPropertyDescriptor(Boolean.prototype.valueOf, 'name').value, \
                 Object.getOwnPropertyDescriptor(Boolean.prototype.valueOf, 'length').writable, \
                 Object.getOwnPropertyDescriptor(Boolean, 'prototype').writable, \
                 Object.getOwnPropertyDescriptor(Boolean, 'prototype').configurable].join('|')",
            )
            .unwrap();
        assert_eq!(
            result,
            crate::Value::String("false|false|valueOf|false|false|false".to_string())
        );
    }

    #[test]
    fn boolean_prototype_cannot_be_deleted() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                "(function() { 'use strict'; try { delete Boolean.prototype; return false; } catch (e) { return e instanceof TypeError; } })()",
            )
            .unwrap();
        assert_eq!(result, crate::Value::Boolean(true));
    }

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
    fn date_static_parse_and_utc_are_callable() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("typeof Date.parse + '|' + typeof Date.UTC"),
            Ok(Value::String("function|function".to_string()))
        );
    }

    #[test]
    fn string_prototype_constructor_is_callable() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("typeof String.prototype.constructor"),
            Ok(Value::String("function".to_string()))
        );
    }

    #[test]
    fn string_prototype_length_is_zero() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(ctx.eval("String.prototype.length"), Ok(Value::Number(0.0)));
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

    #[test]
    fn date_addition_uses_string_hint() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var date = new Date(0); date + true === date.toString() + 'true'"),
            Ok(Value::Boolean(true))
        );
    }

    #[test]
    fn date_to_string_uses_date_timestamp() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("new Date(0).toString()"),
            Ok(Value::String("Date @ 0".to_string()))
        );
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
    fn test_date_get_full_year_month_date() {
        use crate::Context;
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("new Date(1859, 10, 24).getFullYear()").unwrap(),
            Value::Number(1859.0)
        );
        assert_eq!(
            ctx.eval("new Date(1859, 10, 24).getMonth()").unwrap(),
            Value::Number(10.0)
        );
        assert_eq!(
            ctx.eval("new Date(1859, 10, 24).getDate()").unwrap(),
            Value::Number(24.0)
        );
    }

    #[test]
    fn test_date_subclass_regular_subclassing() {
        use crate::Context;
        let mut ctx = Context::new().unwrap();
        ctx.eval("class D extends Date {}").unwrap();
        assert_eq!(
            ctx.eval("new D(1859, 10, 24).getFullYear()").unwrap(),
            Value::Number(1859.0)
        );
        assert_eq!(
            ctx.eval("new D(1859, 10, 24).getMonth()").unwrap(),
            Value::Number(10.0)
        );
        assert_eq!(
            ctx.eval("new D(1859, 10, 24).getDate()").unwrap(),
            Value::Number(24.0)
        );
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

    #[test]
    fn date_prototype_methods_are_non_enumerable() {
        let mut ctx = Context::new().unwrap();
        let value = ctx
            .eval("[Object.getOwnPropertyDescriptor(Date.prototype, 'toString').enumerable, Object.getOwnPropertyDescriptor(Date.prototype, 'toUTCString').enumerable].join('|')")
            .unwrap();
        assert_eq!(value, Value::String("false|false".into()));
    }
}
