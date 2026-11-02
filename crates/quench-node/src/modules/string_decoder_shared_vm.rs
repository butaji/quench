//! `node:string_decoder` in Rust (lib/string_decoder.js) over the native decoder state.
use super::string_decoder_codec::{DecoderState, Layout, StringTooLong, ENCODINGS};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    const CACHE_KEY: &str = "\0binding:string_decoder";
    let shared = c.host_mut().shared_state();
    if let Some(value) = shared.borrow().builtin_cache.get(CACHE_KEY).copied() {
        return Ok(value);
    }
    let binding = c.object_rooted()?;
    for (name, number) in [
        ("kIncompleteCharactersStart", Layout::INCOMPLETE_START),
        ("kIncompleteCharactersEnd", Layout::INCOMPLETE_END),
        ("kMissingBytes", Layout::MISSING),
        ("kBufferedBytes", Layout::BUFFERED),
        ("kEncodingField", Layout::ENCODING),
        ("kNumFields", Layout::SIZE),
        ("kSize", Layout::SIZE),
    ] {
        let value = c.number(number as f64);
        put(c, binding, name, value)?;
    }
    let encodings = ENCODINGS
        .iter()
        .map(|name| c.string_rooted(name))
        .collect::<Vec<_>>();
    let encodings = c.array_rooted(&encodings)?;
    put(c, binding, "encodings", encodings)?;
    for (name, operation) in [
        ("decode", "stringDecoderDecode"),
        ("flush", "stringDecoderFlush"),
    ] {
        let function = c.host_function(crate::host::shared_vm::operation(operation))?;
        let key = c.string_rooted("name");
        let value = c.string_rooted(name);
        c.define_data_property_rooted(function, key, value, false, false, true)?;
        put(c, binding, name, function)?;
    }
    let retained = c.retain(binding)?;
    shared
        .borrow_mut()
        .builtin_cache
        .insert(CACHE_KEY.to_owned(), retained);
    Ok(binding)
}
fn invalid_state(c: &mut Context<'_>) -> Result<RootedError, RootedError> {
    let error = c.type_error_rooted("Invalid StringDecoder native state")?;
    Ok(c.throw(error))
}
fn state(c: &mut Context<'_>, root: RootId) -> Result<DecoderState, RootedError> {
    let Some(bytes) = c.view_bytes_rooted(root) else {
        return Err(invalid_state(c)?);
    };
    let Some(state) = DecoderState::from_bytes(&bytes) else {
        return Err(invalid_state(c)?);
    };
    Ok(state)
}
fn finish(
    c: &mut Context<'_>,
    root: RootId,
    state: &DecoderState,
    result: Result<Vec<u16>, StringTooLong>,
) -> Result<RootId, RootedError> {
    // The same Core-owned bytes exposed by lastChar/lastNeed/lastTotal are the
    // decoder state; there is no second WeakMap or synthetic field ledger.
    // `state` read these bytes from the view, so the view holds them: they are written back
    // in place, as stores of its elements would.
    let written = c.with_buffer_bytes_mut_rooted(root, |bytes| {
        bytes.get_mut(..state.0.len()).map(|target| target.copy_from_slice(&state.0))
    });
    if written.flatten().is_none() {
        return Err(invalid_state(c)?);
    }
    match result {
        Ok(units) => Ok(c.string_units_rooted(&units)),
        Err(_) => {
            let message = format!(
                "Cannot create a string longer than 0x{:x} characters",
                super::string_decoder_codec::MAX_STRING_BYTES
            );
            let error = c.error_rooted(&message)?;
            let code = c.string_rooted("ERR_STRING_TOO_LONG");
            put(c, error, "code", code)?;
            Err(c.throw(error))
        }
    }
}
pub(crate) fn decode(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(&root) = args.first() else {
        return Err(invalid_state(c)?);
    };
    let Some(&input_root) = args.get(1) else {
        return Err(invalid_state(c)?);
    };
    let mut state = state(c, root)?;
    let Some(input) = c.view_bytes_rooted(input_root) else {
        return Err(invalid_state(c)?);
    };
    let result = state.decode(&input);
    finish(c, root, &state, result)
}
pub(crate) fn flush(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(&root) = args.first() else {
        return Err(invalid_state(c)?);
    };
    let mut state = state(c, root)?;
    let result = state.flush();
    finish(c, root, &state, result)
}

