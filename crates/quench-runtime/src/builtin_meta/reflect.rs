//! Reflect method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::ReflectApply => Some("Reflect.apply"),
        Builtin::ReflectConstruct => Some("Reflect.construct"),
        Builtin::ReflectDefineProperty => Some("Reflect.defineProperty"),
        Builtin::ReflectDeleteProperty => Some("Reflect.deleteProperty"),
        Builtin::ReflectGet => Some("Reflect.get"),
        Builtin::ReflectGetOwnPropertyDescriptor => Some("Reflect.getOwnPropertyDescriptor"),
        Builtin::ReflectGetPrototypeOf => Some("Reflect.getPrototypeOf"),
        Builtin::ReflectHas => Some("Reflect.has"),
        _ => fn_name_tail(builtin),
    }
}

const fn fn_name_tail(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::ReflectIsExtensible => Some("Reflect.isExtensible"),
        Builtin::ReflectOwnKeys => Some("Reflect.ownKeys"),
        Builtin::ReflectPreventExtensions => Some("Reflect.preventExtensions"),
        Builtin::ReflectSet => Some("Reflect.set"),
        Builtin::ReflectSetPrototypeOf => Some("Reflect.setPrototypeOf"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::ReflectApply | Builtin::ReflectDefineProperty | Builtin::ReflectSet => Some(3.0),
        Builtin::ReflectConstruct
        | Builtin::ReflectDeleteProperty
        | Builtin::ReflectGet
        | Builtin::ReflectGetOwnPropertyDescriptor
        | Builtin::ReflectHas
        | Builtin::ReflectSetPrototypeOf => Some(2.0),
        Builtin::ReflectGetPrototypeOf
        | Builtin::ReflectIsExtensible
        | Builtin::ReflectOwnKeys
        | Builtin::ReflectPreventExtensions => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::ReflectApply => Some("apply"),
        Builtin::ReflectConstruct => Some("construct"),
        Builtin::ReflectDefineProperty => Some("defineProperty"),
        Builtin::ReflectDeleteProperty => Some("deleteProperty"),
        Builtin::ReflectGet => Some("get"),
        Builtin::ReflectGetOwnPropertyDescriptor => Some("getOwnPropertyDescriptor"),
        Builtin::ReflectGetPrototypeOf => Some("getPrototypeOf"),
        Builtin::ReflectHas => Some("has"),
        _ => short_name_tail(builtin),
    }
}

const fn short_name_tail(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::ReflectIsExtensible => Some("isExtensible"),
        Builtin::ReflectOwnKeys => Some("ownKeys"),
        Builtin::ReflectPreventExtensions => Some("preventExtensions"),
        Builtin::ReflectSet => Some("set"),
        Builtin::ReflectSetPrototypeOf => Some("setPrototypeOf"),
        _ => None,
    }
}
