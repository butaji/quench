pub fn copy_register(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    src: u16,
) -> Result<(), VmError> {
    registers
        .copy(usize::from(dst), usize::from(src))
        .then_some(())
        .ok_or(VmError::MissingReturn)
}
#[inline]
pub fn write_value(registers: &mut crate::register_file::RegisterFile, index: u16, value: Value) {
    let index = usize::from(index);
    registers.write(index, value);
}
#[inline]
pub fn read_register(
    registers: &crate::register_file::RegisterFile,
    index: u16,
) -> Result<Value, VmError> {
    registers
        .read(usize::from(index))
        .map(crate::locals::resolved_replacement)
        .ok_or(VmError::MissingReturn)
}

/// Unchecked variant for hot arithmetic paths where the compiler already
/// guarantees the register index is in bounds.
#[inline]
pub(crate) fn read_register_unchecked(
    registers: &crate::register_file::RegisterFile,
    index: u16,
) -> Value {
    let value = registers
        .read(usize::from(index))
        .expect("register index out of bounds");
    crate::locals::resolved_replacement(value)
}
pub fn get_property(value: &Value, key: &str) -> Value {
    // Deferred module namespaces are the only values for which export lookup
    // can have an effect. Avoid entering the module-binding machinery for the
    // overwhelmingly common primitive/builtin/array property reads.
    if matches!(value, Value::Object(_) | Value::BindingCell(_)) {
        crate::module_bindings::exports(value, key).ok();
    }
    if let Value::BindingCell(cell) = value {
        return get_property(&cell.borrow(), key);
    }
    if let Value::WeakFunction(function) = value {
        return get_property(&function.value(), key);
    }
    if matches!(value, Value::Proxy(_)) {
        return crate::proxy::proxy_get(value, key, Some(value)).unwrap_or(Value::Undefined);
    }
    // Property lookup must observe the latest physical object for this
    // semantic identity. Resolving only the result reads stale scalar fields
    // after an immutable object transition (for example `this.x++`).
    let owner = if crate::vm::is_global_declaration_batch_active()
        && crate::vm::is_global_object(value)
    {
        crate::vm::current_global_object()
    } else if matches!(value, Value::Object(view) if view.iter().any(|(name, _)| name == crate::vm::SCRIPT_GLOBAL_VIEW))
        || matches!(value, Value::ObjectAlias(alias) if alias
            .target()
            .is_some_and(|view| view.iter().any(|(name, _)| name == crate::vm::SCRIPT_GLOBAL_VIEW)))
    {
        crate::vm::current_global_object()
    } else {
        match value {
            Value::Array(_)
            | Value::Object(_)
            | Value::ObjectAlias(_)
            | Value::Function(_)
            | Value::BindingCell(_) => crate::locals::resolved_replacement(value.clone()),
            _ => value.clone(),
        }
    };
    let result = direct_or_primitive_property(&owner, key);
    // Primitive results cannot participate in replacement aliases. Avoid the
    // thread-local replacement lookup on the very common scalar property path.
    match result {
        Value::Array(ref array) if array.prototype().is_some() || array.property("\0prototype").is_some() => result,
        Value::Array(_)
        | Value::Object(_)
        | Value::ObjectAlias(_)
        | Value::Function(_)
        | Value::BindingCell(_) => crate::locals::resolved_replacement(result),
        Value::WeakFunction(_) => result.strong_function(),
        _ => result,
    }
}

    fn direct_or_primitive_property(value: &Value, key: &str) -> Value {
    if key == "stack"
        && matches!(
            crate::execute::get_property(value, crate::builtins::ERROR_SLOT),
            Value::Boolean(true)
        )
        && !matches!(
            crate::execute::get_property(value, "\0quench:stack_decorated"),
            Value::Boolean(true)
        )
        && !crate::execute::has_own_property(value, "code")
        && matches!(
            get_property_value(value, key),
            Value::String(ref stack) if !stack.is_empty() && !stack.contains('\n')
        )
    {
        return crate::vm::execute_builtin_with_receiver(
            crate::ops::Builtin::ErrorPrototypeStackGetter,
            &[],
            Some(value),
        )
        .unwrap_or(Value::Undefined);
    }
    // The unchecked property API is used by host validators as well as VM
    // code.  Accessor-backed own properties still need their observable
    // getter semantics here; otherwise a descriptor silently degrades to its
    // placeholder value and host argument validation sees `undefined`.
    if matches!(value, Value::Object(_))
        && !crate::vm::is_global_object(value)
        && crate::property_define::accessor(value, key, "get").is_some()
    {
        return crate::vm::get_property_with_receiver(value, key, value)
            .unwrap_or(Value::Undefined);
    }
    // An own typed-array property containing `undefined` still shadows its
    // prototype. Keep presence separate from the value so the ordinary
    // prototype fallback cannot resurrect an inherited member.
    if let Some(found) = crate::typed_array_prototype::own_property(value, key) {
        return found;
    }
    let direct = get_property_value(value, key);
    if !matches!(direct, Value::Undefined) {
        return direct;
    }
    if value.typed_array_meta().is_some() {
        // Integer-indexed exotic objects consume every canonical numeric key,
        // including out-of-bounds indices.  An OOB read therefore produces
        // `undefined` without consulting the prototype chain (which keeps a
        // user-installed numeric prototype property from leaking through).
        if crate::typed_array_ops::typed_array_index(key).is_some()
            || crate::typed_array_ops::canonical_numeric_index(key)
        {
            return Value::Undefined;
        }
        if let Some(prototype) = crate::typed_array_prototype::get(value) {
            let inherited = get_property(&prototype, key);
            if !matches!(inherited, Value::Undefined) {
                return inherited;
            }
            if let Ok(inherited) = crate::vm::get_property_with_receiver(&prototype, key, value) {
                if !matches!(inherited, Value::Undefined) {
                    return inherited;
                }
            }
        }
    }
    primitive_prototype_property(value, key)
}
fn get_property_value(value: &Value, key: &str) -> Value {
    use Value::*;
    match value {
        Builtin(builtin) if crate::intl::tolocale::symbol::name(*builtin).is_some() => {
            bind_callable_property(value, crate::ops::Builtin::SymbolPrototype, key)
        }
        Builtin(builtin) => bind_callable_property(value, *builtin, key),
        Array(values) => {
            let property = crate::arrays::property(values, key);
            let property = if matches!(property, Value::Undefined) {
                values
                    .prototype()
                    .map(|prototype| get_property(&prototype, key))
                    .or_else(|| {
                        (!values.is_arguments()).then(|| {
                            get_property(&Value::Builtin(crate::ops::Builtin::ArrayPrototype), key)
                        })
                    })
                    .unwrap_or(property)
            } else {
                property
            };
            if key == "toLocaleString" {
                crate::vm::bind_receiver_property(property, value)
            } else {
                property
            }
        }
        ArrayBuffer(buffer) => array_buffer_property(buffer, key),
        _ => get_property_value_typed_tail(value, key),
    }
}

