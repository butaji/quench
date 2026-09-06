use crate::{
    facts::ProgramDb,
    ops::Op,
    value::{ObjectData, PropertyEntries, Value},
};
use std::{collections::HashMap, rc::Rc};
pub(crate) fn reduce(
    expression: &oxc::ast::ast::NewExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let callee =
        crate::reduce::reduce_expression(&expression.callee, ops, facts, next_register, locals)?;
    let (args, spreads) = crate::reduce::reduce_expressions::calls_reduce::reduce_arguments(
        &expression.arguments,
        ops,
        facts,
        next_register,
        locals,
    )?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Construct {
        dst,
        callee,
        args,
        spreads,
    });
    Some(dst)
}
pub(crate) fn execute(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<(), crate::execute::VmError> {
    let Op::Construct {
        dst,
        callee,
        args,
        spreads,
    } = op
    else {
        return Err(crate::execute::VmError::NotCallable);
    };
    let arguments = collect_construct_arguments(registers, args, spreads)?;
    let target = crate::execute::read_register(registers, *callee)?;
    let value = match &target {
        // Intrinsic builtins are implemented directly; do not enter the VM
        // environment trampoline used by user-defined Function constructors.
        Value::Builtin(builtin) => {
            construct_builtin_target(*builtin, &target, &target, &arguments)?
        }
        _ => construct_value(&target, &arguments)?,
    };
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}
fn collect_construct_arguments(
    registers: &crate::register_file::RegisterFile,
    args: &[u16],
    spreads: &[bool],
) -> Result<Vec<Value>, crate::execute::VmError> {
    let mut arguments = Vec::new();
    for (index, spread) in args.iter().zip(spreads) {
        let value = crate::execute::read_register(registers, *index)?;
        if *spread {
            arguments.extend(crate::collections::iterator::collect_iterable(value)?);
        } else {
            arguments.push(value);
        }
    }
    Ok(arguments)
}
pub(crate) fn construct_value(
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    construct_with_new_target(target, target, arguments)
}
pub(crate) fn construct_value_with_new_target(
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    construct_with_new_target(target, new_target, arguments)
}
pub(crate) fn construct_super(
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    construct_with_new_target(target, new_target, arguments)
}
fn construct_with_new_target(
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let target = peel_construct_value(target);
    let new_target = peel_construct_value(new_target);
    match &target {
        Value::HostCapability(capability) => {
            let value = crate::vm::current_context_or_default()
                .construct_host_with_new_target(
                    crate::ops::HostCapabilityRef {
                        realm: capability.realm(),
                        kind: capability.descriptor.kind,
                    },
                    arguments,
                    &new_target,
                )
                .unwrap_or(Err(crate::vm::not_callable()))?;
            with_new_target_prototype(value, &target, &new_target)
        }
        Value::Builtin(builtin) => {
            if *builtin == crate::ops::Builtin::Uint16Array && is_float16_new_target(&new_target) {
                return construct_float16_array(arguments);
            }
            construct_builtin_target(*builtin, &target, &new_target, arguments)
        }
        Value::Function(function) => construct_function(function, &new_target, arguments),
        Value::BoundFunction(bound) => construct_bound(bound, &target, &new_target, arguments),
        Value::Proxy(_) => crate::proxy::proxy_construct(&target, arguments, Some(&new_target)),
        _ => Err(crate::vm::not_callable()),
    }
}

fn is_float16_new_target(value: &Value) -> bool {
    let prototype = crate::execute::get_property(value, "prototype");
    matches!(
        crate::execute::get_property(&prototype, "\0float16_constructor"),
        Value::Boolean(true)
    )
}

fn construct_builtin_target(
    builtin: crate::ops::Builtin,
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if builtin == crate::ops::Builtin::SharedArrayBuffer
        && !crate::builtins::same_value(Some(target), Some(new_target))
    {
        return construct_shared_array_buffer_with_target(builtin, target, new_target, arguments);
    }
    let needs_new_target = !crate::builtins::same_value(Some(target), Some(new_target));
    if builtin == crate::ops::Builtin::Object && needs_new_target {
        let prototype = crate::execute::get_property_result(new_target, "prototype")?;
        let prototype = if crate::value::is_object(&prototype) {
            prototype
        } else {
            crate::vm::realm_intrinsic_for(
                constructor_realm(new_target),
                crate::ops::Builtin::ObjectPrototype,
            )
        };
        return Ok(crate::value::Value::Object(std::rc::Rc::new(
            crate::value::ObjectData::new(vec![("\0prototype".into(), prototype)]),
        )));
    }
    if builtin == crate::ops::Builtin::Iterator && !needs_new_target {
        return Err(crate::value::error::throw_type_error(
            "Iterator is not constructable",
        ));
    }
    // Promise checks that its executor is callable before GetPrototypeFromConstructor.
    // Fetching the prototype first would incorrectly expose a prototype getter error
    // when the executor is not callable.
    if builtin == crate::ops::Builtin::Promise && needs_new_target {
        let value = construct_builtin_in_realm(builtin, arguments, new_target)?;
        let prototype = crate::execute::get_property_result(new_target, "prototype")?;
        let value = apply_new_target_prototype(value, target, new_target, prototype)?;
        validate_data_view(&value)?;
        return Ok(value);
    }
    if builtin == crate::ops::Builtin::DataView && needs_new_target {
        let value = construct_data_view(arguments)?;
        let prototype = crate::execute::get_property_result(new_target, "prototype")?;
        let value = apply_new_target_prototype(value, target, new_target, prototype)?;
        let value = value;
        validate_data_view(&value)?;
        return Ok(value);
    }
    if builtin == crate::ops::Builtin::ArrayBuffer && needs_new_target {
        validate_array_buffer_limits(arguments)?;
        let prototype = crate::execute::get_property_result(new_target, "prototype")?;
        let value = construct_builtin_in_realm(builtin, arguments, new_target)?;
        return apply_new_target_prototype(value, target, new_target, prototype);
    }
    if is_typed_array_constructor_builtin(builtin) && needs_new_target {
        let value = construct_builtin_in_realm(builtin, arguments, new_target)?;
        let prototype = crate::execute::get_property_result(new_target, "prototype")?;
        return apply_new_target_prototype(value, target, new_target, prototype);
    }
    let prototype = needs_new_target
        .then(|| crate::execute::get_property_result(new_target, "prototype"))
        .transpose()?;
    let value = if is_dynamic_function_constructor(builtin) {
        construct_dynamic_builtin_in_realm(builtin, arguments, target)?
    } else {
        construct_builtin_in_realm(builtin, arguments, new_target)?
    };
    let value = if let Some(prototype) = prototype {
        apply_new_target_prototype(value, target, new_target, prototype)?
    } else {
        with_new_target_prototype(value, target, new_target)?
    };
    validate_data_view(&value)?;
    Ok(value)
}

fn is_typed_array_constructor_builtin(builtin: crate::ops::Builtin) -> bool {
    matches!(
        builtin,
        crate::ops::Builtin::Float64Array
            | crate::ops::Builtin::Float32Array
            | crate::ops::Builtin::Int8Array
            | crate::ops::Builtin::Int16Array
            | crate::ops::Builtin::Int32Array
            | crate::ops::Builtin::Uint8Array
            | crate::ops::Builtin::Uint8ClampedArray
            | crate::ops::Builtin::Uint16Array
            | crate::ops::Builtin::Uint32Array
            | crate::ops::Builtin::BigInt64Array
            | crate::ops::Builtin::BigUint64Array
    )
}

fn is_dynamic_function_constructor(builtin: crate::ops::Builtin) -> bool {
    matches!(
        builtin,
        crate::ops::Builtin::Function
            | crate::ops::Builtin::AsyncFunction
            | crate::ops::Builtin::GeneratorFunction
            | crate::ops::Builtin::AsyncGeneratorFunction
    )
}

fn construct_dynamic_builtin_in_realm(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    constructor: &Value,
) -> Result<Value, crate::execute::VmError> {
    let realm = Some(constructor_realm(constructor));
    match crate::functions_dynamic::construct_builtin_in_realm(builtin, arguments, realm) {
        Some(result) => result,
        None => Err(crate::vm::not_callable()),
    }
}

fn validate_array_buffer_limits(arguments: &[Value]) -> Result<(), crate::execute::VmError> {
    let length = arguments
        .first()
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    let length = to_index(length)?;
    let Some(options) = arguments
        .get(1)
        .filter(|value| crate::value::is_object(value))
    else {
        return Ok(());
    };
    let maximum = crate::execute::get_property_result(options, "maxByteLength")?;
    if matches!(maximum, Value::Undefined) {
        return Ok(());
    }
    let maximum = to_index(crate::conversion::to_number(&maximum)?)?;
    if maximum < length {
        return Err(crate::value::error::throw_range_error(
            "maxByteLength is smaller than byteLength",
        ));
    }
    Ok(())
}

fn construct_shared_array_buffer_with_target(
    builtin: crate::ops::Builtin,
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    validate_shared_array_buffer_length(arguments)?;
    let prototype = crate::execute::get_property_result(new_target, "prototype")?;
    let value = construct_builtin(builtin, arguments)?;
    let prototype = if crate::value::is_object(&prototype) {
        Some(prototype)
    } else {
        realm_default_prototype(target, new_target)
    };
    Ok(prototype.map_or(value.clone(), |prototype| {
        crate::builtins::set_property(value, "\0prototype", prototype)
    }))
}

fn validate_data_view(value: &Value) -> Result<(), crate::execute::VmError> {
    let Value::DataView(view) = value else {
        return Ok(());
    };
    if *view.buffer.detached.borrow() {
        return Err(type_error("Cannot use a detached ArrayBuffer"));
    }
    if view.is_out_of_bounds() {
        return Err(range_error("Invalid DataView byte length"));
    }
    Ok(())
}

fn validate_shared_array_buffer_length(arguments: &[Value]) -> Result<(), crate::execute::VmError> {
    let length = arguments
        .first()
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(0.0);
    let length = to_index(length)?;
    let Some(options) = arguments
        .get(1)
        .filter(|value| crate::value::is_object(value))
    else {
        return Ok(());
    };
    let maximum = crate::execute::get_property_result(options, "maxByteLength")?;
    if matches!(maximum, Value::Undefined) {
        return Ok(());
    }
    if to_index(crate::conversion::to_number(&maximum)?)? < length {
        return Err(range_error("maxByteLength is smaller than byteLength"));
    }
    Ok(())
}
fn with_new_target_prototype(
    value: Value,
    target: &Value,
    new_target: &Value,
) -> Result<Value, crate::execute::VmError> {
    let prototype = crate::execute::get_property_result(new_target, "prototype")?;
    apply_new_target_prototype(value, target, new_target, prototype)
}

fn apply_new_target_prototype(
    value: Value,
    target: &Value,
    new_target: &Value,
    prototype: Value,
) -> Result<Value, crate::execute::VmError> {
    let prototype = if crate::value::is_object(&prototype) {
        prototype
    } else {
        get_prototype_from_constructor(new_target, |realm| {
            realm_default_prototype(target, new_target).unwrap_or_else(|| {
                crate::vm::realm_intrinsic_for(realm, crate::ops::Builtin::ObjectPrototype)
            })
        })
    };
    // Setting an already-correct prototype is an observable no-op. Preserve
    // identity for host-owned constructor results (for example node:test
    // mock call records) instead of materializing a replacement object.
    if crate::builtins::same_value(
        crate::execute::get_prototype_of(&value).ok().as_ref(),
        Some(&prototype),
    ) {
        return Ok(value);
    }
    Ok({
        if let crate::value::Value::Array(_) = &value {
            return Ok(crate::builtins::set_property(
                value,
                "\0prototype",
                prototype,
            ));
        }
        let value = set_internal_prototype(value, prototype)?;
        drop_shadowed_error_constructor(value)
    })
}

fn set_internal_prototype(
    value: Value,
    prototype: Value,
) -> Result<Value, crate::execute::VmError> {
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        return crate::builtins::object::set_prototype_of(&[value, prototype]);
    }
    if matches!(
        value,
        Value::Function(_) | Value::BoundFunction(_) | Value::HostCapability(_)
    ) {
        return crate::builtins::object::set_prototype_of(&[value, prototype]);
    }
    Ok(crate::builtins::set_property(
        value,
        "\0prototype",
        prototype,
    ))
}

