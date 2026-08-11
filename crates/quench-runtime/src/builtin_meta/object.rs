//! Object method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::ObjectPrototypeValueOf => Some("Object.prototype.valueOf"),
        Builtin::ObjectHasOwnProperty => Some("Object.prototype.hasOwnProperty"),
        Builtin::ObjectPropertyIsEnumerable => Some("Object.prototype.propertyIsEnumerable"),
        Builtin::ObjectGetOwnPropertyDescriptor => Some("Object.getOwnPropertyDescriptor"),
        Builtin::ObjectKeys => Some("Object.keys"),
        Builtin::ObjectIs => Some("Object.is"),
        Builtin::ObjectAssign => Some("Object.assign"),
        Builtin::ObjectDefineProperty => Some("Object.defineProperty"),
        Builtin::ObjectGetOwnPropertyNames => Some("Object.getOwnPropertyNames"),
        Builtin::ObjectGetOwnPropertySymbols => Some("Object.getOwnPropertySymbols"),
        Builtin::WeakRefDeref => Some("WeakRef.prototype.deref"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::ObjectPrototypeValueOf | Builtin::BoxedValueOf | Builtin::BooleanValueOf => {
            Some(0.0)
        }
        Builtin::ObjectHasOwnProperty
        | Builtin::ObjectPropertyIsEnumerable
        | Builtin::ObjectKeys
        | Builtin::ObjectGetOwnPropertyNames
        | Builtin::ObjectGetOwnPropertySymbols => Some(1.0),
        Builtin::ObjectGetOwnPropertyDescriptor | Builtin::ObjectIs => Some(2.0),
        Builtin::ObjectAssign => Some(2.0),
        Builtin::WeakRefDeref => Some(0.0),
        Builtin::ObjectDefineProperty => Some(3.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::ObjectPrototypeValueOf | Builtin::BoxedValueOf | Builtin::BooleanValueOf => {
            Some("valueOf")
        }
        Builtin::ObjectHasOwnProperty => Some("hasOwnProperty"),
        Builtin::ObjectPropertyIsEnumerable => Some("propertyIsEnumerable"),
        Builtin::ObjectGetOwnPropertyDescriptor => Some("getOwnPropertyDescriptor"),
        Builtin::ObjectKeys => Some("keys"),
        Builtin::ObjectIs => Some("is"),
        Builtin::ObjectAssign => Some("assign"),
        Builtin::ObjectDefineProperty => Some("defineProperty"),
        Builtin::ObjectGetOwnPropertyNames => Some("getOwnPropertyNames"),
        Builtin::ObjectGetOwnPropertySymbols => Some("getOwnPropertySymbols"),
        Builtin::WeakRefDeref => Some("deref"),
        _ => None,
    }
}
