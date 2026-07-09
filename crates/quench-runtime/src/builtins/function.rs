//! Function built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

// Thread-local storage for Function.prototype (used by interpreter for function expressions)
thread_local! {
    static FUNCTION_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

/// Get the Function.prototype object (for use by interpreter)
pub fn get_function_prototype() -> Option<Rc<RefCell<Object>>> {
    FUNCTION_PROTOTYPE.with(|fp| fp.borrow().clone())
}

// ============================================================================
// Function
// ============================================================================

pub fn register_function(ctx: &mut Context) {
    // Function.prototype - the object that is the prototype of all function objects
    let function_proto = Object::new(ObjectKind::Function);
    let function_proto_rc = Rc::new(RefCell::new(function_proto));
    let function_proto_clone = Rc::clone(&function_proto_rc);
    
    // Store Function.prototype for interpreter to use
    FUNCTION_PROTOTYPE.with(|fp| {
        *fp.borrow_mut() = Some(Rc::clone(&function_proto_rc));
    });
    
    // Set Function.prototype's prototype to Object.prototype
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        function_proto_rc.borrow_mut().prototype = Some(object_proto);
    }
    
    // Function.prototype.toString - returns a string representation of the function
    function_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::String("[Function]".to_string()))
    }))));
    
    // Function.prototype.call - placeholder, interpreter handles this specially
    function_proto_rc.borrow_mut().set("call", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Err(crate::value::JsError("Function.prototype.call must be called on a function".to_string()))
    }))));
    
    // Function.prototype.apply - calls the function with a given this value and array of arguments
    function_proto_rc.borrow_mut().set("apply", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Err(crate::value::JsError("Function.prototype.apply must be called on a function".to_string()))
    }))));
    
    // Function.prototype.bind - creates a new function that has its this value bound
    function_proto_rc.borrow_mut().set("bind", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Err(crate::value::JsError("Function.prototype.bind must be called on a function".to_string()))
    }))));

    // Function constructor with prototype
    let function_constructor = NativeConstructor::new(
        move |_args| {
            // Function constructor creates a new function from arguments
            // In practice, we just return an empty function
            let func = Object::with_prototype(ObjectKind::Function, Rc::clone(&function_proto_clone));
            let func_rc = Rc::new(RefCell::new(func));
            Ok(Value::Object(func_rc))
        },
        function_proto_rc,
    );

    ctx.set_global("Function".to_string(), Value::NativeConstructor(Rc::new(function_constructor)));
}