fn drop_shadowed_error_constructor(mut value: Value) -> Value {
    let Value::Object(data) = &mut value else {
        return value;
    };
    let data = std::rc::Rc::make_mut(data);
    let is_error = data
        .properties
        .iter()
        .any(|(key, _)| key == crate::builtins::ERROR_SLOT);
    if is_error {
        data.invalidate_layout();
        data.properties.retain_names(|key| key != "constructor");
    }
    value
}
include!("construct_realm.rs");
fn construct_bound(
    bound: &crate::value::BoundFunctionValue,
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let mut combined = bound.arguments.clone();
    combined.extend_from_slice(arguments);
    let new_target = bound_construct_new_target(target, new_target);
    let float16 = matches!(
        crate::execute::get_property_result(&bound.receiver, "\0float16_constructor"),
        Ok(Value::Boolean(true))
    );
    let mut value = if float16 {
        construct_float16_array(&combined)?
    } else {
        construct_bound_target(bound, &bound.target, &new_target, &combined)?
    };
    if float16 {
        value.mark_float16_array();
        if let Ok(prototype) = crate::execute::get_property_result(&bound.receiver, "\0prototype") {
            value = crate::execute::set_prototype_of(&value, &prototype)?;
        }
    }
    if let Value::HostCapability(capability) = &bound.receiver {
        // Host-created objects are owned by the host implementation. Adding
        // ordinary properties here goes through the aliasing setter, which
        // clones object data when the host retains an Rc and breaks JS object
        // identity. Host dispatch already carries the capability realm.
        if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
            // A host constructor's `newTarget` is normally the same bound
            // function as `target`.  The ordinary helper intentionally skips
            // that identity case, but host-created objects still need the
            // constructor's explicit prototype: it owns the observable
            // method surface (for example EventTarget's dispatch methods).
            let prototype_target = if matches!(
                new_target,
                Value::Builtin(crate::ops::Builtin::HostCapability(_))
            ) {
                // BoundFunction [[Construct]] replaces an identical
                // newTarget with its bound target.  Preserve the bound
                // function for GetPrototypeFromConstructor so custom host
                // constructors retain their declared prototype.
                Value::BoundFunction(std::rc::Rc::new(bound.clone()))
            } else {
                new_target.clone()
            };
            let prototype = crate::execute::get_property_result(&prototype_target, "prototype")?;
            // Host constructors may return an object whose prototype is owned
            // by the host implementation (for example fs.ReadStream).  A
            // bound capability has no explicit JavaScript `prototype`
            // property, so deriving the realm fallback here would replace
            // the host object's method surface with an empty object.  Keep
            // that existing prototype when no constructor prototype was
            // declared; constructors that expose one still take the normal
            // path below.
            if matches!(prototype, Value::Undefined)
                && matches!(
                    crate::execute::get_prototype_of(&value),
                    Ok(Value::Object(_) | Value::ObjectAlias(_) | Value::Null)
                )
            {
                return Ok(value);
            }
            return apply_new_target_prototype(value, target, &prototype_target, prototype);
        }
        let value = crate::builtins::set_property(
            value,
            "\0realm",
            Value::HostCapability(capability.clone()),
        );
        return Ok(crate::builtins::set_property(
            value,
            "\0creation_realm",
            Value::HostCapability(capability.clone()),
        ));
    }
    Ok(value)
}

