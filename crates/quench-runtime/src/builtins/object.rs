//! Object built-in

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::builtins::object_static::{
    object_assign, object_create, object_define_property, object_entries, object_freeze,
    object_from_entries, object_get_own_property_descriptor, object_get_own_property_names,
    object_get_prototype_of, object_has_own, object_is, object_is_extensible, object_is_frozen,
    object_keys, object_prevent_extensions, object_values,
};
use crate::value::{JsError, NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

thread_local! {
    static OBJECT_PROTOTYPE: RefCell<Option<Rc<RefCell<Object>>>> = const { RefCell::new(None) };
}

pub fn get_object_prototype() -> Option<Rc<RefCell<Object>>> {
    OBJECT_PROTOTYPE.with(|op| op.borrow().clone())
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
    let mut constructor = object_constructor;
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
        Value::NativeFunction(Rc::new(NativeFunction::new(object_assign))),
    );
    constructor.set_static_method(
        "create",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_create))),
    );
    constructor.set_static_method(
        "defineProperty",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_define_property))),
    );
    constructor.set_static_method(
        "getOwnPropertyDescriptor",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            object_get_own_property_descriptor,
        ))),
    );
    constructor.set_static_method(
        "getOwnPropertyNames",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_get_own_property_names))),
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
        "hasOwn",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_has_own))),
    );
    constructor.set_static_method(
        "is",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_is))),
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
        "preventExtensions",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_prevent_extensions))),
    );
    constructor.set_static_method(
        "isExtensible",
        Value::NativeFunction(Rc::new(NativeFunction::new(object_is_extensible))),
    );

    constructor.set_name("Object");
    let object_ctor = Value::NativeConstructor(Rc::new(constructor));
    // Set Object.prototype.constructor = Object
    object_proto_rc
        .borrow_mut()
        .set("constructor", object_ctor.clone());
    ctx.set_global("Object".to_string(), object_ctor);
}