fn get_property_value_typed_tail(value: &Value, key: &str) -> Value {
    use Value::*;
    if key == "constructor" && !value.is_float16_array() {
        if let Some(result) = crate::typed_array_prototype::constructor_override(value) {
            return result;
        }
    }
    match value {
        Float64Array(view) => float64_array_property(view, key),
        Float32Array(view) => float32_array_property(view, key),
        Int8Array(view) => int8_array_property(view, key),
        Int16Array(view) => int16_array_property(view, key),
        Uint16Array(view) => uint16_array_property(view, key),
        Int32Array(view) => int32_array_property(view, key),
        Uint8Array(view) => uint8_array_property(view, key),
        Uint32Array(view) => uint32_array_property(view, key),
        Uint8ClampedArray(view) => uint8_clamped_array_property(view, key),
        BigInt64Array(_) | BigUint64Array(_) => vm_typed_bigint::property(value, key),
        DataView(view) => data_view_property(view, key),
        _ => get_property_value_tail(value, key),
    }
}

fn get_property_value_tail(value: &Value, key: &str) -> Value {
    use Value::*;
    match value {
        Object(properties) => object_property(properties, value, key),
        ObjectAlias(alias) => object_alias_property(alias, key),
        String(value) if crate::conversion::is_symbol_string(value) => Value::Undefined,
        String(value) => string_property(value, key),
        StringUnits(units) => string_units_property(units, key),
        Number(value) => number_property(*value, key),
        Boolean(value) => boolean_property(*value, key),
        _ => get_property_value_object_tail(value, key),
    }
}

