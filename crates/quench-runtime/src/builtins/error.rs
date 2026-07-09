//! Error built-ins

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::convert::to_js_string;
use crate::value::{JsError, NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;
use crate::interpreter::get_native_this;

pub fn register_error(ctx: &mut Context) {
    let error_proto = create_error_proto("Error");
    let error_proto_rc = Rc::new(RefCell::new(error_proto));

    // Error.prototype inherits from Object.prototype
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        error_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    register_error_constructor(ctx, "Error", &error_proto_rc);
    register_type_error(ctx, &error_proto_rc);
    register_reference_error(ctx, &error_proto_rc);
    register_syntax_error(ctx, &error_proto_rc);
}

fn create_error_proto(name: &str) -> Object {
    let mut proto = Object::new(ObjectKind::Ordinary);
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
            let final_name = if name_str == "undefined" {
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
    let proto_clone = Rc::clone(proto);
    let constructor = NativeConstructor::new(
        move |args| {
            let message = args.first().cloned().unwrap_or(Value::Undefined);
            let error_obj = Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&proto_clone));
            let error_rc = Rc::new(RefCell::new(error_obj));
            error_rc.borrow_mut().set("message", message);
            Ok(Value::Object(error_rc))
        },
        Rc::clone(proto),
    );
    ctx.set_global(
        name.to_string(),
        Value::NativeConstructor(Rc::new(constructor)),
    );
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