/// Create an object from the argument to Object()
fn create_object_from_arg(args: &[Value]) -> Result<Value, JsError> {
    let obj = if args.is_empty() {
        let mut obj = Object::new(ObjectKind::Ordinary);
        if let Some(proto) = get_object_prototype() {
            obj.prototype = Some(proto);
        }
        obj
    } else {
        match &args[0] {
            Value::Undefined | Value::Null => Object::new(ObjectKind::Ordinary),
            Value::Boolean(b) => {
                let mut obj = boxed_object("Boolean");
                obj.exotic_kind = Some(crate::value::kind::ExoticKind::Boolean);
                set_boxed_value(&mut obj, Value::Boolean(*b));
                obj
            }
            Value::Number(n) => {
                let mut obj = boxed_object("Number");
                obj.exotic_kind = Some(crate::value::kind::ExoticKind::Number);
                set_boxed_value(&mut obj, Value::Number(*n));
                obj
            }
            Value::String(s) => {
                let mut obj = boxed_object("String");
                obj.exotic_kind = Some(crate::value::kind::ExoticKind::String);
                set_boxed_value(&mut obj, Value::String(s.clone()));
                // String exotic object: one indexed property per character plus length
                let chars: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
                let len = chars.len();
                for (i, ch) in chars.iter().enumerate() {
                    obj.properties.insert(i.to_string(), ch.clone());
                }
                obj.elements = chars;
                obj.properties
                    .insert("length".to_string(), Value::Number(len as f64));
                obj
            }
            Value::Symbol(_) => {
                let mut obj = boxed_object("Symbol");
                set_boxed_value(&mut obj, args[0].clone());
                obj
            }
            Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
            | Value::Class(_) => {
                return Ok(args[0].clone());
            }
        }
    };
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// Create a boxed-primitive object linked to the named constructor's
/// prototype (String/Number/Boolean/Symbol), so `instanceof` and prototype
/// methods like `valueOf` behave as specified.
fn boxed_object(constructor_name: &str) -> Object {
    let mut obj = Object::new(ObjectKind::Ordinary);
    if let Some(proto) = constructor_prototype(constructor_name) {
        obj.prototype = Some(proto);
    }
    obj
}

/// Store the primitive payload of a boxed wrapper as a non-enumerable
/// internal property (stands in for the spec's [[PrimitiveData]] slot).
pub(crate) fn set_boxed_value(obj: &mut Object, value: Value) {
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

/// Resolve a global constructor's `prototype` object via the current context.
fn constructor_prototype(name: &str) -> Option<Rc<RefCell<Object>>> {
    let ctx_ptr = crate::context::CURRENT_CONTEXT.with(|cell| *cell.borrow());
    let p = ctx_ptr?;
    // SAFETY: CURRENT_CONTEXT is set for the duration of eval, and native
    // functions only run during eval.
    let ctx = unsafe { &*p };
    match ctx.get_global(name) {
        Some(Value::Object(o)) => match o.borrow().get("prototype") {
            Some(Value::Object(p)) => Some(p),
            _ => None,
        },
        Some(Value::NativeFunction(nf)) => match nf.get_property("prototype") {
            Some(Value::Object(p)) => Some(p),
            _ => None,
        },
        Some(Value::NativeConstructor(nc)) => Some(Rc::clone(&nc.prototype)),
        _ => None,
    }
}

/// Get builtin tag for simple value types.
fn simple_builtin_tag(val: &Value) -> Option<String> {
    let tag = if matches!(val, Value::Undefined) {
        "Undefined"
    } else if matches!(val, Value::Null) {
        "Null"
    } else if matches!(val, Value::Boolean(_)) {
        "Boolean"
    } else if matches!(val, Value::Number(_)) {
        "Number"
    } else if matches!(val, Value::String(_)) {
        "String"
    } else if matches!(val, Value::Symbol(_)) {
        "Symbol"
    } else if matches!(
        val,
        Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
            | Value::Class(_)
    ) {
        "Function"
    } else {
        return None;
    };
    Some(tag.to_string())
}

/// Get the builtin tag string for Object.prototype.toString based on value type
fn get_builtin_tag(this_val: &Value) -> String {
    if let Some(tag) = simple_builtin_tag(this_val) {
        return tag;
    }
    if let Value::Object(o) = this_val {
        return get_object_builtin_tag(o);
    }
    "Object".to_string()
}

/// Get builtin tag for object values
fn get_object_builtin_tag(o: &Rc<RefCell<Object>>) -> String {
    let obj = o.borrow();

    // Check for @@toStringTag first
    if let Some(tag) = get_to_string_tag(&obj.properties) {
        return tag;
    }

    // Check exotic kind for boxed primitives
    if let Some(tag) = get_exotic_kind_tag(&obj.exotic_kind) {
        return tag;
    }

    // Fall back to ObjectKind-based tag
    get_object_kind_tag(obj.kind.clone())
}

/// Extract @@toStringTag from properties.
fn get_to_string_tag(properties: &IndexMap<String, Value>) -> Option<String> {
    for (k, v) in properties {
        if k.starts_with("Symbol(") && k.contains("toStringTag") {
            if let Value::String(tag) = v {
                return Some(tag.clone());
            }
        }
    }
    None
}

/// Get tag from exotic kind.
fn get_exotic_kind_tag(exotic: &Option<crate::value::kind::ExoticKind>) -> Option<String> {
    if let Some(e) = exotic {
        match e {
            crate::value::kind::ExoticKind::String => Some("String".to_string()),
            crate::value::kind::ExoticKind::Number => Some("Number".to_string()),
            crate::value::kind::ExoticKind::Boolean => Some("Boolean".to_string()),
        }
    } else {
        None
    }
}

/// Get tag from ObjectKind.
fn get_object_kind_tag(kind: ObjectKind) -> String {
    let tag = if kind == ObjectKind::Ordinary {
        "Object"
    } else if kind == ObjectKind::Array {
        "Array"
    } else if matches!(
        kind,
        ObjectKind::Function | ObjectKind::ArrowFunction | ObjectKind::Class
    ) {
        "Function"
    } else if kind == ObjectKind::Date {
        "Date"
    } else if kind == ObjectKind::RegExp {
        "RegExp"
    } else if kind == ObjectKind::Map {
        "Map"
    } else if kind == ObjectKind::Set {
        "Set"
    } else if kind == ObjectKind::Promise {
        "Promise"
    } else {
        "global"
    };
    tag.to_string()
}

/// Register methods on Object.prototype
fn register_object_prototype_methods(object_proto_rc: &Rc<RefCell<Object>>) {
    // Object.prototype.toString
    object_proto_rc.borrow_mut().set(
        "toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            let tag = get_builtin_tag(&this_val);
            Ok(Value::String(format!("[object {}]", tag)))
        }))),
    );

    // Object.prototype.valueOf
    object_proto_rc.borrow_mut().set(
        "valueOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(_) = &this_val {
                return Ok(this_val);
            }
            Ok(crate::value::to_object(&this_val))
        }))),
    );

    // Object.prototype.hasOwnProperty
    object_proto_rc.borrow_mut().set(
        "hasOwnProperty",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            object_prototype_has_own_property,
        ))),
    );

    // Object.prototype.isPrototypeOf
    object_proto_rc.borrow_mut().set(
        "isPrototypeOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            object_prototype_is_prototype_of,
        ))),
    );

    // Object.prototype.propertyIsEnumerable
    object_proto_rc.borrow_mut().set(
        "propertyIsEnumerable",
        Value::NativeFunction(Rc::new(NativeFunction::new(
            object_prototype_property_is_enumerable,
        ))),
    );
}

