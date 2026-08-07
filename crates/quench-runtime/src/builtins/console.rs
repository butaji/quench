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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Context;

    fn eval_ok(src: &str) -> Value {
        let mut ctx = Context::new().unwrap();
        ctx.eval(src).unwrap()
    }

    #[test]
    fn console_exists_as_global() {
        let result = eval_ok("typeof console");
        assert_eq!(result.to_string(), "object");
    }

    #[test]
    fn console_log_exists() {
        let result = eval_ok("typeof console.log");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn console_error_exists() {
        let result = eval_ok("typeof console.error");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn console_warn_exists() {
        let result = eval_ok("typeof console.warn");
        assert_eq!(result.to_string(), "function");
    }

    #[test]
    fn console_log_returns_undefined() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("console.log('test')");
        assert!(result.is_ok());
    }
}
