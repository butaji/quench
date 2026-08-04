//! Error built-ins

use std::cell::RefCell;
use std::rc::Rc;

use crate::interpreter::get_native_this;
use crate::value::convert::to_js_string;
use crate::value::object::PropertyDescriptor;
use crate::value::{NativeConstructor, Object, ObjectKind, Value};
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
        let type_error_proto_rc = match &type_error_val {
            Value::Object(obj) => obj.borrow().get("prototype").and_then(|v| match v {
                Value::Object(rc) => Some(rc.clone()),
                _ => None,
            }),
            Value::NativeConstructor(nc) => Some(Rc::clone(&nc.prototype)),
            _ => None,
        };
        if let Some(proto_rc) = type_error_proto_rc {
            crate::value::register_error_constructor(type_error_val, proto_rc);
        }
    }
    register_reference_error(ctx, &error_proto_rc);
    register_syntax_error(ctx, &error_proto_rc);
    register_range_error(ctx, &error_proto_rc);
    register_eval_error(ctx, &error_proto_rc);
    register_uri_error(ctx, &error_proto_rc);
    register_aggregate_error(ctx, &error_proto_rc);
    register_suppressed_error(ctx, &error_proto_rc);
}

fn create_error_proto(name: &str) -> Object {
    let mut proto = Object::new(ObjectKind::Ordinary);
    proto.set("name", Value::String(name.to_string()));
    proto
}

fn register_error_constructor(ctx: &mut Context, name: &str, proto: &Rc<RefCell<Object>>) {
    let proto_for_closure = Rc::clone(proto);
    let constructor = NativeConstructor::new(
        move |args| {
            let message = args.first().cloned();
            // Use the passed `this` (from super()) or create a new object
            let error_rc = match crate::interpreter::get_native_this() {
                Some(Value::Object(obj)) => obj,
                _ => {
                    let obj =
                        Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&proto_for_closure));
                    Rc::new(RefCell::new(obj))
                }
            };
            // Per ES spec: only set message as own property when argument is provided
            // and not undefined. Descriptor uses enumerable: false.
            if let Some(msg) = message {
                if msg != Value::Undefined {
                    error_rc.borrow_mut().define_own_property(
                        "message",
                        &PropertyDescriptor {
                            value: Some(msg),
                            writable: Some(true),
                            enumerable: Some(false),
                            configurable: Some(true),
                            ..Default::default()
                        },
                    );
                }
            }
            if let Some(Value::Object(options)) = args.get(1) {
                if let Some(cause) = options.borrow().get("cause") {
                    error_rc.borrow_mut().define_own_property(
                        "cause",
                        &PropertyDescriptor {
                            value: Some(cause),
                            writable: Some(true),
                            enumerable: Some(false),
                            configurable: Some(true),
                            ..Default::default()
                        },
                    );
                }
            }
            Ok(Value::Object(error_rc))
        },
        Rc::clone(proto),
    );
    constructor.set_name(name);
    let ctor = Value::NativeConstructor(Rc::new(constructor));
    // Set Error.prototype.constructor = Error
    let mut prototype = proto.borrow_mut();
    prototype.set("constructor", ctor.clone());
    if let Some(flags) = prototype.descriptors.get_mut("constructor") {
        flags.enumerable = false;
    }
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

fn register_suppressed_error(ctx: &mut Context, parent_proto: &Rc<RefCell<Object>>) {
    let mut proto = create_error_proto("SuppressedError");
    proto.prototype = Some(Rc::clone(parent_proto));
    let proto = Rc::new(RefCell::new(proto));
    let constructor_proto = Rc::clone(&proto);
    let constructor = NativeConstructor::new(
        move |args| {
            let mut object =
                Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&constructor_proto));
            object.set("error", args.first().cloned().unwrap_or(Value::Undefined));
            object.set(
                "suppressed",
                args.get(1).cloned().unwrap_or(Value::Undefined),
            );
            Ok(Value::Object(Rc::new(RefCell::new(object))))
        },
        Rc::clone(&proto),
    );
    constructor.set_name("SuppressedError");
    let constructor = Value::NativeConstructor(Rc::new(constructor));
    let mut prototype = proto.borrow_mut();
    prototype.set("constructor", constructor.clone());
    if let Some(flags) = prototype.descriptors.get_mut("constructor") {
        flags.enumerable = false;
    }
    ctx.set_global("SuppressedError".to_string(), constructor);
}

fn register_aggregate_error(ctx: &mut Context, parent_proto: &Rc<RefCell<Object>>) {
    let proto = create_error_proto("AggregateError");
    let proto_rc = Rc::new(RefCell::new(proto));
    proto_rc.borrow_mut().prototype = Some(Rc::clone(parent_proto));
    let proto_for_closure = Rc::clone(&proto_rc);
    let constructor = NativeConstructor::new(
        move |args| {
            let set_fields = |obj: &mut Object| {
                if let Some(errors_arg) = args.first() {
                    obj.set("errors", errors_arg.clone());
                }
                if let Some(msg_arg) = args.get(1) {
                    if !matches!(msg_arg, Value::Undefined) {
                        obj.set("message", Value::String(to_js_string(msg_arg)));
                    }
                }
                if let Some(Value::Object(options)) = args.get(2) {
                    if let Some(cause) = options.borrow().get("cause") {
                        obj.define_own_property(
                            "cause",
                            &PropertyDescriptor {
                                value: Some(cause),
                                writable: Some(true),
                                enumerable: Some(false),
                                configurable: Some(true),
                                ..Default::default()
                            },
                        );
                    }
                }
            };
            if let Some(Value::Object(error_rc)) = get_native_this() {
                let mut obj = error_rc.borrow_mut();
                if obj.prototype.is_none() {
                    obj.prototype = Some(Rc::clone(&proto_for_closure));
                }
                set_fields(&mut obj);
                obj.set("name", Value::String("AggregateError".to_string()));
                drop(obj);
                return Ok(Value::Object(error_rc));
            }
            let error_obj =
                Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&proto_for_closure));
            let error_rc = Rc::new(RefCell::new(error_obj));
            set_fields(&mut error_rc.borrow_mut());
            error_rc
                .borrow_mut()
                .set("name", Value::String("AggregateError".to_string()));
            Ok(Value::Object(error_rc))
        },
        Rc::clone(&proto_rc),
    );
    constructor.set_name("AggregateError");
    let ctor = Value::NativeConstructor(Rc::new(constructor));
    let mut prototype = proto_rc.borrow_mut();
    prototype.set("constructor", ctor.clone());
    if let Some(flags) = prototype.descriptors.get_mut("constructor") {
        flags.enumerable = false;
    }
    ctx.set_global("AggregateError".to_string(), ctor);
}