fn construct_bound_target(
    bound: &crate::value::BoundFunctionValue,
    target: &Value,
    new_target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    match target {
        Value::Builtin(builtin) => {
            if let crate::ops::Builtin::HostCapability(kind) = builtin {
                let Value::HostCapability(capability) = &bound.receiver else {
                    return Err(crate::vm::not_callable());
                };
                return crate::vm::current_context_or_default()
                    .construct_host_with_new_target(
                        crate::ops::HostCapabilityRef {
                            realm: capability.realm(),
                            kind: *kind,
                        },
                        arguments,
                        new_target,
                    )
                    .unwrap_or(Err(crate::vm::not_callable()));
            }
            if crate::builtin_meta::constructor_name(*builtin).is_none() {
                return Err(crate::vm::not_callable());
            }
            let value = construct_bound_in_realm(bound, *builtin, arguments)?;
            with_new_target_prototype(value, &Value::Builtin(*builtin), new_target)
        }
        Value::Function(function) => construct_function(function, new_target, arguments),
        Value::BoundFunction(next) => construct_bound(
            next,
            &Value::BoundFunction(std::rc::Rc::clone(next)),
            new_target,
            arguments,
        ),
        _ => Err(crate::vm::not_callable()),
    }
}

fn construct_builtin(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if builtin == crate::ops::Builtin::TypedArray {
        return Err(crate::value::error::throw_type_error(
            "TypedArray is not directly constructible",
        ));
    }
    if crate::builtin_meta::is_error_constructor(builtin) {
        return construct_error(&builtin, arguments);
    }
    if let Some(result) = crate::functions_dynamic::construct_builtin(builtin, arguments) {
        return result;
    }
    if let Some(result) = construct_typed_builtin(builtin, arguments) {
        return result;
    }
    construct_builtin_match(builtin, arguments)
}