fn get_property_value_object_tail(value: &Value, key: &str) -> Value {
    use Value::*;
    match value {
        Function(function) if matches!(key, "apply" | "call" | "bind") => {
            function_prototype_method(key).unwrap_or_else(|| function_property(function, key))
        }
        Function(function) => function_property(function, key),
        BoundFunction(bound) => bound_function_property(value, bound, key),
        _ => get_property_value_collection_tail(value, key),
    }
}

fn function_prototype_method(key: &str) -> Option<Value> {
    Some(Value::Builtin(match key {
        "apply" => Builtin::FunctionApply,
        "call" => Builtin::FunctionCall,
        "bind" => Builtin::FunctionBind,
        _ => return None,
    }))
}

fn get_property_value_collection_tail(value: &Value, key: &str) -> Value {
    use Value::*;
    match value {
        Map(data) => map_property(data, key),
        Set(data) => set_property(data, key),
        Iterator(_) => iterator_property(value, key),
        Generator(_) => generator_property(value, key),
        _ => get_property_value_async_tail(value, key),
    }
}

fn set_property(data: &crate::value::SetData, key: &str) -> Value {
    if key == "Symbol.toStringTag" {
        let prototype = if data.weak {
            Builtin::WeakSetPrototype
        } else {
            Builtin::SetPrototype
        };
        return intrinsic_override_property(prototype, key, &Value::Builtin(prototype))
            .unwrap_or_else(|| crate::builtins::property(prototype, key));
    }
    let own = data.property(key);
    if !matches!(own, Value::Undefined) {
        return own;
    }
    if key == "constructor" {
        return Value::Builtin(if data.weak {
            Builtin::WeakSet
        } else {
            Builtin::Set
        });
    }
    if key == "size" && !data.weak {
        return Value::Number(data.values.borrow().len() as f64);
    }
    if data.weak {
        crate::collections::set::weak_property(key)
    } else {
        crate::collections::set::property(key)
    }
}

fn get_property_value_async_tail(value: &Value, key: &str) -> Value {
    match value {
        Promise(promise) => promise_value_property(promise, value, key),
        HostCapability(capability) => host_capability_property(value, capability.descriptor, key),
        _ => Value::Undefined,
    }
}

fn iterator_property(value: &Value, key: &str) -> Value {
    if let Value::Iterator(data) = value {
        if let Some(own) = data.property(key) {
            return own;
        }
    }
    if key == "Symbol.toStringTag" {
        let Value::Iterator(data) = value else {
            return Value::Undefined;
        };
        let prototype = crate::collections::iterator::builtin_for(data);
        let tag = intrinsic_override_property(prototype, key, value)
            .unwrap_or_else(|| crate::builtins::property(prototype, key));
        if !matches!(tag, Value::Undefined) {
            return tag;
        }
        return get_property(&Value::Builtin(Builtin::IteratorPrototype), key);
    }
    if key == "constructor" {
        return Value::Builtin(
            match crate::collections::iterator::builtin_for(match value {
                Value::Iterator(data) => data,
                _ => unreachable!(),
            }) {
                Builtin::MapIteratorPrototype => Builtin::Map,
                Builtin::SetIteratorPrototype => Builtin::Set,
                _ => Builtin::Object,
            },
        );
    }
    let property = crate::collections::iterator::property_for(value, key);
    iterator_property_value(value, property)
}

