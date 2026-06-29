//! Host function registration API
//!
//! This module provides the API for registering native (host) functions
//! from the embedding application (the main quench crate).
//!
//! The quench-runtime crate itself does NOT depend on the quench bridge.
//! Instead, the main crate calls Context::register_native() to register
//! bridge functions.

use std::rc::Rc;
use std::cell::RefCell;

use crate::value::{Value, NativeFunction, to_js_string};
use crate::Context;

/// Context extension trait for host function registration
/// 
/// Implement this trait to provide host functions from the embedding application.
pub trait HostFunctions {
    /// Register all host functions into the given context
    fn register_functions(ctx: &mut Context);
}

/// Register a native function into the context
pub fn register_native<F>(ctx: &mut Context, name: &str, f: F)
where
    F: Fn(Vec<Value>) -> Result<Value, crate::value::JsError> + 'static,
{
    ctx.set_global(name.to_string(), Value::NativeFunction(Rc::new(NativeFunction::new(f))));
}

/// Internal - used by Context::init_builtins
pub(crate) fn register_builtin_functions(ctx: &mut Context) {
    // console
    register_console(ctx);
}

/// Setup the console global
fn register_console(ctx: &mut Context) {
    use crate::{Object, ObjectKind};
    
    let console = Object::new(ObjectKind::Ordinary);
    let console = Rc::new(RefCell::new(console));
    
    // console.log
    let console_clone = Rc::clone(&console);
    console.borrow_mut().set("log", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let msg = args.iter()
            .map(|v| to_js_string(v))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!("[console.log] {}", msg);
        Ok(Value::Undefined)
    }))));
    
    // console.error
    let console_err = Rc::clone(&console);
    console.borrow_mut().set("error", Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
        let msg = args.iter()
            .map(|v| to_js_string(v))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!("[console.error] {}", msg);
        Ok(Value::Undefined)
    }))));
    
    // console.warn
    console.borrow_mut().set("warn", console_clone.borrow().get("log").unwrap_or(Value::Undefined));
    
    ctx.set_global("console".to_string(), Value::Object(console));
}


