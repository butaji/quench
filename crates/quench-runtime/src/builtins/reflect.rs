//! Minimal Reflect global — currently only `Reflect.ownKeys`, which the
//! test262 harness (deepEqual.js `format`) needs. Tests that require the
//! full Reflect API are still skipped via the `Reflect` feature gate.

use crate::context::Context;
use crate::value::{Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

pub fn register_reflect(ctx: &mut Context) {
    let mut reflect = Object::new(ObjectKind::Ordinary);
    reflect.set(
        "ownKeys",
        Value::NativeFunction(Rc::new(crate::value::NativeFunction::new(
            |args: Vec<Value>| match args.first() {
                Some(Value::Object(o)) => {
                    let keys: Vec<Value> = o
                        .borrow()
                        .own_keys()
                        .into_iter()
                        .map(Value::String)
                        .collect();
                    Ok(Value::Object(Rc::new(RefCell::new(
                        Object::new_array_from(keys),
                    ))))
                }
                _ => {
                    let (err_val, js_err) = crate::value::error::create_js_error_with_type(
                        "Reflect.ownKeys called on non-object",
                        "TypeError",
                    );
                    crate::value::set_thrown_value(err_val);
                    Err(js_err)
                }
            },
        ))),
    );
    ctx.set_global(
        "Reflect".to_string(),
        Value::Object(Rc::new(RefCell::new(reflect))),
    );
}
