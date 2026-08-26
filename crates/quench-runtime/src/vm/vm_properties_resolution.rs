pub fn get_property_result(value: &Value, key: &str) -> Result<Value, VmError> {
    let value = crate::locals::resolved_replacement(value.clone());
    // A materialized global binding can transiently leave the receiver nullish
    // while lowering a member assignment.  Recover only when the requested
    // property is a real Math intrinsic; this preserves ordinary nullish
    // property errors and does not mask setters or depend on a register id.
    let value = if matches!(value, Value::Null | Value::Undefined)
        && (crate::math::property(key).is_some() || crate::math::constant(key).is_some())
    {
        crate::vm::realm_intrinsic(crate::ops::Builtin::Math)
    } else {
        value
    };
    let result = get_property_with_receiver(&value, key, &value)?;
    Ok(crate::locals::resolved_replacement(result).strong_function())
}

pub(crate) fn get_named_property_result(
    value: &Value,
    key: &str,
    cache: &std::cell::Cell<u64>,
) -> Result<Value, VmError> {
    let retry_resolved = !matches!(value, Value::Object(object) if !object.has_replacement());
    if let Value::Object(object) = value {
        if let Some(value) = get_named_cached_object(object, cache) {
            return Ok(value);
        }
    }
    let resolved = crate::locals::resolved_replacement(value.clone());
    if retry_resolved {
        if let Value::Object(object) = &resolved {
            if let Some(value) = get_named_cached_object(object, cache) {
                crate::execution_trace::named_get_miss_reason("unknown");
                return Ok(value);
            }
        }
    }
    crate::execution_trace::event(crate::execution_trace::Event::NamedPropertyMiss);
    crate::execution_trace::named_property_miss(key);
    let result = get_property_result(&resolved, key)?.strong_function();
    if let Value::Object(object) = &resolved {
        if let Some(slot) = cacheable_own_slot(&resolved, key) {
            cache.set(crate::machine::pack_named_cache(
                object.semantic_layout_id(),
                slot,
            ));
        } else if let Some(entry) = cacheable_immediate_prototype(object, key) {
            install_prototype_cache(cache, entry);
        }
    }
    Ok(result)
}

/// Resolve an already-proven object word without constructing an owning
/// `Value::Object`. A cache miss deliberately returns to complete semantics.
#[inline(always)]
pub(crate) fn get_named_cached_object(
    object: &crate::value::ObjectData,
    cache: &std::cell::Cell<u64>,
) -> Option<Value> {
    match get_named_cached_payload(object, cache)? {
        NamedCachedPayload::Word(word) => Some(unsafe { &*word }.load().strong_function()),
        NamedCachedPayload::Cell(cell) => Some(unsafe { &*cell }.load().strong_function()),
        NamedCachedPayload::Value(value) => Some(value.strong_function()),
    }
}

pub(crate) enum NamedCachedPayload {
    Word(*const crate::register_file::SlotWord),
    Cell(*const crate::value::BindingCell),
    Value(Value),
}

