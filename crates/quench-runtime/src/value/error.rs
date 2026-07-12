//! Error handling for JavaScript runtime errors.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use super::{Object, ObjectKind, Value};
use crate::context::CURRENT_CONTEXT;

/// JavaScript error - wraps error messages
#[derive(Clone)]
pub struct JsError(pub String);

impl JsError {
    /// Create a new JsError
    pub fn new(msg: impl Into<String>) -> Self {
        JsError(msg.into())
    }
}

impl fmt::Debug for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JsError({:?})", self.0)
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for JsError {}

impl From<&str> for JsError {
    fn from(s: &str) -> Self {
        JsError(s.to_string())
    }
}

impl From<String> for JsError {
    fn from(s: String) -> Self {
        JsError(s)
    }
}

// Thread-local storage for the original thrown value during exception propagation
thread_local! {
    static THROWN_VALUE: RefCell<Option<Value>> = const { RefCell::new(None) };
}

/// Set the thrown value for the current catch block to retrieve
pub fn set_thrown_value(value: Value) {
    THROWN_VALUE.with(|cell| {
        *cell.borrow_mut() = Some(value);
    });
}

/// Get and clear the thrown value (called by catch block)
pub fn take_thrown_value() -> Option<Value> {
    THROWN_VALUE.with(|cell| cell.borrow_mut().take())
}

/// Peek at the thrown value without consuming it (used by assert.throws)
pub fn get_thrown_value() -> Option<Value> {
    THROWN_VALUE.with(|cell| cell.borrow().clone())
}

// Thread-local Error prototype for creating errors outside of eval context
thread_local! {
    static ERROR_PROTOTYPE: Cell<Option<Rc<RefCell<Object>>>> = const { Cell::new(None) };
    static TYPE_ERROR_PROTOTYPE: Cell<Option<Rc<RefCell<Object>>>> = const { Cell::new(None) };
}

/// Register Error prototype for use in create_js_error (called during init)
pub fn register_error_constructor(error: Value, error_prototype: Rc<RefCell<Object>>) {
    let name = if let Value::Object(obj) = &error {
        obj.borrow()
            .get("name")
            .map(|v| crate::value::to_js_string(&v))
            .unwrap_or_default()
    } else {
        String::new()
    };

    match name.as_str() {
        "Error" => ERROR_PROTOTYPE.with(|cell| cell.set(Some(error_prototype))),
        "TypeError" => TYPE_ERROR_PROTOTYPE.with(|cell| cell.set(Some(error_prototype))),
        _ => ERROR_PROTOTYPE.with(|cell| cell.set(Some(error_prototype))),
    }
}

/// Create a JS Error object and set it as the thrown value.
/// Tries Test262Error first (preferred by test262 harness), falls back to Error.
/// Returns the error value and the JsError wrapper.
pub fn create_js_error(message: &str) -> (Value, JsError) {
    create_js_error_with_type(message, "Error")
}

/// Create a JS Error object with a specific error type.
pub fn create_js_error_with_type(message: &str, error_type: &str) -> (Value, JsError) {
    // First try to get Error from CURRENT_CONTEXT
    let ctx_ptr = CURRENT_CONTEXT.with(|cell| *cell.borrow());

    if let Some(p) = ctx_ptr {
        // SAFETY: ctx_ptr is valid because CURRENT_CONTEXT is set during eval
        let ctx = unsafe { &mut *p };
        let ctor = match error_type {
            "SyntaxError" | "TypeError" | "ReferenceError" | "RangeError"
            | "EvalError" | "URIError" | "InternalError" => ctx
                .get_global(error_type)
                .or_else(|| ctx.get_global("Test262Error"))
                .or_else(|| ctx.get_global("Error")),
            _ => ctx
                .get_global("Test262Error")
                .or_else(|| ctx.get_global("Error")),
        };

        if let Some(ctor_val) = ctor {
            let arg = Value::String(message.to_string());
            let result = crate::eval::call_value_with_this(ctor_val, vec![arg], Value::Undefined);
            if let Ok(v) = result {
                set_thrown_value(v.clone());
                return (v, JsError(message.to_string()));
            }
        }
    }

    // Fallback: try thread-local prototypes
    let proto_rc = if error_type == "TypeError" {
        TYPE_ERROR_PROTOTYPE
            .with(|cell| cell.take())
            .inspect(|p| {
                TYPE_ERROR_PROTOTYPE.with(|c| c.set(Some(p.clone())));
            })
            .or_else(|| {
                ERROR_PROTOTYPE.with(|cell| cell.take()).inspect(|p| {
                    ERROR_PROTOTYPE.with(|c| c.set(Some(p.clone())));
                })
            })
    } else {
        ERROR_PROTOTYPE.with(|cell| cell.take()).inspect(|p| {
            ERROR_PROTOTYPE.with(|c| c.set(Some(p.clone())));
        })
    };

    if let Some(proto_rc) = proto_rc {
        let arg = Value::String(message.to_string());
        let mut obj = Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&proto_rc));
        obj.set("message", arg);
        obj.set("name", Value::String(error_type.to_string()));
        let err_val = Value::Object(Rc::new(RefCell::new(obj)));
        set_thrown_value(err_val.clone());
        return (err_val, JsError(message.to_string()));
    }

    // Last resort: create minimal object without prototype chain
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("message", Value::String(message.to_string()));
    obj.set("name", Value::String(error_type.to_string()));
    let err = Value::Object(Rc::new(RefCell::new(obj)));
    set_thrown_value(err.clone());
    (err, JsError(message.to_string()))
}
