//! Object built-in

use std::cell::RefCell;
use std::rc::Rc;

pub mod constructor;
pub mod helpers;
pub mod prototype_methods;
#[cfg(test)]
mod tests;

use crate::builtins::object_static::{
    object_assign, object_create, object_define_properties, object_define_property, object_entries,
    object_freeze, object_from_entries, object_get_own_property_descriptor,
    object_get_own_property_descriptors, object_get_own_property_names,
    object_get_own_property_symbols, object_get_prototype_of, object_has_own,
    object_is_extensible, object_is_frozen, object_is_sealed, object_keys,
    object_prevent_extensions, object_seal, object_set_prototype_of, object_values,
};
use crate::value::{NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

pub use constructor::create_object_from_arg;

thread_local! {
    static OBJECT_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

pub fn get_object_prototype() -> Option<Rc<RefCell<Object>>> {
    OBJECT_PROTOTYPE.with(|op| op.borrow().clone())
}

/// Save the thread-local prototype cache (realm snapshot support)
pub(crate) fn save_object_prototype() -> Option<Rc<RefCell<Object>>> {
    get_object_prototype()
}

/// Restore the thread-local prototype cache (realm snapshot support)
pub(crate) fn restore_object_prototype(proto: Option<Rc<RefCell<Object>>>) {
    OBJECT_PROTOTYPE.with(|op| *op.borrow_mut() = proto);
}

pub fn register_object(ctx: &mut Context) {
    // Object.prototype - the prototype object for all ordinary objects
    let object_proto = Object::new(ObjectKind::Ordinary);
    let object_proto_rc = Rc::new(RefCell::new(object_proto));
    let object_proto_for_ctor = Rc::clone(&object_proto_rc);

    // Set up Object.prototype methods
    register_object_prototype_methods(&object_proto_rc);

    // Store Object.prototype for later use
    OBJECT_PROTOTYPE.with(|op| {
        *op.borrow_mut() = Some(Rc::clone(&object_proto_rc));
    });

    // Create Object constructor as a NativeConstructor
    let object_constructor = NativeConstructor::new(
        move |args: Vec<Value>| create_object_from_arg(&args),
        object_proto_for_ctor,
    );

    // Register static methods on Object constructor
    let constructor = object_constructor;
    constructor.set_static_method(
        "keys",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_keys))),
    );
    constructor.set_static_method(
        "values",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_values))),
    );
    constructor.set_static_method(
        "entries",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_entries))),
    );
    constructor.set_static_method(
        "assign",
        Value::NativeFunction(Rc::new(NativeFunction::new_named("assign", object_assign))),
    );
    constructor.set_static_method(
        "create",
        Value::NativeFunction(Rc::new(NativeFunction::new_named("create", object_create))),
    );
    constructor.set_static_method(
        "defineProperty",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_define_property))),
    );
    constructor.set_static_method(
        "defineProperties",
        Value::NativeFunction(Rc::new(NativeFunction::new_named(
            "defineProperties",
            object_define_properties,
        ))),
    );
    constructor.set_static_method(
        "getOwnPropertyDescriptor",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            object_get_own_property_descriptor,
        ))),
    );
    constructor.set_static_method(
        "getOwnPropertyDescriptors",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            object_get_own_property_descriptors,
        ))),
    );
    constructor.set_static_method(
        "getOwnPropertyNames",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_get_own_property_names))),
    );
    constructor.set_static_method(
        "getOwnPropertySymbols",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            object_get_own_property_symbols,
        ))),
    );
    constructor.set_static_method(
        "freeze",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_freeze))),
    );
    constructor.set_static_method(
        "isFrozen",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_is_frozen))),
    );
    constructor.set_static_method(
        "seal",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_seal))),
    );
    constructor.set_static_method(
        "isSealed",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_is_sealed))),
    );
    constructor.set_static_method(
        "hasOwn",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_has_own))),
    );
    constructor.set_static_method(
        "fromEntries",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_from_entries))),
    );
    constructor.set_static_method(
        "getPrototypeOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_get_prototype_of))),
    );
    constructor.set_static_method(
        "setPrototypeOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_set_prototype_of))),
    );
    constructor.set_static_method(
        "preventExtensions",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_prevent_extensions))),
    );
    constructor.set_static_method(
        "isExtensible",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_is_extensible))),
    );

    constructor.set_name("Object");
    let object_ctor = Value::NativeConstructor(Rc::new(constructor));
    // Set Object.prototype.constructor = Object (non-enumerable)
    object_proto_rc
        .borrow_mut()
        .set_builtin_method("constructor", object_ctor.clone());
    if let Some(Value::Object(global_obj)) = ctx.get_global("globalThis") {
        global_obj.borrow_mut().prototype = Some(Rc::clone(&object_proto_rc));
    }
    ctx.set_global("Object".to_string(), object_ctor);
}

