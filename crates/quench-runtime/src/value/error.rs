//! Error handling for JavaScript runtime errors.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use super::{Object, ObjectKind, Value};
use crate::context::CURRENT_CONTEXT;

/// JavaScript error - wraps error messages
#[derive(Clone, PartialEq)]
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

// Thread-local storage for the native Test262Error constructor and prototype.
// Stored here so they survive even after JS harness files (e.g. sta.js)
// overwrite the global Test262Error binding.
thread_local! {
    static TEST262_ERROR: RefCell<Option<Value>> = const { RefCell::new(None) };
    static TEST262_ERROR_PROTO: RefCell<Option<Rc<RefCell<Object>>>> =
        const { RefCell::new(None) };
    // The Test262Error Value from the MAIN realm, cloned here so it can be used
    // by create_js_error_with_type even when CURRENT_CONTEXT points to a sub-realm
    // (e.g., inside $262.createRealm().global.eval(...)). This ensures the wrapped
    // error's .constructor is the main realm's Test262Error, so that
    // err.constructor === Test262Error works in test code comparing against the
    // main realm's global.
    static MAIN_REALM_TEST262_ERROR: RefCell<Option<Value>> =
        const { RefCell::new(None) };
}

/// Set the native Test262Error constructor (called by harness injection)
pub fn set_test262_error(val: Value) {
    TEST262_ERROR.with(|cell| *cell.borrow_mut() = Some(val));
}

/// Set the Test262Error prototype (called by harness injection)
pub fn set_test262_error_proto(proto: Rc<RefCell<Object>>) {
    TEST262_ERROR_PROTO.with(|cell| *cell.borrow_mut() = Some(proto));
}

/// Set the main realm's Test262Error Value (called by harness injection).
/// This is the Value::Function from sta.js (or NativeConstructor before it loads).
/// Used by create_js_error_with_type to ensure wrapped errors always have the
/// main realm's Test262Error as their .constructor.
pub fn set_main_realm_test262_error(val: Value) {
    MAIN_REALM_TEST262_ERROR.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(val);
        }
    });
}

/// Get the main realm's Test262Error Value (used by create_js_error_with_type)
fn get_main_realm_test262_error() -> Option<Value> {
    MAIN_REALM_TEST262_ERROR.with(|cell| cell.borrow().clone())
}

/// Get the native Test262Error constructor (used by create_js_error_with_type)
pub fn get_test262_error() -> Option<Value> {
    TEST262_ERROR.with(|cell| cell.borrow().clone())
}

/// Get the native Test262Error constructor with its prototype.
/// Returns (prototype, constructor_value) for building error objects with proper
/// prototype chains and constructor identity.
pub fn get_test262_error_with_proto() -> Option<(Rc<RefCell<Object>>, Value)> {
    let ctor = get_test262_error()?;
    let proto = TEST262_ERROR_PROTO.with(|cell| cell.borrow().clone());
    proto.map(|p| (p, ctor))
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

/// Helper: call a function (NativeFunction or ValueFunction) as a constructor
/// to produce an error object. Sets thrown value and returns it.
fn call_function_constructor<F>(ctor: &Value, message: &str, get_proto: F) -> Value
where
    F: Fn(&Value) -> Option<Value>,
{
    use crate::eval::call_value_with_this;

    let arg = Value::String(message.to_string());
    let proto_rc = if let Some(Value::Object(p)) = get_proto(ctor) {
        p.clone()
    } else {
        get_test262_error_with_proto()
            .map(|(p, _)| p)
            .unwrap_or_else(|| {
                crate::builtins::get_object_prototype().expect("Object.prototype must be available")
            })
    };
    let mut new_obj = Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&proto_rc));
    // Explicitly set constructor so `err.constructor === Test262Error` works (identity check).
    new_obj.set("constructor", ctor.clone());
    let new_obj_rc = Rc::new(RefCell::new(new_obj));
    let result = call_value_with_this(
        ctor.clone(),
        vec![arg],
        Value::Object(Rc::clone(&new_obj_rc)),
    );
    // Per ES spec: if constructor returns a non-object, use the new object.
    // Always ensure .constructor === ctor for identity checks.
    let final_val = match result {
        Ok(v)
            if matches!(
                v,
                Value::Object(_)
                    | Value::Function(_)
                    | Value::NativeFunction(_)
                    | Value::NativeConstructor(_)
                    | Value::Class(_)
            ) =>
        {
            // Set constructor on the returned object/function so identity check passes.
            match &v {
                Value::Object(ref obj_rc) => {
                    obj_rc.borrow_mut().set("constructor", ctor.clone());
                }
                Value::Function(ref f) => {
                    let _ = f.set_property("constructor", ctor.clone());
                }
                _ => {}
            }
            v
        }
        Ok(_) => Value::Object(Rc::clone(&new_obj_rc)),
        Err(_) => Value::Object(Rc::clone(&new_obj_rc)),
    };
    set_thrown_value(final_val.clone());
    final_val
}

