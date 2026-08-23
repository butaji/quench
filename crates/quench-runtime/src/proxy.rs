use crate::{
    execute::VmError,
    ops::{Builtin, FunctionKind},
    value::{ProxyValue, Value},
};
use std::rc::Rc;
use std::slice;
include!("proxy_set.rs");
include!("proxy_ops.rs");
pub fn builtin(builtin: Builtin, arguments: &[Value]) -> Result<Value, VmError> {
    match builtin {
        // Proxy is a constructor only; invoking it without `new` must throw.
        Builtin::Proxy => Err(crate::value::error::throw_type_error(
            "Proxy constructor must be called with new",
        )),
        Builtin::ProxyRevocable => proxy_revocable(arguments),
        Builtin::ReflectGet => reflect_get(arguments),
        Builtin::ReflectSet => reflect_set(arguments),
        Builtin::ReflectHas => reflect_has(arguments),
        Builtin::ReflectDeleteProperty => reflect_delete_property(arguments),
        Builtin::ReflectGetPrototypeOf => reflect_get_prototype_of(arguments),
        Builtin::ReflectSetPrototypeOf => reflect_set_prototype_of(arguments),
        Builtin::ReflectIsExtensible => reflect_is_extensible(arguments),
        Builtin::ReflectPreventExtensions => reflect_prevent_extensions(arguments),
        Builtin::ReflectGetOwnPropertyDescriptor => reflect_get_own_property_descriptor(arguments),
        Builtin::ReflectDefineProperty => reflect_define_property(arguments),
        Builtin::ReflectOwnKeys => reflect_own_keys(arguments),
        Builtin::ReflectApply => reflect_apply(arguments),
        Builtin::ReflectConstruct => reflect_construct(arguments),
        _ => Err(VmError::NotCallable),
    }
}

include!("proxy_reflect.rs");