#[inline(always)]
pub(crate) fn get_named_cached_payload(
    object: &crate::value::ObjectData,
    cache: &std::cell::Cell<u64>,
) -> Option<NamedCachedPayload> {
    if object.has_replacement() {
        crate::execution_trace::event(crate::execution_trace::Event::NamedGetReplacement);
        crate::execution_trace::named_get_miss_reason("replacement");
        return None;
    }
    let layout = object.semantic_layout_id();
    let cached = cache.get();
    if cached & PROTOTYPE_CACHE_TAG != 0 {
        if let Some(payload) =
            prototype_cache_hit(object, layout, cached, cache as *const _ as usize)
        {
            crate::execution_trace::event(crate::execution_trace::Event::NamedPropertyHit);
            return Some(payload);
        }
        crate::execution_trace::event(crate::execution_trace::Event::NamedGetPrototypeMiss);
        crate::execution_trace::named_get_miss_reason("prototype");
        return None;
    }
    let Some((cached_layout, slot)) = crate::machine::unpack_named_cache(cached) else {
        crate::execution_trace::event(crate::execution_trace::Event::NamedGetCacheEmpty);
        crate::execution_trace::named_get_miss_reason("empty");
        return None;
    };
    if layout != cached_layout {
        crate::execution_trace::event(crate::execution_trace::Event::NamedGetLayoutMismatch);
        crate::execution_trace::named_get_miss_reason("layout");
        return None;
    }
    let Some(word) = object.hot_properties().slot_word(slot as usize) else {
        crate::execution_trace::event(crate::execution_trace::Event::NamedGetSlotMissing);
        crate::execution_trace::named_get_miss_reason("slot");
        return None;
    };
    word.trace_named_payload("own");
    crate::execution_trace::event(crate::execution_trace::Event::NamedPropertyHit);
    Some(NamedCachedPayload::Word(word))
}

#[inline(always)]
fn named_cached_payload(value: &Value) -> NamedCachedPayload {
    match value {
        Value::BindingCell(cell) => NamedCachedPayload::Cell(std::rc::Rc::as_ptr(cell)),
        value => NamedCachedPayload::Value(property_value(value)),
    }
}

/// Return a raw pointer to the canonical word cell on a guarded named hit.
/// The pointer remains owned by `object`; callers must keep that object alive
/// until the word has been copied.
#[inline(always)]
pub(crate) fn get_named_cached_cell(
    object: &crate::value::ObjectData,
    cache: &std::cell::Cell<u64>,
) -> Option<*const crate::value::BindingCell> {
    match get_named_cached_payload(object, cache)? {
        NamedCachedPayload::Word(_) => None,
        NamedCachedPayload::Cell(cell) => Some(cell),
        NamedCachedPayload::Value(_) => None,
    }
}

const PROTOTYPE_CACHE_TAG: u64 = 1 << 63;

#[derive(Clone)]
struct PrototypeLink {
    prototype_slot: u32,
    prototype: std::rc::Weak<crate::value::ObjectData>,
    prototype_layout: u32,
}

#[derive(Clone)]
struct PrototypeNamedCache {
    site: usize,
    receiver_layout: u32,
    depth: u8,
    links: [Option<PrototypeLink>; 4],
    value_slot: u32,
}

