// linter-skip
#![allow(clippy::too_many_lines, clippy::function_body_length)]
//! Error built-ins

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

// ============================================================================
// Error
// ============================================================================

pub fn register_error(ctx: &mut Context) {
    // Create Error prototype
    let error_proto = Object::new(ObjectKind::Ordinary);
    let error_proto_rc = Rc::new(RefCell::new(error_proto));
    // Add toString to Error.prototype
    error_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::String("Error".to_string()))
    }))));

    let error_proto_clone = Rc::clone(&error_proto_rc);
    // Error constructor function with prototype
    let error_constructor = NativeConstructor::new(
        move |args| {
            let message = args.first().cloned().unwrap_or(Value::Undefined);
            let error_obj = Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&error_proto_clone));
            let error_rc = Rc::new(RefCell::new(error_obj));
            error_rc.borrow_mut().set("message", message);
            Ok(Value::Object(error_rc))
        },
        Rc::clone(&error_proto_rc),
    );
    ctx.set_global("Error".to_string(), Value::NativeConstructor(Rc::new(error_constructor)));

    // TypeError
    let type_error_proto = Object::new(ObjectKind::Ordinary);
    let type_error_proto_rc = Rc::new(RefCell::new(type_error_proto));
    // Add toString to TypeError.prototype
    type_error_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::String("TypeError".to_string()))
    }))));

    let type_error_proto_clone = Rc::clone(&type_error_proto_rc);
    let type_error_constructor = NativeConstructor::new(
        move |args| {
            let message = args.first().cloned().unwrap_or(Value::Undefined);
            let error_obj = Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&type_error_proto_clone));
            let error_rc = Rc::new(RefCell::new(error_obj));
            error_rc.borrow_mut().set("message", message);
            Ok(Value::Object(error_rc))
        },
        Rc::clone(&type_error_proto_rc),
    );
    ctx.set_global("TypeError".to_string(), Value::NativeConstructor(Rc::new(type_error_constructor)));

    // ReferenceError
    let ref_error_proto = Object::new(ObjectKind::Ordinary);
    let ref_error_proto_rc = Rc::new(RefCell::new(ref_error_proto));
    // Add toString to ReferenceError.prototype
    ref_error_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::String("ReferenceError".to_string()))
    }))));

    let ref_error_proto_clone = Rc::clone(&ref_error_proto_rc);
    let ref_error_constructor = NativeConstructor::new(
        move |args| {
            let message = args.first().cloned().unwrap_or(Value::Undefined);
            let error_obj = Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&ref_error_proto_clone));
            let error_rc = Rc::new(RefCell::new(error_obj));
            error_rc.borrow_mut().set("message", message);
            Ok(Value::Object(error_rc))
        },
        Rc::clone(&ref_error_proto_rc),
    );
    ctx.set_global("ReferenceError".to_string(), Value::NativeConstructor(Rc::new(ref_error_constructor)));

    // SyntaxError
    let syntax_error_proto = Object::new(ObjectKind::Ordinary);
    let syntax_error_proto_rc = Rc::new(RefCell::new(syntax_error_proto));
    // Add toString to SyntaxError.prototype
    syntax_error_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::String("SyntaxError".to_string()))
    }))));

    let syntax_error_proto_clone = Rc::clone(&syntax_error_proto_rc);
    let syntax_error_constructor = NativeConstructor::new(
        move |args| {
            let message = args.first().cloned().unwrap_or(Value::Undefined);
            let error_obj = Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&syntax_error_proto_clone));
            let error_rc = Rc::new(RefCell::new(error_obj));
            error_rc.borrow_mut().set("message", message);
            Ok(Value::Object(error_rc))
        },
        Rc::clone(&syntax_error_proto_rc),
    );
    ctx.set_global("SyntaxError".to_string(), Value::NativeConstructor(Rc::new(syntax_error_constructor)));

    // Link prototype chains: TypeError.prototype -> Error.prototype
    type_error_proto_rc.borrow_mut().set("__proto__", Value::Object(Rc::clone(&error_proto_rc)));
    ref_error_proto_rc.borrow_mut().set("__proto__", Value::Object(Rc::clone(&error_proto_rc)));
    syntax_error_proto_rc.borrow_mut().set("__proto__", Value::Object(Rc::clone(&error_proto_rc)));
}