fn iterator_property_value(value: &Value, property: Value) -> Value {
    // Array iterators inherit `next` from ArrayIterator.prototype. Keep that
    // prototype edge observable: user code may replace it before a constructor
    // consumes the iterator. Protocol iterators carry their own `next` method
    // and therefore do not consult this override.
    if matches!(
        value,
        Value::Iterator(data)
            if matches!(&*data.state.borrow(), crate::value::IteratorState::Native { .. })
    ) {
        if let Some(override_value) =
            crate::vm::intrinsic_override_property(Builtin::ArrayIteratorPrototype, "next", value)
        {
            return crate::vm::bind_method(value, override_value);
        }
    }
    if matches!(
        property,
        Value::Builtin(
            Builtin::IteratorNext
                | Builtin::RegExpStringIteratorNext
                | Builtin::StringIteratorNext
                | Builtin::SetIteratorNext
                | Builtin::MapIteratorNext
        )
    ) {
        return property;
    }
    bind_method(value, property)
}

pub(crate) fn intrinsic_override_property(
    builtin: Builtin,
    key: &str,
    receiver: &Value,
) -> Option<Value> {
    let descriptor = crate::builtins::read_intrinsic_override(builtin, key)?;
    if let Some(value) = crate::builtins::read_descriptor_value(builtin, key) {
        return Some(value);
    }
    let Value::Object(fields) = descriptor else {
        return Some(Value::Undefined);
    };
    let getter = fields
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "get").then_some(value.clone()))?;
    if matches!(getter, Value::Undefined) {
        return Some(Value::Undefined);
    }
    let result = match getter {
        Value::Builtin(builtin) => {
            crate::vm::execute_builtin_with_receiver(builtin, &[], Some(receiver))
        }
        getter => crate::functions::execute_target(&getter, receiver, &[]),
    };
    Some(result.unwrap_or(Value::Undefined))
}

fn promise_value_property(promise: &crate::value::PromiseData, value: &Value, key: &str) -> Value {
    if key == "Symbol.toStringTag" {
        return intrinsic_override_property(Builtin::PromisePrototype, key, value)
            .unwrap_or_else(|| crate::builtins::property(Builtin::PromisePrototype, key));
    }
    let result =
        promise_property_value(promise, key).unwrap_or_else(|| promise_property(value, key));
    result
}

fn promise_property_value(promise: &crate::value::PromiseData, key: &str) -> Option<Value> {
    if let Some(value) = promise.property(key) {
        return Some(value);
    }
    let prototype = promise.prototype()?;
    let value = get_property(&prototype, key);
    (!matches!(value, Value::Undefined)).then_some(value)
}
fn map_property(data: &crate::value::MapData, key: &str) -> Value {
    if key == "constructor" {
        return map_constructor(data.weak);
    }
    map_collection_property(data, key)
}

fn map_constructor(weak: bool) -> Value {
    Value::Builtin(if weak { Builtin::WeakMap } else { Builtin::Map })
}

fn map_collection_property(data: &crate::value::MapData, key: &str) -> Value {
    if key == "Symbol.toStringTag" {
        let prototype = if data.weak {
            Builtin::WeakMapPrototype
        } else {
            Builtin::MapPrototype
        };
        return intrinsic_override_property(prototype, key, &Value::Builtin(prototype))
            .unwrap_or_else(|| crate::builtins::property(prototype, key));
    }
    let own = data.property(key);
    if !matches!(own, Value::Undefined) {
        return own;
    }
    if key == "size" && !data.weak {
        return Value::Number(data.keys.borrow().len() as f64);
    }
    if data.weak {
        crate::collections::map::weak_property(key)
    } else {
        crate::collections::map::property(key)
    }
}
/// Look up a property on the boxed prototype for a primitive value.
///
/// Returns the result when the prototype or its chain owns `key`; otherwise
/// `Undefined`. Used to surface `constructor` and prototype methods for
/// primitive values like `1`, `true`, and `null`-shaped access.
fn primitive_prototype_property(value: &Value, key: &str) -> Value {
    let Some(prototype) = primitive_prototype(value) else {
        return Value::Undefined;
    };
    if key == "constructor" {
        if let Some(builtin) = primitive_constructor(value) {
            return Value::Builtin(builtin);
        }
    }
    get_property(&prototype, key)
}