thread_local! {
    static PROTOTYPE_NAMED_CACHES: std::cell::RefCell<Vec<Option<PrototypeNamedCache>>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

const PROTOTYPE_CACHE_SLOTS: usize = 4096;

fn prototype_cache_hit(
    receiver: &crate::value::ObjectData,
    receiver_layout: u32,
    cache: u64,
    site: usize,
) -> Option<NamedCachedPayload> {
    if cache & PROTOTYPE_CACHE_TAG == 0 {
        return None;
    }
    let index = usize::try_from((cache & !PROTOTYPE_CACHE_TAG).checked_sub(1)?).ok()?;
    PROTOTYPE_NAMED_CACHES.with(|caches| {
        let caches = caches.borrow();
        let entry = caches.get(index)?.as_ref()?;
        if entry.site != site || entry.receiver_layout != receiver_layout {
            return None;
        }
        let mut owners: [*const crate::value::ObjectData; 4] = [std::ptr::null(); 4];
        for (depth, link) in entry.links[..usize::from(entry.depth)]
            .iter()
            .flatten()
            .enumerate()
        {
            let owner = if depth == 0 {
                receiver
            } else {
                // SAFETY: every pointer is owned by the preceding canonical
                // prototype slot, rooted by `receiver` for this lookup.
                unsafe { owners[depth - 1].as_ref()? }
            };
            let prototype = owner
                .hot_properties()
                .slot_word(link.prototype_slot as usize)?
                .object_or_null_ptr()??;
            if prototype != link.prototype.as_ptr()
                || unsafe { &*prototype }.semantic_layout_id() != link.prototype_layout
            {
                return None;
            }
            owners[depth] = prototype;
        }
        // SAFETY: the validated chain above keeps this owner reachable from
        // `receiver` until the returned word has been copied by the caller.
        let owner = unsafe { owners[usize::from(entry.depth).checked_sub(1)?].as_ref()? };
        let word = owner.hot_properties().slot_word(entry.value_slot as usize)?;
        word.trace_named_payload("prototype");
        Some(NamedCachedPayload::Word(word))
    })
}

fn cacheable_immediate_prototype(
    receiver: &crate::value::ObjectData,
    key: &str,
) -> Option<PrototypeNamedCache> {
    let mut links = std::array::from_fn(|_| None);
    let mut retained = std::array::from_fn::<_, 4, _>(|_| None);
    for depth in 0..links.len() {
        let owner = if depth == 0 {
            receiver
        } else {
            retained[depth - 1].as_deref()?
        };
        if shadows_named_property(owner, key) {
            return None;
        }
        let prototype_slot = owner.hot_properties().position_rev("\0prototype")?;
        let Value::Object(prototype) = owner.hot_properties().slot_value(prototype_slot)? else {
            return None;
        };
        links[depth] = Some(PrototypeLink {
            prototype_slot: u32::try_from(prototype_slot).ok()?,
            prototype: std::rc::Rc::downgrade(&prototype),
            prototype_layout: prototype.semantic_layout_id(),
        });
        if let Some(value_slot) =
            cacheable_own_slot(&Value::Object(std::rc::Rc::clone(&prototype)), key)
        {
            return Some(PrototypeNamedCache {
                site: 0,
                receiver_layout: receiver.semantic_layout_id(),
                depth: u8::try_from(depth + 1).ok()?,
                links,
                value_slot,
            });
        }
        retained[depth] = Some(prototype);
    }
    None
}

fn shadows_named_property(object: &crate::value::ObjectData, key: &str) -> bool {
    object.hot_properties().names().any(|name| {
        name == key
            || crate::builtins::is_deleted_key_for(name, key)
            || crate::builtins::is_descriptor_key_for(name, key)
    })
}

fn install_prototype_cache(cache: &std::cell::Cell<u64>, entry: PrototypeNamedCache) {
    PROTOTYPE_NAMED_CACHES.with(|caches| {
        let mut caches = caches.borrow_mut();
        if caches.is_empty() {
            caches.resize_with(PROTOTYPE_CACHE_SLOTS, || None);
        }
        let mut entry = entry;
        entry.site = cache as *const _ as usize;
        let start = entry.links[..usize::from(entry.depth)]
            .iter()
            .flatten()
            .fold(entry.site ^ entry.receiver_layout as usize, |hash, link| {
                hash.wrapping_mul(0x9e37_79b1)
                    .wrapping_add(link.prototype.as_ptr() as usize)
                    .wrapping_add(link.prototype_layout as usize)
                    .wrapping_add((link.prototype_slot as usize) << 8)
            })
            .wrapping_add(entry.value_slot as usize)
            & (PROTOTYPE_CACHE_SLOTS - 1);
        let index = (0..PROTOTYPE_CACHE_SLOTS)
            .map(|offset| (start + offset) & (PROTOTYPE_CACHE_SLOTS - 1))
            .find(|index| {
                caches[*index]
                    .as_ref()
                    .is_none_or(|cached| cached.site == entry.site)
            })
            .unwrap_or(start);
        caches[index] = Some(entry);
        cache.set(PROTOTYPE_CACHE_TAG | index as u64 + 1);
    });
}

#[cfg(test)]
mod named_prototype_cache_tests {
    use super::get_named_property_result;
    use crate::value::{ObjectData, Value};
    use std::{cell::Cell, rc::Rc};

    fn receiver(prototype_value: f64) -> Value {
        let prototype = Rc::new(ObjectData::new(vec![(
            "method".into(),
            Value::Number(prototype_value),
        )]));
        Value::Object(Rc::new(ObjectData::new(vec![(
            "\0prototype".into(),
            Value::Object(prototype),
        )])))
    }

    #[test]
    fn prototype_cache_guards_identity_when_receiver_layout_matches() {
        let cache = Cell::new(0);
        assert_eq!(
            get_named_property_result(&receiver(1.0), "method", &cache).unwrap(),
            Value::Number(1.0)
        );
        assert_eq!(
            get_named_property_result(&receiver(2.0), "method", &cache).unwrap(),
            Value::Number(2.0)
        );
    }
}

fn cacheable_own_slot(value: &Value, key: &str) -> Option<u32> {
    let Value::Object(object) = value else {
        return None;
    };
    if crate::vm::is_global_object(value) {
        return None;
    }
    let mut own = None;
    let mut metadata = None;
    for (slot, name) in object.hot_properties().names().enumerate().rev() {
        if crate::builtins::is_deleted_key_for(name, key) {
            return None;
        }
        if name == key && own.is_none() {
            own = Some((slot, object.hot_properties().slot_value(slot)?));
        }
        if metadata.is_none() && crate::builtins::is_descriptor_key_for(name, key) {
            metadata = object.hot_properties().slot_value(slot);
        }
    }
    let (slot, value) = own?;
    if matches!(value, Value::Null) && crate::vm::global_builtin_exists(key) {
        return None;
    }
    if metadata.is_some_and(|value| accessor_descriptor(&value)) {
        return None;
    }
    u32::try_from(slot).ok()
}

pub(crate) fn get_property_with_receiver(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Result<Value, VmError> {
    crate::module_bindings::exports(value, key)?;
    if let Some(value) = proven_own_data(value, key) {
        return Ok(value);
    }
    if let Some(result) = early_property_result(value, key, receiver) {
        return result;
    }
    if let Some(result) = array_property_result(value, key, receiver) {
        return result;
    }
    if let Some(result) = function_inherited_property_result(value, key, receiver) {
        return result;
    }
    if let Some(result) = object_inherited_property_result(value, key, receiver) {
        return result;
    }
    if let Some(result) = crate::disposable_stack::accessor(value, key, receiver) {
        return result;
    }
    if let Some(result) = data_view_instance_accessor(value, key) {
        return result;
    }
    if let Some(result) = descriptor_property_result(value, key, receiver) {
        return result;
    }
    finish_property_access(value, key, receiver)
}

pub(crate) fn proven_own_data(value: &Value, key: &str) -> Option<Value> {
    let Value::Object(properties) = value else {
        return None;
    };
    if crate::vm::is_global_object(value) {
        return None;
    }
    let mut own = None;
    let mut metadata = None;
    for (slot, name) in properties.names().enumerate().rev() {
        if crate::builtins::is_deleted_key_for(name, key) {
            return None;
        }
        if own.is_none() && name == key {
            own = properties.slot_value(slot);
        }
        if metadata.is_none() && crate::builtins::is_descriptor_key_for(name, key) {
            metadata = properties.slot_value(slot);
        }
    }
    let own = own?;
    if matches!(own, Value::Null) && crate::vm::global_builtin_exists(key) {
        return None;
    }
    if metadata.is_some_and(|value| accessor_descriptor(&value)) {
        return None;
    }
    Some(property_value(&own))
}

/// Return the canonical execute word for proven ordinary own data. This is
/// the L0 projection of `proven_own_data`: callers inspect the tag in place
/// and fall back before any accessor, descriptor, deletion, or cell semantics.
pub(crate) fn proven_own_word<'a>(
    object: &'a crate::value::ObjectData,
    key: &str,
) -> Option<&'a crate::register_file::SlotWord> {
    let mut own = None;
    let mut metadata = None;
    for (slot, name) in object.hot_properties().names().enumerate().rev() {
        if crate::builtins::is_deleted_key_for(name, key) {
            return None;
        }
        if own.is_none() && name == key {
            own = object.hot_properties().slot_word(slot);
        }
        if metadata.is_none() && crate::builtins::is_descriptor_key_for(name, key) {
            metadata = object.hot_properties().slot_value(slot);
        }
    }
    if metadata.is_some_and(|value| accessor_descriptor(&value)) {
        return None;
    }
    own
}

