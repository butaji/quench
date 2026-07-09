//! Date built-in and global utility functions

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, to_bool, NativeConstructor, NativeFunction, Object, ObjectKind, Value};
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
        let n = s.trim().parse::<i64>().ok().map(|n| n as f64).unwrap_or(f64::NAN);
        Ok(Value::Number(n))
    });
    ctx.register_native("parseFloat", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        let n = s.trim().parse::<f64>().unwrap_or(f64::NAN);
        Ok(Value::Number(n))
    });
    ctx.register_native("isNaN", |args| {
        let n = args.first().map(to_number).unwrap_or(f64::NAN);
        Ok(Value::Boolean(n.is_nan()))
    });
    ctx.register_native("isFinite", |args| {
        let n = args.first().map(to_number).unwrap_or(f64::NAN);
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
    let string_fn = Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        Ok(Value::String(s))
    })));
    ctx.set_global("String".to_string(), string_fn);

    let boolean_fn = Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let b = args.first().map(to_bool).unwrap_or(false);
        Ok(Value::Boolean(b))
    })));
    ctx.set_global("Boolean".to_string(), boolean_fn);
}

// ============================================================================
// Date
// ============================================================================

fn chrono_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

pub fn register_date(ctx: &mut Context) {
    let date_proto = Object::new(ObjectKind::Date);
    let date_proto_rc = Rc::new(RefCell::new(date_proto));

    date_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::String(format!("Date @ {}", chrono_now())))
    }))));
    date_proto_rc.borrow_mut().set("valueOf", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::Number(chrono_now() as f64))
    }))));

    let date_proto_clone = Rc::clone(&date_proto_rc);
    let date_constructor = NativeConstructor::new(
        move |_args| {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as f64;
            let date_obj = Object::with_prototype(ObjectKind::Date, Rc::clone(&date_proto_clone));
            let date = Rc::new(RefCell::new(date_obj));
            date.borrow_mut().set("_timestamp", Value::Number(now));
            Ok(Value::Object(date))
        },
        date_proto_rc.clone(),
    );

    let date_wrapper = Object::new(ObjectKind::Ordinary);
    let date_wrapper_rc = Rc::new(RefCell::new(date_wrapper));
    date_wrapper_rc.borrow_mut().set("constructor", Value::NativeConstructor(Rc::new(date_constructor)));
    date_wrapper_rc.borrow_mut().set("prototype", Value::Object(Rc::clone(&date_proto_rc)));
    date_wrapper_rc.borrow_mut().set("now", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::Number(chrono_now() as f64))
    }))));
    ctx.set_global("Date".to_string(), Value::Object(date_wrapper_rc));
}