fn primitive_prototype(value: &Value) -> Option<Value> {
    let builtin = match value {
        Value::Number(_) => Builtin::NumberPrototype,
        Value::Boolean(_) => Builtin::BooleanPrototype,
        Value::StringUnits(_) => Builtin::StringPrototype,
        Value::String(value) if !crate::conversion::is_symbol_string(value) => {
            Builtin::StringPrototype
        }
        Value::String(value) if crate::conversion::is_symbol_string(value) => {
            Builtin::SymbolPrototype
        }
        Value::BigInt(_) => Builtin::BigIntPrototype,
        _ => return None,
    };
    Some(realm_prototype(builtin))
}

fn realm_prototype(builtin: Builtin) -> Value {
    crate::vm::current_realm_intrinsic(builtin).unwrap_or(Value::Builtin(builtin))
}

fn primitive_constructor(value: &Value) -> Option<Builtin> {
    match value {
        Value::Number(_) => Some(Builtin::Number),
        Value::Boolean(_) => Some(Builtin::Boolean),
        Value::String(value) if !crate::conversion::is_symbol_string(value) => {
            Some(Builtin::String)
        }
        Value::String(value) if crate::conversion::is_symbol_string(value) => Some(Builtin::Symbol),
        Value::BigInt(_) => Some(Builtin::BigInt),
        _ => None,
    }
}

fn generator_property(value: &Value, key: &str) -> Value {
    let is_async = matches!(value, Value::Generator(generator) if generator.function.is_async);
    if key == "Symbol.toStringTag" {
        let prototype =
            crate::builtins::object::get_prototype_of(Some(value)).unwrap_or_else(|_| {
                if is_async {
                    crate::builtins::async_generator_prototype()
                } else {
                    crate::builtins::generator_prototype()
                }
            });
        if let Ok(Value::Object(fields)) =
            crate::builtins::object::descriptor(Some(&prototype), Some(&Value::String(key.into())))
        {
            if let Some(getter) = fields
                .iter()
                .find_map(|(name, value)| (name == "get").then_some(value.clone()))
            {
                return match getter {
                    Value::Builtin(builtin) => {
                        crate::vm::execute_builtin_with_receiver(builtin, &[], Some(value))
                            .unwrap_or(Value::Undefined)
                    }
                    getter => crate::functions::execute_target(&getter, value, &[])
                        .unwrap_or(Value::Undefined),
                };
            }
        }
        return crate::execute::get_property_result(&prototype, key).unwrap_or_else(|_| {
            Value::String(
                if is_async {
                    "AsyncGenerator"
                } else {
                    "Generator"
                }
                .into(),
            )
        });
    }
    let builtin = match key {
        "next" => {
            if is_async {
                crate::ops::Builtin::AsyncGeneratorNext
            } else {
                crate::ops::Builtin::GeneratorNext
            }
        }
        "return" => {
            if is_async {
                crate::ops::Builtin::AsyncGeneratorReturn
            } else {
                crate::ops::Builtin::GeneratorReturn
            }
        }
        "throw" => {
            if is_async {
                crate::ops::Builtin::AsyncGeneratorThrow
            } else {
                crate::ops::Builtin::GeneratorThrow
            }
        }
        "toArray" => crate::ops::Builtin::IteratorToArray,
        "map" => crate::ops::Builtin::IteratorMap,
        "filter" => crate::ops::Builtin::IteratorFilter,
        "flatMap" => crate::ops::Builtin::IteratorFlatMap,
        "drop" => crate::ops::Builtin::IteratorDrop,
        "take" => crate::ops::Builtin::IteratorTake,
        "reduce" => crate::ops::Builtin::IteratorReduce,
        "find" => crate::ops::Builtin::IteratorFind,
        "forEach" => crate::ops::Builtin::IteratorForEach,
        "some" => crate::ops::Builtin::IteratorSome,
        "every" => crate::ops::Builtin::IteratorEvery,
        _ => {
            let prototype = if is_async {
                crate::builtins::async_generator_prototype()
            } else {
                crate::builtins::generator_prototype()
            };
            return crate::execute::get_property_result(&prototype, key)
                .unwrap_or(Value::Undefined);
        }
    };
    bind_method(value, Value::Builtin(builtin))
}