fn accessor_descriptor(value: &Value) -> bool {
    let Value::Object(fields) = value else {
        return true;
    };
    fields
        .names()
        .any(|name| matches!(name.as_str(), "get" | "set"))
}

fn function_inherited_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    let Value::Function(function) = value else {
        return None;
    };
    let properties = function.properties.borrow();
    if properties.iter().any(|(name, _)| name == key) {
        return None;
    }
    if matches!(key, "prototype") {
        return None;
    }
    if matches!(key, "caller" | "arguments")
        && function.strictness == crate::ops::FunctionStrictness::Sloppy
        && matches!(function.kind, crate::ops::FunctionKind::Ordinary)
    {
        return Some(Ok(Value::Undefined));
    }
    let prototype = properties
        .iter()
        .rev()
        .find_map(|(name, value)| {
            (name == "\0function_prototype" || name == "\0prototype").then(|| value.clone())
        })
        .unwrap_or_else(|| function_kind_prototype(function));
    Some(get_property_with_receiver(&prototype, key, receiver))
}

fn function_kind_prototype(function: &crate::value::FunctionValue) -> Value {
    let builtin = match (function.kind, function.is_async) {
        (crate::ops::FunctionKind::Generator, true) => {
            crate::ops::Builtin::AsyncGeneratorFunctionPrototype
        }
        (crate::ops::FunctionKind::Generator, false) => {
            crate::ops::Builtin::GeneratorFunctionPrototype
        }
        (_, true) => crate::ops::Builtin::AsyncFunctionPrototype,
        (_, false) => crate::ops::Builtin::FunctionPrototype,
    };
    crate::vm::realm_id_for_global_value(&function.captures.get(0))
        .and_then(|realm| crate::vm::realm::intrinsic(realm, builtin))
        .unwrap_or(Value::Builtin(builtin))
}

