//! Object method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::ObjectPrototypeValueOf => Some("Object.prototype.valueOf"),
        Builtin::ObjectHasOwnProperty => Some("Object.prototype.hasOwnProperty"),
        Builtin::ObjectPrototypeIsPrototypeOf => Some("Object.prototype.isPrototypeOf"),
        Builtin::ObjectPrototypeDefineGetter => Some("Object.prototype.__defineGetter__"),
        Builtin::ObjectPrototypeDefineSetter => Some("Object.prototype.__defineSetter__"),
        Builtin::ObjectPrototypeLookupGetter => Some("Object.prototype.__lookupGetter__"),
        Builtin::ObjectPrototypeLookupSetter => Some("Object.prototype.__lookupSetter__"),
        Builtin::ObjectPropertyIsEnumerable => Some("Object.prototype.propertyIsEnumerable"),
        Builtin::ObjectGetOwnPropertyDescriptor => Some("Object.getOwnPropertyDescriptor"),
        Builtin::ObjectKeys => Some("Object.keys"),
        Builtin::ObjectValues => Some("Object.values"),
        Builtin::ObjectEntries => Some("Object.entries"),
        Builtin::ObjectIs => Some("Object.is"),
        Builtin::ObjectAssign => Some("Object.assign"),
        Builtin::ObjectFromEntries => Some("Object.fromEntries"),
        Builtin::ObjectGroupBy => Some("Object.groupBy"),
        Builtin::ObjectDefineProperty => Some("Object.defineProperty"),
        Builtin::ObjectGetOwnPropertyNames => Some("Object.getOwnPropertyNames"),
        Builtin::ObjectGetOwnPropertySymbols => Some("Object.getOwnPropertySymbols"),
        Builtin::WeakRefDeref => Some("WeakRef.prototype.deref"),
        Builtin::ProxyRevocable => Some("Proxy.revocable"),
        Builtin::ProxyRevoke => Some("revoke"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::ObjectPrototypeValueOf | Builtin::BoxedValueOf | Builtin::BooleanValueOf => {
            Some(0.0)
        }
        Builtin::ObjectHasOwnProperty
        | Builtin::ObjectPrototypeIsPrototypeOf
        | Builtin::ObjectPrototypeDefineGetter
        | Builtin::ObjectPrototypeDefineSetter
        | Builtin::ObjectPrototypeLookupGetter
        | Builtin::ObjectPrototypeLookupSetter
        | Builtin::ObjectPropertyIsEnumerable
        | Builtin::ObjectKeys
        | Builtin::ObjectValues
        | Builtin::ObjectEntries
        | Builtin::ObjectGetOwnPropertyNames
        | Builtin::ObjectGetOwnPropertySymbols => Some(1.0),
        Builtin::ObjectGetOwnPropertyDescriptor | Builtin::ObjectIs => Some(2.0),
        Builtin::ObjectAssign => Some(2.0),
        Builtin::ObjectFromEntries => Some(1.0),
        Builtin::ObjectGroupBy => Some(2.0),
        Builtin::WeakRefDeref => Some(0.0),
        Builtin::ProxyRevocable => Some(2.0),
        Builtin::ProxyRevoke => Some(0.0),
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
        Builtin::ObjectPrototypeIsPrototypeOf => Some("isPrototypeOf"),
        Builtin::ObjectPrototypeDefineGetter => Some("__defineGetter__"),
        Builtin::ObjectPrototypeDefineSetter => Some("__defineSetter__"),
        Builtin::ObjectPrototypeLookupGetter => Some("__lookupGetter__"),
        Builtin::ObjectPrototypeLookupSetter => Some("__lookupSetter__"),
        Builtin::ObjectPropertyIsEnumerable => Some("propertyIsEnumerable"),
        Builtin::ObjectGetOwnPropertyDescriptor => Some("getOwnPropertyDescriptor"),
        Builtin::ObjectKeys => Some("keys"),
        Builtin::ObjectValues => Some("values"),
        Builtin::ObjectEntries => Some("entries"),
        Builtin::ObjectIs => Some("is"),
        Builtin::ObjectAssign => Some("assign"),
        Builtin::ObjectFromEntries => Some("fromEntries"),
        Builtin::ObjectGroupBy => Some("groupBy"),
        Builtin::ObjectDefineProperty => Some("defineProperty"),
        Builtin::ObjectGetOwnPropertyNames => Some("getOwnPropertyNames"),
        Builtin::ObjectGetOwnPropertySymbols => Some("getOwnPropertySymbols"),
        Builtin::WeakRefDeref => Some("deref"),
        Builtin::ProxyRevocable => Some("revocable"),
        Builtin::ProxyRevoke => Some("revoke"),
        _ => None,
    }
}
