fn define_builtin(builtin: Builtin, arguments: &[Value]) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::ObjectDefineProperty => Some(crate::builtins::define_property(arguments)),
        Builtin::ObjectDefineProperties => Some(crate::builtins::define_properties(arguments)),
        _ => None,
    }
}

fn stateful_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::GeneratorNext => Some(crate::generator::next(receiver, arguments)),
        Builtin::GeneratorReturn => Some(crate::generator::return_(receiver, arguments)),
        Builtin::GeneratorThrow => Some(crate::generator::throw(receiver, arguments)),
        Builtin::ProxyRevoke => Some(crate::proxy::revoke(receiver)),
        Builtin::Math => Some(Err(not_callable())),
        _ => None,
    }
}

fn is_object_special(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::ObjectHasOwnProperty
            | Builtin::ObjectHasOwn
            | Builtin::ObjectGetOwnPropertyDescriptor
            | Builtin::ObjectGetOwnPropertyNames
            | Builtin::ObjectGetOwnPropertySymbols
            | Builtin::ObjectKeys
            | Builtin::ObjectValues
            | Builtin::ObjectEntries
            | Builtin::ObjectAssign
            | Builtin::ObjectFromEntries
            | Builtin::ObjectGroupBy
            | Builtin::ObjectCreate
            | Builtin::ObjectSetPrototypeOf
            | Builtin::ObjectPropertyIsEnumerable
            | Builtin::ObjectPrototypeIsPrototypeOf
            | Builtin::ObjectPrototypeDefineGetter
            | Builtin::ObjectPrototypeDefineSetter
            | Builtin::ObjectPrototypeLookupGetter
            | Builtin::ObjectPrototypeLookupSetter
    )
}

include!("vm_host.rs");
include!("vm_boolean_value.rs");
include!("vm_builtins.rs");
include!("vm_properties.rs");
include!("vm_dispatch.rs");