fn object_inherited_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    let Value::Object(properties) = value else {
        return None;
    };
    if properties.iter().any(|(name, _)| name == key) {
        return None;
    }
    if crate::vm::realm::id_for_global(properties).is_some()
        || crate::vm::is_global_object(&Value::Object(properties.clone()))
    {
        return None;
    }
    // Per spec §GetV for boxed primitives: an exotic String object exposes
    // its [[StringData]] as virtual indexed properties. Resolve the tag
    // before walking the prototype chain.
    if let Some(value) = boxed_string_property(properties, key) {
        return Some(Ok(value));
    }
    let Some(prototype_slot) = properties.position_rev("\0prototype") else {
        return Some(get_property_with_receiver(
            &Value::Builtin(crate::ops::Builtin::ObjectPrototype),
            key,
            receiver,
        ));
    };
    let prototype = properties.slot_value(prototype_slot)?;
    if matches!(prototype, Value::Null) {
        return Some(Ok(Value::Undefined));
    }
    Some(get_property_with_receiver(&prototype, key, receiver))
}

fn finish_property_access(value: &Value, key: &str, receiver: &Value) -> Result<Value, VmError> {
    match crate::property_define::accessor(value, key, "get") {
        None => Ok(receiver_property(value, key, receiver)),
        Some(Value::Undefined) => Ok(Value::Undefined),
        Some(getter) => invoke_accessor(&getter, receiver),
    }
}

