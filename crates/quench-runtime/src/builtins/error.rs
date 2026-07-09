//! Error built-ins

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

pub fn register_error(ctx: &mut Context) {
    let error_proto = create_error_proto("Error");
    let error_proto_rc = Rc::new(RefCell::new(error_proto));

    register_error_constructor(ctx, "Error", &error_proto_rc);
    register_type_error(ctx, &error_proto_rc);
    register_reference_error(ctx, &error_proto_rc);
    register_syntax_error(ctx, &error_proto_rc);
}

fn create_error_proto(name: &str) -> Object {
    let proto = Object::new(ObjectKind::Ordinary);
    proto.set(
        "toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(move |_args| {
            Ok(Value::String(name.to_string()))
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
    proto_rc
        .borrow_mut()
        .set("__proto__", Value::Object(Rc::clone(parent_proto)));
    register_error_constructor(ctx, "TypeError", &proto_rc);
}

fn register_reference_error(ctx: &mut Context, parent_proto: &Rc<RefCell<Object>>) {
    let proto = create_error_proto("ReferenceError");
    let proto_rc = Rc::new(RefCell::new(proto));
    proto_rc
        .borrow_mut()
        .set("__proto__", Value::Object(Rc::clone(parent_proto)));
    register_error_constructor(ctx, "ReferenceError", &proto_rc);
}

fn register_syntax_error(ctx: &mut Context, parent_proto: &Rc<RefCell<Object>>) {
    let proto = create_error_proto("SyntaxError");
    let proto_rc = Rc::new(RefCell::new(proto));
    proto_rc
        .borrow_mut()
        .set("__proto__", Value::Object(Rc::clone(parent_proto)));
    register_error_constructor(ctx, "SyntaxError", &proto_rc);
}