/// The cache key of the decoders' native-state symbol.
const STATE_SYMBOL_KEY: &str = "\0node/string_decoder/kNativeDecoder";

/// `kNativeDecoder`: the private-by-convention key of a decoder's native state buffer.
fn state_symbol(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    if let Some(symbol) = shared.borrow().builtin_cache.get(STATE_SYMBOL_KEY).copied() {
        return Ok(symbol);
    }
    let symbol = c.symbol_rooted(Some("kNativeDecoder"));
    let symbol = c.retain(symbol)?;
    shared.borrow_mut().builtin_cache.insert(STATE_SYMBOL_KEY.to_owned(), symbol);
    Ok(symbol)
}

/// `node:string_decoder` (lib/string_decoder.js): `{ StringDecoder }`.
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let operation = crate::host::shared_vm::operation;
    let class = c.host_constructor_named(operation("stringDecoderConstruct"), "StringDecoder")?;
    let length = c.number(1.0);
    let key = c.string_rooted("length");
    c.define_data_property_rooted(class, key, length, false, false, true)?;
    let prototype = c.get_named_property_rooted(class, "prototype")?;
    for (name, operation_name, length) in [
        ("write", "stringDecoderWrite", 1.0),
        ("end", "stringDecoderEnd", 1.0),
        ("text", "stringDecoderText", 2.0),
    ] {
        let method = c.host_function_named(operation(operation_name), name)?;
        let key = c.string_rooted("length");
        let length = c.number(length);
        c.define_data_property_rooted(method, key, length, false, false, true)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(prototype, key, method, true, true, true)?;
    }
    for (index, name) in ["lastChar", "lastNeed", "lastTotal"].into_iter().enumerate() {
        let data = c.number(index as f64);
        let getter = c.host_function_named_with_data(operation("stringDecoderField"), "get", data)?;
        let descriptor = c.null_object_rooted()?;
        c.assign_named_property_rooted(descriptor, "get", getter)?;
        let yes = c.boolean(true);
        c.assign_named_property_rooted(descriptor, "enumerable", yes)?;
        c.assign_named_property_rooted(descriptor, "configurable", yes)?;
        let key = c.string_rooted(name);
        c.define_property_descriptor_rooted(prototype, key, descriptor)?;
    }
    let module = c.object_rooted()?;
    put(c, module, "StringDecoder", class)?;
    Ok(module)
}

/// `new StringDecoder(encoding)`.
pub(crate) fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if c.new_target_rooted().is_none() {
        let error = c.type_error_rooted("Cannot set properties of undefined (setting 'encoding')")?;
        return Err(c.throw(error));
    }
    let encoding = args.first().copied().unwrap_or_else(|| c.undefined());
    let name = match c.value_type_rooted(encoding) {
        Some(quench_runtime::ValueType::Undefined | quench_runtime::ValueType::Null) => Some("utf8"),
        _ => {
            let text = c.to_string(encoding)?;
            if text.is_empty() { Some("utf8") } else { super::buffer_codec::canonical_encoding(&text) }
        }
    };
    let Some(name) = name else {
        let text = c.to_string(encoding)?;
        let error = c.type_error_rooted(&format!("Unknown encoding: {text}"))?;
        super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_UNKNOWN_ENCODING")?;
        return Err(c.throw(error));
    };
    let name_root = c.string_rooted(name);
    put(c, this, "encoding", name_root)?;
    let mut bytes = [0; Layout::SIZE];
    bytes[Layout::ENCODING] = ENCODINGS.iter().position(|candidate| *candidate == name).unwrap_or_default() as u8;
    let state = super::buffer_shared_vm::from_bytes(c, &bytes)?;
    let key = state_symbol(c)?;
    c.define_data_property_rooted(this, key, state, true, true, true)?;
    Ok(this)
}