fn early_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    if matches!(value, Value::Null | Value::Undefined) {
        return Some(Err(crate::value::error::throw_type_error(&format!(
            "Cannot read property `{key}` of null or undefined"
        ))));
    }
    if matches!(value, Value::Proxy(_)) {
        return Some(crate::proxy::proxy_get(value, key, Some(receiver)));
    }
    if matches!(value, Value::Array(values) if values.is_strict_arguments() && key == "callee") {
        return Some(Err(crate::value::error::throw_type_error(
            "'callee' is unavailable on strict arguments",
        )));
    }
    if has_restricted_function_property(value, key) {
        return Some(Err(crate::value::error::throw_type_error(
            "'caller' and 'arguments' are unavailable on this function",
        )));
    }
    if key == "buffer" && is_typed_array_prototype(value) {
        return Some(Err(crate::value::error::throw_type_error(
            "Receiver is not a TypedArray",
        )));
    }
    None
}

fn is_typed_array_prototype(value: &Value) -> bool {
    matches!(
        value,
        Value::Builtin(
            crate::ops::Builtin::Float64ArrayPrototype
                | crate::ops::Builtin::Float32ArrayPrototype
                | crate::ops::Builtin::Int8ArrayPrototype
                | crate::ops::Builtin::Int16ArrayPrototype
                | crate::ops::Builtin::Int32ArrayPrototype
                | crate::ops::Builtin::Uint8ArrayPrototype
                | crate::ops::Builtin::Uint16ArrayPrototype
                | crate::ops::Builtin::Uint32ArrayPrototype
                | crate::ops::Builtin::Uint8ClampedArrayPrototype
                | crate::ops::Builtin::BigInt64ArrayPrototype
                | crate::ops::Builtin::BigUint64ArrayPrototype
        )
    )
}

fn array_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    if let Some(getter) = array_accessor(value, key, "get") {
        return Some(match getter {
            Value::Undefined => Ok(Value::Undefined),
            getter => invoke_accessor(&getter, receiver),
        });
    }
    let Value::Array(values) = value else {
        return None;
    };
    if array_has_own_property(values, key) {
        return None;
    }
    if values.is_arguments() {
        return Some(get_property_with_receiver(
            &Value::Builtin(crate::ops::Builtin::ObjectPrototype),
            key,
            receiver,
        ));
    }
    if let Some(getter) = crate::arrays::prototype_override_getter(key) {
        return Some(match getter {
            Value::Undefined => Ok(Value::Undefined),
            getter => invoke_accessor(&getter, receiver),
        });
    }
    Some(get_property_with_receiver(
        &Value::Builtin(crate::ops::Builtin::ArrayPrototype),
        key,
        receiver,
    ))
}

fn array_has_own_property(values: &crate::value::ArrayData, key: &str) -> bool {
    key == "length"
        || crate::arrays::array_index(key).is_some_and(|index| values.has_index(index as usize))
        || values.descriptor(key).is_some()
        || values.property(key).is_some()
}