include!("construct_builtins.rs");

fn construct_builtin_in_realm(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    new_target: &Value,
) -> Result<Value, crate::execute::VmError> {
    let realm = Some(constructor_realm(new_target));
    if let Some(result) =
        crate::functions_dynamic::construct_builtin_in_realm(builtin, arguments, realm)
    {
        return result;
    }
    construct_builtin(builtin, arguments)
}
fn construct_function(
    function: &std::rc::Rc<crate::value::FunctionValue>,
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    if !crate::functions::is_constructible(function) {
        return Err(crate::vm::not_callable());
    }
    if is_default_derived_constructor(function) {
        let super_constructor = derived_constructor(function)?;
        let receiver = construct_with_new_target(&super_constructor, target, arguments)?;
        let receiver = initialize_instance_fields(function, receiver)?;
        // A default derived constructor must honor the active newTarget.  In
        // nested typed-array subclasses, using this function's prototype
        // loses the outer subclass constructor and breaks species creation.
        let prototype = get_prototype_from_constructor(target, |_| {
            crate::execute::get_property(&Value::Function(function.clone()), "prototype")
        });
        return set_internal_prototype(receiver, prototype);
    }
    if crate::functions::is_derived_constructor(function) {
        let _context = crate::super_scope::Guard::install(function, &Value::Undefined);
        let (result, final_this) =
            crate::functions::execute_construct(function, &Value::Undefined, target, arguments)?;
        return finish_derived_construct(result, final_this);
    }
    if function.instance_fields.borrow().is_empty() && !matches!(target, Value::Proxy(_)) {
        if let Some(object) = try_record_constructor(function, target, arguments) {
            return Ok(object);
        }
        if !function.code.facts().direct_constructor.is_empty() {
            let prototype = get_prototype_from_constructor(target, |realm| {
                crate::vm::realm_intrinsic_for(realm, crate::ops::Builtin::ObjectPrototype)
            });
            if let Some(object) =
                crate::functions::direct_constructor_object(function, prototype, arguments)
            {
                crate::execution_trace::kernel("direct_record_constructor", false);
                return Ok(object);
            }
        }
    }
    let receiver = constructor_receiver(target);
    let object = initialize_instance_fields(function, receiver)?;
    let (result, final_this) =
        crate::functions::execute_construct(function, &object, target, arguments)?;
    if crate::value::is_object(&result) {
        Ok(crate::locals::resolved_replacement(result))
    } else if crate::value::is_object(&final_this) {
        Ok(crate::locals::resolved_replacement(final_this))
    } else {
        Ok(crate::locals::resolved_replacement(object))
    }
}