fn native_state(c: &mut Context<'_>, this: RootId) -> Result<RootId, RootedError> {
    let key = state_symbol(c)?;
    let state = if c.is_object_rooted(this)? { c.get_property_rooted(this, key)? } else { c.undefined() };
    if !c.truthy_rooted(state)? {
        let error = c.type_error_rooted("Value of \"this\" must be of type StringDecoder")?;
        super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_THIS")?;
        return Err(c.throw(error));
    }
    Ok(state)
}

/// `decoder.write(buf)`.
pub(crate) fn write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = args.first().copied().unwrap_or_else(|| c.undefined());
    if c.value_type_rooted(input) == Some(quench_runtime::ValueType::String) {
        return Ok(input);
    }
    if !c.has_builtin_brand_rooted(input, quench_runtime::BuiltinBrand::ArrayBufferView)? {
        return Err(super::util_shared_vm::argument_type_error_with_expected(
            c,
            input,
            "buf",
            "an instance of Buffer, TypedArray, or DataView",
        )?);
    }
    let state = native_state(c, this)?;
    let undefined = c.undefined();
    decode(c, undefined, &[state, input])
}

/// `decoder.end(buf)`.
pub(crate) fn end(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let mut text = Vec::new();
    if let Some(&input) = args.first().filter(|input| c.rooted_value(**input).is_some_and(|value| !value.is_undefined())) {
        let written = write(c, this, &[input])?;
        text = c.string_code_units(written)?.unwrap_or_default();
    }
    let state = native_state(c, this)?;
    let buffered = c.with_view_bytes_rooted(state, |bytes| bytes[Layout::BUFFERED]).unwrap_or_default();
    if buffered > 0 {
        let undefined = c.undefined();
        let flushed = flush(c, undefined, &[state])?;
        text.extend(c.string_code_units(flushed)?.unwrap_or_default());
    }
    Ok(c.string_units_rooted(&text))
}

/// `decoder.text(buf, offset)`: forgets the partial character, then writes `buf.slice(offset)`.
pub(crate) fn text(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = native_state(c, this)?;
    c.with_buffer_bytes_mut_rooted(state, |bytes| {
        bytes[Layout::MISSING] = 0;
        bytes[Layout::BUFFERED] = 0;
    });
    let undefined = c.undefined();
    let input = args.first().copied().unwrap_or(undefined);
    let offset = args.get(1).copied().unwrap_or(undefined);
    let slice = c.get_named_property_rooted(input, "slice")?;
    let sliced = c.call_rooted(slice, input, &[offset])?;
    write(c, this, &[sliced])
}

/// `lastChar`, `lastNeed` and `lastTotal`, by their index in that order.
pub(crate) fn field(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let index = c.to_number(data)? as usize;
    let key = state_symbol(c)?;
    let state = c.get_property_rooted(this, key)?;
    let field = |c: &mut Context<'_>, offset: usize| c.with_view_bytes_rooted(state, |bytes| f64::from(bytes[offset])).unwrap_or_default();
    match index {
        0 => {
            let subarray = c.get_named_property_rooted(state, "subarray")?;
            let start = c.number(Layout::INCOMPLETE_START as f64);
            let end = c.number(Layout::INCOMPLETE_END as f64);
            c.call_rooted(subarray, state, &[start, end])
        }
        1 => {
            let missing = field(c, Layout::MISSING);
            Ok(c.number(missing))
        }
        _ => {
            let total = field(c, Layout::BUFFERED) + field(c, Layout::MISSING);
            Ok(c.number(total))
        }
    }
}