fn descriptor_property_result(
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Result<Value, VmError>> {
    if let Value::Builtin(builtin) = value {
        if let Some(getter) = crate::builtins::object::intrinsic_getter(*builtin, key) {
            return Some(invoke_accessor(&Value::Builtin(getter), receiver));
        }
        let property = receiver_property(value, key, receiver);
        return (!matches!(property, Value::Undefined)).then_some(Ok(property));
    }
    if let Value::Array(values) = value {
        if key == "length" {
            return Some(Ok(if values.is_arguments() {
                values.arguments_length_value()
            } else {
                Value::Number(values.logical_len() as f64)
            }));
        }
        if let Ok(index) = key.parse::<usize>() {
            return values.get_index(index).map(Ok);
        }
        if values.descriptor(key).is_none() {
            return values.property(key).map(|value| Ok(property_value(&value)));
        }
    }
    if let Value::String(text) = value {
        if key == "length" {
            return Some(Ok(Value::Number(crate::strings::utf16_len(text) as f64)));
        }
        if let Ok(index) = key.parse::<usize>() {
            return crate::strings::char_at_utf16(text, index)
                .map(Ok);
        }
        return None;
    }
    if let Value::Function(function) = value {
        if let Some(result) = function_descriptor_result(function, value, key, receiver) {
            return result;
        }
    }
    #[cfg(feature = "execution-trace")]
    crate::execution_trace::descriptor_object(match value {
        Value::Object(_) | Value::ObjectAlias(_) => "internal:object",
        Value::Array(_) => "internal:array",
        Value::Function(_) | Value::BoundFunction(_) => "internal:function",
        Value::String(_) | Value::StringUnits(_) => "internal:string",
        _ => "internal:other",
    });
    let Ok(descriptor) =
        crate::builtins::object::descriptor(Some(value), Some(&Value::String(key.to_string())))
    else {
        return None;
    };
    if matches!(descriptor, Value::Undefined) {
        return None;
    }
    if let Value::Object(descriptor) = descriptor {
        if let Some(getter) = descriptor
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "get").then_some(value))
        {
            return Some(match getter {
                Value::Undefined => Ok(Value::Undefined),
                getter => invoke_accessor(&getter, receiver),
            });
        }
    }
    Some(Ok(receiver_property(value, key, receiver)))
}

fn function_descriptor_result(
    function: &crate::value::FunctionValue,
    value: &Value,
    key: &str,
    receiver: &Value,
) -> Option<Option<Result<Value, VmError>>> {
    let properties = function.properties.borrow();
    if properties
        .iter()
        .any(|(name, _)| crate::builtins::is_deleted_key_for(name, key))
    {
        return Some(None);
    }
    if !properties.iter().any(|(name, _)| name == key) {
        return Some(None);
    }
    let metadata = crate::builtins::descriptor_metadata(properties.as_slice(), key);
    let getter = match metadata {
        None => None,
        Some(Value::Object(descriptor)) => descriptor
            .iter()
            .rev()
            .find_map(|(name, value)| (name == "get").then(|| value.clone())),
        Some(_) => return None,
    };
    drop(properties);
    Some(Some(match getter {
        Some(Value::Undefined) => Ok(Value::Undefined),
        Some(getter) => invoke_accessor(&getter, receiver),
        None => Ok(receiver_property(value, key, receiver)),
    }))
}

/// Invoke a getter using the receiver as `this`. The getter's own
/// `OrdinaryCallEvaluate` semantics handle ToObject coercion for sloppy
/// functions; strict functions keep the receiver as-is.
fn invoke_accessor(getter: &Value, receiver: &Value) -> Result<Value, VmError> {
    // Accessor descriptors may retain a live global/module binding cell.
    // Resolve the cell before applying the getter's callable dispatch.
    if let Value::BindingCell(cell) = getter {
        let value = cell.load();
        return invoke_accessor(&value, receiver);
    }
    match getter {
        Value::Function(_) | Value::BoundFunction(_) => {
            crate::functions::execute_target(getter, receiver, &[])
        }
        Value::Builtin(builtin) => {
            crate::vm::execute_builtin_with_receiver(*builtin, &[], Some(receiver))
        }
        _ => Err(crate::vm::not_callable()),
    }
}

fn receiver_property(value: &Value, key: &str, receiver: &Value) -> Value {
    let property = get_property(value, key);
    if should_preserve_receiver_property(value, key, &property, receiver)
        || same_property_receiver(value, receiver)
    {
        return property;
    }
    bind_receiver_property(property, receiver)
}

fn should_preserve_receiver_property(
    value: &Value,
    key: &str,
    property: &Value,
    receiver: &Value,
) -> bool {
    if object_has_property(value, key) {
        return true;
    }
    if plural_rules_instance(receiver) {
        return true;
    }
    matches!(value, Value::Builtin(_))
        || matches!(value, Value::Object(_)) && crate::vm::is_global_object(value)
        || is_intl_number_format_property(property)
        || is_boxed_primitive(receiver) && matches!(property, Value::Builtin(_))
        || matches!(key, "constructor" | "prototype")
        // Promise instances must return the prototype's `then`/`catch`/
        // `finally` by reference (ES §27.2.5); binding the receiver
        // creates a fresh BoundFunction per access.
        || matches!(value, Value::Promise(_)) && matches!(property, Value::Builtin(_))
}