/// Create a JS Error object with a specific error type.
pub fn create_js_error_with_type(message: &str, error_type: &str) -> (Value, JsError) {
    // For Test262Error, use the main realm's Test262Error Value (stored during
    // harness injection). This is critical when CURRENT_CONTEXT points to a sub-realm
    // (e.g., inside $262.createRealm().global.eval(...)): we still want the wrapped
    // error's .constructor to be the main realm's Test262Error, so that
    // err.constructor === Test262Error works in test code running in the main realm.
    if error_type == "Test262Error" {
        if let Some(te) = get_main_realm_test262_error() {
            let final_val = match &te {
                Value::NativeConstructor(nc) => {
                    // Build error with the main realm's Test262Error as .constructor.
                    // The ctor_for_obj is the same as te (main realm's Test262Error).
                    let ctor_for_obj = te.clone();
                    let proto_rc = Rc::clone(&nc.prototype);
                    let mut new_obj =
                        Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&proto_rc));
                    new_obj.set("constructor", ctor_for_obj.clone());
                    let new_obj_rc = Rc::new(RefCell::new(new_obj));
                    let arg = Value::String(message.to_string());
                    let result = crate::eval::call_value_with_this(
                        te.clone(),
                        vec![arg],
                        Value::Object(Rc::clone(&new_obj_rc)),
                    );
                    match result {
                        Ok(Value::Object(_)) => result.unwrap(),
                        Ok(_) | Err(_) => Value::Object(Rc::clone(&new_obj_rc)),
                    }
                }
                Value::NativeFunction(_) | Value::Function(_) => {
                    call_function_constructor(&te, message, |v| match v {
                        Value::NativeFunction(f) => f.get_property("prototype"),
                        Value::Function(f) => f.get_property("prototype"),
                        _ => None,
                    })
                }
                _ => {
                    // Unexpected type; build a plain error with main realm Test262Error.
                    let proto = get_test262_error_with_proto()
                        .map(|(p, _)| p)
                        .unwrap_or_else(|| {
                            Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)))
                        });
                    let mut err_obj = Object::with_prototype(ObjectKind::Ordinary, proto);
                    err_obj.set("message", Value::String(message.to_string()));
                    err_obj.set("name", Value::String("Test262Error".to_string()));
                    err_obj.set("constructor", te.clone());
                    Value::Object(Rc::new(RefCell::new(err_obj)))
                }
            };
            return (final_val, JsError(format!("Test262Error: {}", message)));
        }
        // No main realm Test262Error: fall back to thread-local native Test262Error.
        if let Some((proto, constructor)) = get_test262_error_with_proto() {
            let mut obj = Object::with_prototype(ObjectKind::Ordinary, proto);
            obj.set("message", Value::String(message.to_string()));
            obj.set("name", Value::String("Test262Error".to_string()));
            obj.set("constructor", constructor);
            let err_val = Value::Object(Rc::new(RefCell::new(obj)));
            return (err_val, JsError(format!("Test262Error: {}", message)));
        }
    }

    // First try to get Error from CURRENT_CONTEXT
    let ctx_ptr = CURRENT_CONTEXT.with(|cell| *cell.borrow());

    if let Some(p) = ctx_ptr {
        // SAFETY: ctx_ptr is valid because CURRENT_CONTEXT is set during eval;
        // we only use it immutably (get_global) so this does not conflict with
        // the outer &mut Context from eval_impl.
        let ctx = unsafe { &*p };
        let ctor = match error_type {
            "SyntaxError" | "TypeError" | "ReferenceError" | "RangeError" | "EvalError"
            | "URIError" | "InternalError" => ctx
                .get_global(error_type)
                .or_else(|| ctx.get_global("Error")),
            _ => ctx
                .get_global("Error")
                .or_else(|| ctx.get_global("Test262Error")),
        };

        if let Some(ctor_val) = ctor {
            let arg = Value::String(message.to_string());
            let result = crate::eval::call_value_with_this(ctor_val, vec![arg], Value::Undefined);
            if let Ok(v) = result {
                set_thrown_value(v.clone());
                return (v, JsError(format!("{}: {}", error_type, message)));
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
        return (err_val, JsError(format!("{}: {}", error_type, message)));
    }

    // Last resort: create minimal object without prototype chain
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("message", Value::String(message.to_string()));
    obj.set("name", Value::String(error_type.to_string()));
    let err = Value::Object(Rc::new(RefCell::new(obj)));
    set_thrown_value(err.clone());
    (err, JsError(format!("{}: {}", error_type, message)))
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