include!("construct_record.rs");

fn finish_derived_construct(
    result: Value,
    final_this: Value,
) -> Result<Value, crate::execute::VmError> {
    if crate::value::is_object(&result) {
        return Ok(crate::locals::resolved_replacement(result));
    }
    if !matches!(result, Value::Undefined) {
        return Err(crate::value::error::throw_type_error(
            "Derived constructor returned a non-object value",
        ));
    }
    if crate::value::is_object(&final_this) {
        return Ok(crate::locals::resolved_replacement(final_this));
    }
    Err(crate::value::error::throw_reference_error(
        "Derived constructor did not initialize this",
    ))
}

pub(crate) fn initialize_instance_fields(
    function: &crate::value::FunctionValue,
    receiver: Value,
) -> Result<Value, crate::execute::VmError> {
    let receiver = crate::locals::resolved_replacement(receiver);
    let _home = crate::super_scope::Guard::install(&std::rc::Rc::new(function.clone()), &receiver);
    initialize_instance_fields_impl(function, receiver)
}
include!("construct_instance_fields.rs");
pub(crate) fn derived_constructor(
    function: &crate::value::FunctionValue,
) -> Result<Value, crate::execute::VmError> {
    // An ordinary function may have its function object prototype reassigned
    // (for example `Object.setPrototypeOf(F, ArrayBuffer)`) without becoming
    // a derived class constructor. The parser/IR records actual `extends`
    // semantics explicitly; only those functions participate in super
    // construction and the uninitialised-`this` check.
    if !crate::functions::is_derived_constructor(function) {
        return Err(crate::value::error::throw_reference_error(
            "super is unavailable",
        ));
    }
    let proto = crate::builtins::object::get_prototype_of(Some(&crate::value::Value::Function(
        std::rc::Rc::new(function.clone()),
    )))?;
    if !is_bare_function_prototype(&proto) {
        return Ok(proto);
    }
    function
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then(|| value.clone()))
        .ok_or_else(|| crate::value::error::throw_reference_error("super is unavailable"))
}

fn is_bare_function_prototype(value: &crate::value::Value) -> bool {
    matches!(
        value,
        crate::value::Value::Builtin(crate::ops::Builtin::FunctionPrototype)
    )
}

include!("construct_tail.rs");
