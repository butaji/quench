//! Console built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, NativeFunction, Object, Value};
use crate::Context;

// ============================================================================
// Console
// ============================================================================

pub fn register_console(ctx: &mut Context) {
    let console = Object::new(crate::value::ObjectKind::Ordinary);
    let console = Rc::new(RefCell::new(console));

    console.borrow_mut().set(
        "log",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let msg = args.iter().map(to_js_string).collect::<Vec<_>>().join(" ");
            println!("{}", msg);
            Ok(Value::Undefined)
        }))),
    );

    console.borrow_mut().set(
        "error",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let msg = args.iter().map(to_js_string).collect::<Vec<_>>().join(" ");
            eprintln!("{}", msg);
            Ok(Value::Undefined)
        }))),
    );

    console.borrow_mut().set(
        "warn",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            let msg = args.iter().map(to_js_string).collect::<Vec<_>>().join(" ");
            println!("{}", msg);
            Ok(Value::Undefined)
        }))),
    );

    ctx.set_global("console".to_string(), Value::Object(console));
}