/// Object.prototype.hasOwnProperty - checks if property exists directly on object
fn object_prototype_has_own_property(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key_val = args.first();
    if let Some(key_val) = key_val {
        if let Value::Object(o) = &this_val {
            let obj = o.borrow();

            // Check for symbol properties
            if let Value::Symbol(_) = key_val {
                if obj.has_symbol(key_val) {
                    return Ok(Value::Boolean(true));
                }
            }

            // Check string properties and numeric array indices
            if let Some(key_str) = get_property_key(key_val) {
                if obj.has_own(&key_str) {
                    return Ok(Value::Boolean(true));
                }
            }
        } else if let Value::Function(f) = &this_val {
            // ValueFunction stores properties in a HashMap
            if let Some(key_str) = get_property_key(key_val) {
                if f.get_property(&key_str).is_some() {
                    return Ok(Value::Boolean(true));
                }
                return Ok(Value::Boolean(false));
            }
        } else if let Value::NativeFunction(nf) = &this_val {
            if let Some(key_str) = get_property_key(key_val) {
                // Check built-in properties
                if key_str == "name" || key_str == "length" {
                    return Ok(Value::Boolean(true));
                }
                // Check prototype
                if key_str == "prototype" && nf.prototype.borrow().is_some() {
                    return Ok(Value::Boolean(true));
                }
                // Check user-defined properties
                if nf.get_property(&key_str).is_some() {
                    return Ok(Value::Boolean(true));
                }
            }
        } else if let Value::Class(c) = &this_val {
            if let Some(key_str) = get_property_key(key_val) {
                // Check if this configurable property was deleted
                if c.deleted_properties.borrow().contains(&key_str) {
                    return Ok(Value::Boolean(false));
                }
                if key_str == "name" {
                    return Ok(Value::Boolean(true));
                }
                if key_str == "prototype" {
                    return Ok(Value::Boolean(true)); // classes always have prototype
                }
            }
        }
    }
    Ok(Value::Boolean(false))
}

/// Object.prototype.isPrototypeOf - checks if this object is in prototype chain
fn object_prototype_is_prototype_of(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let Some(Value::Object(v)) = args.first() else {
        return Ok(Value::Boolean(false));
    };
    let mut current = v.borrow().prototype.clone();
    while let Some(proto) = current {
        if Rc::ptr_eq(
            &proto,
            match &this_val {
                Value::Object(o) => o,
                _ => return Ok(Value::Boolean(false)),
            },
        ) {
            return Ok(Value::Boolean(true));
        }
        current = proto.borrow().prototype.clone();
    }
    Ok(Value::Boolean(false))
}

/// Object.prototype.propertyIsEnumerable - checks if property is enumerable
fn object_prototype_property_is_enumerable(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key_val = args.first();
    if let Some(key_val) = key_val {
        if let Value::Object(o) = &this_val {
            let obj = o.borrow();

            // Check for symbol properties first (stored in symbol_properties)
            if let Value::Symbol(_) = key_val {
                if obj.has_symbol(key_val) {
                    // Symbol properties are enumerable by default
                    return Ok(Value::Boolean(true));
                }
            }

            // Check string properties and numeric array indices
            if let Some(key) = get_property_key(key_val) {
                if obj.has_own(&key) {
                    return Ok(Value::Boolean(obj.is_enumerable(&key)));
                }
            }
        }
    }
    Ok(Value::Boolean(false))
}

/// Get a property key from argument (handles strings and symbols)
/// For symbols, returns the raw symbol string (e.g., "Symbol():123")
/// This matches how symbols are stored in properties map
fn get_property_key(arg: &Value) -> Option<String> {
    match arg {
        Value::String(s) => Some(s.clone()),
        // For symbols, return the raw symbol string (e.g., "Symbol():123")
        // Note: to_js_string wraps this as "Symbol(...)" for display purposes,
        // but the raw string is what's stored in properties
        Value::Symbol(s) => Some(s.desc.clone().unwrap_or_default()),
        _ => None,
    }
}