fn function_property(function: &crate::value::FunctionValue, key: &str) -> Value {
    let properties = function.properties.borrow();
    if let Some((_, value)) = properties.iter().rev().find(|(name, _)| name == key) {
        return property_value(value);
    }
    if matches!(key, "caller" | "arguments")
        && function.strictness == crate::ops::FunctionStrictness::Sloppy
        && matches!(function.kind, crate::ops::FunctionKind::Ordinary)
    {
        return Value::Undefined;
    }
    if key == "constructor" {
        return function_realm_intrinsic(function, function_constructor(function));
    }
    function_inherited_property(function, &properties, key)
}

fn function_realm_intrinsic(
    function: &crate::value::FunctionValue,
    builtin: crate::ops::Builtin,
) -> Value {
    crate::vm::realm::intrinsic(crate::construct::function_realm_id(function), builtin)
        .unwrap_or(Value::Builtin(builtin))
}

fn function_inherited_property(
    function: &crate::value::FunctionValue,
    properties: &[(String, Value)],
    key: &str,
) -> Value {
    if matches!(key, "prototype") {
        return Value::Undefined;
    }
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| {
            (name == "\0function_prototype" || name == "\0prototype").then(|| property_value(value))
        })
        .map_or_else(
            || function_prototype_property(function, key),
            |prototype| inherited_function_property(function, prototype, key),
        )
}

fn inherited_function_property(
    function: &crate::value::FunctionValue,
    prototype: Value,
    key: &str,
) -> Value {
    let inherited = get_property(&prototype, key);
    let Value::BoundFunction(bound) = &inherited else {
        return inherited;
    };
    if let Value::Builtin(builtin) = bound.target {
        return bind_method(
            &Value::Function(std::rc::Rc::new(function.clone())),
            Value::Builtin(builtin),
        );
    }
    if !matches!(prototype, Value::Builtin(Builtin::Promise))
        || !matches!(
            bound.target,
            Value::Builtin(
                Builtin::PromiseResolve
                    | Builtin::PromiseReject
                    | Builtin::PromiseAll
                    | Builtin::PromiseAllKeyed
                    | Builtin::PromiseAllSettled
                    | Builtin::PromiseAny
                    | Builtin::PromiseRace
                    | Builtin::PromiseWithResolvers
                    | Builtin::PromiseTry
            )
        )
    {
        return inherited;
    }
    bind_method(
        &Value::Function(std::rc::Rc::new(function.clone())),
        bound.target.clone(),
    )
}
fn function_prototype_property(function: &crate::value::FunctionValue, key: &str) -> Value {
    let builtin = if function.is_async {
        Builtin::AsyncFunctionPrototype
    } else {
        Builtin::FunctionPrototype
    };
    function_prototype_property_for_builtin(builtin, key)
}

pub(super) fn function_prototype_property_for_builtin(builtin: Builtin, key: &str) -> Value {
    let value = builtin_property(builtin, key);
    value_or_object_prototype(value, key)
}

fn value_or_object_prototype(value: Value, key: &str) -> Value {
    if matches!(value, Value::Undefined) {
        return builtin_property(Builtin::ObjectPrototype, key);
    }
    value
}

fn property_value(value: &Value) -> Value {
    match value {
        Value::BindingCell(cell) => property_value_owned(cell.load()),
        Value::WeakFunction(function) => function.value(),
        value => value.clone(),
    }
}

fn property_value_owned(value: Value) -> Value {
    match value {
        Value::BindingCell(cell) => property_value_owned(cell.load()),
        Value::WeakFunction(function) => function.value(),
        value => value,
    }
}

fn function_constructor(function: &crate::value::FunctionValue) -> Builtin {
    match (function.kind, function.is_async) {
        (crate::ops::FunctionKind::Generator, true) => Builtin::AsyncGeneratorFunction,
        (crate::ops::FunctionKind::Generator, false) => Builtin::GeneratorFunction,
        (_, true) => Builtin::AsyncFunction,
        (_, false) => Builtin::Function,
    }
}
include!("vm_properties_resolution.rs");
include!("vm_properties_special.rs");
use crate::value::Value::{HostCapability, Promise};
