//! Native decoder storage operations for configured Node source.
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
    if let Some(value) = shared.borrow().module_cache.get(CACHE_KEY).copied() {
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
        .module_cache
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
    for (index, byte) in state.0.iter().enumerate() {
        let key = c.string_rooted(&index.to_string());
        let value = c.number(f64::from(*byte));
        c.set_property_rooted(root, key, value, root)?;
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