/// Store the primitive payload of a boxed wrapper as a non-enumerable
/// internal property (stands in for the spec's [[PrimitiveData]] slot).
pub(crate) fn set_boxed_value(obj: &mut Object, value: Value) {
    obj.exotic_kind = match value {
        Value::BigInt(_) => Some(crate::value::kind::ExoticKind::BigInt),
        Value::Boolean(_) => Some(crate::value::kind::ExoticKind::Boolean),
        Value::Number(_) => Some(crate::value::kind::ExoticKind::Number),
        Value::String(_) => Some(crate::value::kind::ExoticKind::String),
        Value::Symbol(_) => obj.exotic_kind.clone(),
        _ => obj.exotic_kind.clone(),
    };
    obj.define(
        "_value",
        value,
        crate::value::PropertyFlags {
            value: None,
            writable: true,
            enumerable: false,
            configurable: true,
        },
    );
}

/// Register methods on Object.prototype
fn register_object_prototype_methods(object_proto_rc: &Rc<RefCell<Object>>) {
    use prototype_methods::{
        object_prototype_has_own_property, object_prototype_is_prototype_of,
        object_prototype_lookup_getter, object_prototype_lookup_setter,
        object_prototype_property_is_enumerable,
    };

    // Object.prototype.toString
    object_proto_rc.borrow_mut().set_builtin_method(
        "toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            let tag = helpers::get_builtin_tag(&this_val);
            Ok(Value::String(format!("[object {}]", tag)))
        }))),
    );

    // Object.prototype.toLocaleString — delegates to toString per spec
    object_proto_rc.borrow_mut().set_builtin_method(
        "toLocaleString",
        Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
            // Per ES §19.1.3.5: Return ? Invoke(this, "toString").
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            use std::rc::Rc;
            let scratch_env = Rc::new(std::cell::RefCell::new(crate::env::Environment::new()));
            let method =
                crate::eval::member::eval_member_access(&this_val, "toString", &scratch_env)?;
            crate::eval::function::call_value_with_this(method, args.to_vec(), this_val)
        }))),
    );

    // Object.prototype.valueOf
    object_proto_rc.borrow_mut().set_builtin_method(
        "valueOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            // Return this for any object-like value (Object, Function, Class, etc.)
            match &this_val {
                Value::Object(o) => {
                    // Boxed primitives (new Boolean, new Number, new String, Object(BigInt))
                    // store the wrapped value as _value; valueOf must unwrap it.
                    let has_boxed = o.borrow().get("_value");
                    if let Some(boxed) = has_boxed {
                        Ok(boxed)
                    } else {
                        Ok(this_val)
                    }
                }
                Value::Function(_)
                | Value::NativeFunction(_)
                | Value::NativeConstructor(_)
                | Value::Generator(_)
                | Value::Class(_) => Ok(this_val),
                _ => crate::value::to_object(&this_val),
            }
        }))),
    );

    // Object.prototype.hasOwnProperty
    object_proto_rc.borrow_mut().set_builtin_method(
        "hasOwnProperty",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            object_prototype_has_own_property,
        ))),
    );

    // Object.prototype.isPrototypeOf
    object_proto_rc.borrow_mut().set_builtin_method(
        "isPrototypeOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            object_prototype_is_prototype_of,
        ))),
    );

    // Object.prototype.propertyIsEnumerable
    object_proto_rc.borrow_mut().set_builtin_method(
        "propertyIsEnumerable",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            object_prototype_property_is_enumerable,
        ))),
    );

    object_proto_rc.borrow_mut().set_builtin_method(
        "__lookupGetter__",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_prototype_lookup_getter))),
    );

    object_proto_rc.borrow_mut().set_builtin_method(
        "__lookupSetter__",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_prototype_lookup_setter))),
    );
}
