//! Error built-ins

use std::cell::RefCell;
use std::rc::Rc;

use crate::interpreter::get_native_this;
use crate::value::convert::to_js_string;
use crate::value::{JsError, NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

#[cfg(test)]
mod tests;

pub fn register_error(ctx: &mut Context) {
    let error_proto = create_error_proto("Error");
    let error_proto_rc = Rc::new(RefCell::new(error_proto));

    // Error.prototype inherits from Object.prototype
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        error_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    register_error_constructor(ctx, "Error", &error_proto_rc);

    // Register Error globally for create_js_error (for errors thrown outside eval context)
    let error_val = ctx.get_global("Error").unwrap();
    crate::value::register_error_constructor(error_val, Rc::clone(&error_proto_rc));

    register_type_error(ctx, &error_proto_rc);

    // Register TypeError globally for create_js_error_with_type
    if let Some(type_error_val) = ctx.get_global("TypeError") {
        if let Value::Object(type_error_obj) = &type_error_val {
            let type_error_proto = type_error_obj.borrow().get("prototype");
            if let Some(Value::Object(type_error_proto_rc)) = type_error_proto {
                crate::value::register_error_constructor(
                    type_error_val,
                    Rc::clone(&type_error_proto_rc),
                );
            }
        }
    }
    register_reference_error(ctx, &error_proto_rc);
    register_syntax_error(ctx, &error_proto_rc);
    register_range_error(ctx, &error_proto_rc);
    register_eval_error(ctx, &error_proto_rc);
    register_uri_error(ctx, &error_proto_rc);
}

fn create_error_proto(name: &str) -> Object {
    let mut proto = Object::new(ObjectKind::Ordinary);
    proto.set("name", Value::String(name.to_string()));
    let default_name = name.to_string();
    proto.set(
        "toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |_args| {
            let this = get_native_this().unwrap_or(Value::Undefined);
            let Value::Object(obj_rc) = &this else {
                return Err(JsError::from(
                    "Error.prototype.toString called on non-object",
                ));
            };
            let name_val = obj_rc.borrow().get("name").unwrap_or(Value::Undefined);
            let name_str = to_js_string(&name_val);
            let final_name = if name_str == "undefined" || name_str.is_empty() {
                &default_name
            } else {
                &name_str
            };
            let msg_val = obj_rc.borrow().get("message").unwrap_or(Value::Undefined);
            let msg_str = match &msg_val {
                Value::Undefined => String::new(),
                _ => to_js_string(&msg_val),
            };
            if final_name.is_empty() {
                return Ok(Value::String(msg_str));
            }
            if msg_str.is_empty() {
                return Ok(Value::String(final_name.clone()));
            }
            Ok(Value::String(format!("{}: {}", final_name, msg_str)))
        }))),
    );
    proto
}

fn register_error_constructor(ctx: &mut Context, name: &str, proto: &Rc<RefCell<Object>>) {
    let proto_for_closure = Rc::clone(proto);
    let name_str = name.to_string();
    let constructor = NativeConstructor::new(
        move |args| {
            let name_str = name_str.clone();
            let set_message = |obj: &mut Object| {
                if let Some(msg_arg) = args.first() {
                    if !matches!(msg_arg, Value::Undefined) {
                        obj.set("message", Value::String(to_js_string(msg_arg)));
                    }
                }
            };
            if let Some(Value::Object(error_rc)) = get_native_this() {
                let mut obj = error_rc.borrow_mut();
                if obj.prototype.is_none() {
                    obj.prototype = Some(Rc::clone(&proto_for_closure));
                }
                set_message(&mut obj);
                obj.set("name", Value::String(name_str));
                drop(obj);
                return Ok(Value::Object(error_rc));
            }
            let error_obj =
                Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&proto_for_closure));
            let error_rc = Rc::new(RefCell::new(error_obj));
            set_message(&mut error_rc.borrow_mut());
            error_rc.borrow_mut().set("name", Value::String(name_str));
            Ok(Value::Object(error_rc))
        },
        Rc::clone(proto),
    );
    constructor.set_name(name);
    let ctor = Value::NativeConstructor(Rc::new(constructor));
    // Set Error.prototype.constructor = Error
    proto.borrow_mut().set("constructor", ctor.clone());
    ctx.set_global(name.to_string(), ctor);
}

fn register_type_error(ctx: &mut Context, parent_proto: &Rc<RefCell<Object>>) {
    let proto = create_error_proto("TypeError");
    let proto_rc = Rc::new(RefCell::new(proto));
    proto_rc.borrow_mut().prototype = Some(Rc::clone(parent_proto));
    register_error_constructor(ctx, "TypeError", &proto_rc);
}

fn register_reference_error(ctx: &mut Context, parent_proto: &Rc<RefCell<Object>>) {
    let proto = create_error_proto("ReferenceError");
    let proto_rc = Rc::new(RefCell::new(proto));
    proto_rc.borrow_mut().prototype = Some(Rc::clone(parent_proto));
    register_error_constructor(ctx, "ReferenceError", &proto_rc);
}

fn register_syntax_error(ctx: &mut Context, parent_proto: &Rc<RefCell<Object>>) {
    let proto = create_error_proto("SyntaxError");
    let proto_rc = Rc::new(RefCell::new(proto));
    proto_rc.borrow_mut().prototype = Some(Rc::clone(parent_proto));
    register_error_constructor(ctx, "SyntaxError", &proto_rc);
}

fn register_range_error(ctx: &mut Context, parent_proto: &Rc<RefCell<Object>>) {
    let proto = create_error_proto("RangeError");
    let proto_rc = Rc::new(RefCell::new(proto));
    proto_rc.borrow_mut().prototype = Some(Rc::clone(parent_proto));
    register_error_constructor(ctx, "RangeError", &proto_rc);
}

fn register_eval_error(ctx: &mut Context, parent_proto: &Rc<RefCell<Object>>) {
    let proto = create_error_proto("EvalError");
    let proto_rc = Rc::new(RefCell::new(proto));
    proto_rc.borrow_mut().prototype = Some(Rc::clone(parent_proto));
    register_error_constructor(ctx, "EvalError", &proto_rc);
}

fn register_uri_error(ctx: &mut Context, parent_proto: &Rc<RefCell<Object>>) {
    let proto = create_error_proto("URIError");
    let proto_rc = Rc::new(RefCell::new(proto));
    proto_rc.borrow_mut().prototype = Some(Rc::clone(parent_proto));
    register_error_constructor(ctx, "URIError", &proto_rc);
}
