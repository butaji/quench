//! Error built-ins

use std::cell::RefCell;
use std::rc::Rc;

use crate::interpreter::get_native_this;
use crate::value::error::{create_js_error_with_type, set_thrown_value};
use crate::value::object::PropertyDescriptor;
use crate::value::{NativeConstructor, Object, ObjectKind, PropertyFlags, Value};
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
    // Per ES §19.5.6.3.2 / §20.5.8.3.2 (and analogues for each NativeError
    // subclass), the initial value of NativeError.prototype.message is the
    // empty String. Without this, the prototype's message reads back as
    // `undefined` and the propertyHelper.js / assert.sameValue checks fail.
    proto.define(
        "message",
        Value::String(String::new()),
        PropertyFlags {
            value: Some(Value::String(String::new())),
            writable: true,
            enumerable: false,
            configurable: true,
        },
    );
    proto
}

fn convert_error_message(message: Value) -> Result<String, crate::JsError> {
    let primitive = crate::value::primitive::to_primitive(&message, Some("string"))?;
    if matches!(primitive, Value::Symbol(_)) {
        let (_, error) =
            create_js_error_with_type("Cannot convert a Symbol to a string", "TypeError");
        return Err(error);
    }
    Ok(crate::value::convert::to_js_string(&primitive))
}

fn register_error_constructor(ctx: &mut Context, name: &str, proto: &Rc<RefCell<Object>>) {
    let proto_for_closure = Rc::clone(proto);
    let mut constructor = NativeConstructor::new(
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
            error_rc.borrow_mut().error_data = true;
            // Per ES spec: only set message as own property when argument is provided
            // and not undefined. Descriptor uses enumerable: false.
            if let Some(msg) = message {
                if msg != Value::Undefined {
                    // Per ES §20.5.1.1 / §21.4.1 etc. — ToString the message;
                    // ToString = ToPrimitive with hint "string" then to string.
                    // A Symbol result throws TypeError; other thrown errors
                    // propagate unchanged.
                    let msg_str = convert_error_message(msg)?;
                    error_rc.borrow_mut().define_own_property(
                        "message",
                        &PropertyDescriptor {
                            value: Some(Value::String(msg_str)),
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
    if name != "Error" {
        if let Some(error_constructor) = ctx.get_global("Error") {
            constructor.set_own_prototype(error_constructor);
        }
    }
    let ctor = Value::NativeConstructor(Rc::new(constructor));
    // Set Error.prototype.constructor = Error
    let mut prototype = proto.borrow_mut();
    prototype.set("constructor", ctor.clone());
    if let Some(flags) = prototype.descriptors.get_mut("constructor") {
        flags.enumerable = false;
    }
    // Per ES §19.5.6.2 / §20.5.8.2 (NativeError length = 1).
    // Set length as a static property on the constructor so `verifyProperty`
    // (which reads via Object.getOwnPropertyDescriptor) sees {value:1, writable:false,
    // enumerable:false, configurable:true}.
    if let Value::NativeConstructor(nc_ref) = &ctor {
        nc_ref.set_static_method("length", Value::Number(1.0));
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
    let mut constructor = NativeConstructor::new(
        move |args| {
            let mut object =
                Object::with_prototype(ObjectKind::Ordinary, Rc::clone(&constructor_proto));
            let message = match args.get(2).cloned() {
                Some(message) if message != Value::Undefined => {
                    Some(convert_error_message(message)?)
                }
                _ => None,
            };
            if let Some(message) = message {
                object.define_own_property(
                    "message",
                    &PropertyDescriptor {
                        value: Some(Value::String(message)),
                        writable: Some(true),
                        enumerable: Some(false),
                        configurable: Some(true),
                        ..Default::default()
                    },
                );
            }
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
    if let Some(error_constructor) = ctx.get_global("Error") {
        constructor.set_own_prototype(error_constructor);
    }
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
    let mut constructor = NativeConstructor::new(
        move |args| {
            // Per ES §22.1.7.1 AggregateError ( errors, message ): ToString
            // the message argument; throw TypeError if it is a Symbol.
            let msg_str = match args.get(1) {
                Some(Value::Symbol(_)) => {
                    let (err_val, js_err) = create_js_error_with_type(
                        "Cannot convert a Symbol to a string",
                        "TypeError",
                    );
                    set_thrown_value(err_val);
                    return Err(js_err);
                }
                Some(value) if !matches!(value, Value::Undefined) => {
                    // Per ES §7.1.1: ToString = ToPrimitive with hint "string",
                    // then to string. ToPrimitive with hint "string" calls
                    // Symbol.toPrimitive first, then toString, then valueOf.
                    let prim = match crate::value::primitive::to_primitive(value, Some("string")) {
                        Ok(p) => p,
                        Err(e) => return Err(e),
                    };
                    // If the primitive is a Symbol, ToString throws TypeError.
                    if matches!(prim, Value::Symbol(_)) {
                        let (err_val, js_err) = create_js_error_with_type(
                            "Cannot convert a Symbol to a string",
                            "TypeError",
                        );
                        set_thrown_value(err_val);
                        return Err(js_err);
                    }
                    Some(crate::value::convert::to_js_string(&prim))
                }
                _ => None,
            };
            let mut args = args;
            let errors = args.first().cloned().unwrap_or(Value::Undefined);
            if !matches!(&errors, Value::Object(object) if object.borrow().kind == ObjectKind::Array)
            {
                let values = crate::eval::object::iterable_to_list(&errors)?;
                let mut array = Object::new(ObjectKind::Array);
                array.elements = values;
                let value = Value::Object(Rc::new(RefCell::new(array)));
                if let Some(first) = args.first_mut() {
                    *first = value;
                } else {
                    args.push(value);
                }
            }
            let set_fields = |obj: &mut Object| {
                if let Some(errors_arg) = args.first() {
                    obj.set("errors", errors_arg.clone());
                }
                if let Some(msg) = &msg_str {
                    obj.define_own_property(
                        "message",
                        &PropertyDescriptor {
                            value: Some(Value::String(msg.clone())),
                            writable: Some(true),
                            enumerable: Some(false),
                            configurable: Some(true),
                            ..Default::default()
                        },
                    );
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
    constructor.set_static_method("length", Value::Number(2.0));
    if let Some(error_constructor) = ctx.get_global("Error") {
        constructor.set_own_prototype(error_constructor);
    }
    let ctor = Value::NativeConstructor(Rc::new(constructor));
    let mut prototype = proto_rc.borrow_mut();
    prototype.define(
        "constructor",
        ctor.clone(),
        PropertyFlags {
            value: Some(ctor.clone()),
            writable: true,
            enumerable: false,
            configurable: true,
        },
    );
    ctx.set_global("AggregateError".to_string(), ctor);
}