fn plural_rules_instance(value: &Value) -> bool {
    let Value::Object(properties) = value else {
        return false;
    };
    properties.iter().any(|(name, prototype)| {
        name == "\0prototype"
            && match prototype {
                Value::Builtin(Builtin::IntlPluralRulesPrototype) => true,
                Value::BoundFunction(bound) => {
                    bound.target == Value::Builtin(Builtin::IntlPluralRulesPrototype)
                }
                _ => false,
            }
    })
}
fn is_boxed_primitive(value: &Value) -> bool {
    matches!(
        value,
        Value::Object(properties)
            if properties.iter().any(|(name, value)|
                name == "_value"
                    && matches!(
                        value,
                        Value::Number(_) | Value::Boolean(_) | Value::String(_) | Value::BigInt(_)
                    ))
    )
}

fn object_has_property(value: &Value, key: &str) -> bool {
    matches!(value, Value::Object(properties) if properties.iter().rev().any(|(name, _)| name == key))
}

fn is_intl_number_format_property(property: &Value) -> bool {
    matches!(
        property,
        Value::Builtin(
            Builtin::IntlNumberFormatFormatToParts
                | Builtin::IntlNumberFormatFormatRange
                | Builtin::IntlNumberFormatFormatRangeToParts
        )
    )
}

pub(crate) fn bind_receiver_property(property: Value, receiver: &Value) -> Value {
    match property {
        Value::Builtin(builtin)
            if !is_accessor_builtin(builtin)
                && !is_iterator_next_builtin(builtin)
                && crate::intl::tolocale::symbol::name(builtin).is_none() =>
        {
            bind_method(receiver, Value::Builtin(builtin))
        }
        other => other,
    }
}

fn is_iterator_next_builtin(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::IteratorNext
            | Builtin::RegExpStringIteratorNext
            | Builtin::StringIteratorNext
            | Builtin::SetIteratorNext
            | Builtin::MapIteratorNext
    )
}

/// Accessor getters/setters carry their `this` at invocation time; binding
/// them to the object they were read from (e.g. a property descriptor's
/// `.get`) would call them with the wrong receiver.
fn is_accessor_builtin(builtin: Builtin) -> bool {
    if builtin == Builtin::IntlNumberFormatFormat {
        return false;
    }
    let name = crate::builtins::builtin_name(builtin);
    name.starts_with("get ") || name.starts_with("set ")
}

#[cfg(test)]
mod proven_own_data_tests {
    use super::proven_own_data;
    use crate::value::{ObjectData, Value};
    use std::rc::Rc;

    fn object(entries: Vec<(String, Value)>) -> Value {
        Value::Object(Rc::new(ObjectData::new(entries)))
    }

    #[test]
    fn returns_plain_own_data() {
        let value = object(vec![("field".into(), Value::Number(7.0))]);
        assert_eq!(proven_own_data(&value, "field"), Some(Value::Number(7.0)));
        assert_eq!(proven_own_data(&value, "missing"), None);
    }

    #[test]
    fn rejects_deleted_and_accessor_properties() {
        let deleted = object(vec![
            ("field".into(), Value::Number(7.0)),
            (crate::builtins::deleted_key("field"), Value::Undefined),
        ]);
        assert_eq!(proven_own_data(&deleted, "field"), None);

        let getter = object(vec![("get".into(), Value::Undefined)]);
        let accessor = object(vec![
            ("field".into(), Value::Number(7.0)),
            (crate::builtins::descriptor_key("field"), getter),
        ]);
        assert_eq!(proven_own_data(&accessor, "field"), None);
    }
}
