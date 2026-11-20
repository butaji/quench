//! Argument validation and normalization of the public Hash/Hmac policy: what Node's
//! hash.js checks before a value reaches the canonical native digest binding.
use super::crypto_hash_source_shared_vm::{get, nullish, undefined, Context};
use quench_runtime::{BuiltinBrand, RootId, RootedError, ValueType};

const DATA_TYPES: &str = "of type string or an instance of Buffer, TypedArray, or DataView";
const BUFFER_SOURCE_TYPES: &str = "of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView";
/// The digest encoding `crypto.hash` returns when its options name none.
const DEFAULT_OUTPUT_ENCODING: &str = "hex";

/// A Node-coded `ERR_INVALID_ARG_TYPE` for `value`, which is not of type `expected`.
fn invalid_type(c: &mut Context<'_>, value: RootId, name: &str, expected: &str) -> Result<RootedError, RootedError> {
    let error = super::util_shared_vm::argument_type_error(c, value, name, expected)?;
    if let Some(exception) = error.exception {
        super::node_errors_shared_vm::hide_type_error(c, exception, "ERR_INVALID_ARG_TYPE")?;
    }
    Ok(error)
}

pub(super) fn validate_string(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.value_type_rooted(value) == Some(ValueType::String) {
        return Ok(());
    }
    Err(invalid_type(c, value, name, "string")?)
}

/// Hash input: a string or a byte view.
pub(super) fn validate_data(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.value_type_rooted(value) == Some(ValueType::String) || c.has_builtin_brand_rooted(value, BuiltinBrand::ArrayBufferView)? {
        return Ok(());
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(c, value, name, DATA_TYPES)?)
}

pub(super) fn output_length(c: &mut Context<'_>, options: RootId) -> Result<RootId, RootedError> {
    if c.value_type_rooted(options) != Some(ValueType::Object) || nullish(c, options) {
        return Ok(c.undefined());
    }
    let length = get(c, options, "outputLength")?;
    checked_output_length(c, length, "options.outputLength")
}

pub(super) fn checked_output_length(c: &mut Context<'_>, length: RootId, name: &str) -> Result<RootId, RootedError> {
    if undefined(c, length) {
        return Ok(length);
    }
    let maximum = c.number(f64::from(u32::MAX));
    let error = match super::util_shared_vm::validate_integer(c, length, name, 0.0, maximum) {
        Ok(value) => return Ok(c.number(value + 0.0)),
        Err(error) => error,
    };
    if let Some(exception) = error.exception {
        if c.rooted_value(length).is_some_and(|value| value.as_number().is_some()) {
            super::node_errors_shared_vm::hide_range_error(c, exception, "ERR_OUT_OF_RANGE")?;
        } else {
            super::node_errors_shared_vm::hide_type_error(c, exception, "ERR_INVALID_ARG_TYPE")?;
        }
    }
    Err(error)
}

/// A cSHAKE parameter (`functionName`, `customization`): bytes without a NUL.
pub(super) fn parameter(c: &mut Context<'_>, value: RootId, name: &str) -> Result<RootId, RootedError> {
    if undefined(c, value) {
        return Ok(value);
    }
    let bytes = buffer_source(c, value, name, None)?;
    if c.with_buffer_bytes_rooted(bytes, |bytes| bytes.contains(&0)).unwrap_or(false) {
        return Err(super::util_shared_vm::argument_value_error(c, value, name, "must not contain NUL bytes")?);
    }
    Ok(bytes)
}

/// The cSHAKE parameters an options object names, each `undefined` when absent.
pub(super) fn parameters(c: &mut Context<'_>, options: RootId) -> Result<[RootId; 2], RootedError> {
    let function_name = get(c, options, "functionName")?;
    let function_name = parameter(c, function_name, "options.functionName")?;
    let customization = get(c, options, "customization")?;
    let customization = parameter(c, customization, "options.customization")?;
    Ok([function_name, customization])
}

pub(super) fn buffer_source(c: &mut Context<'_>, value: RootId, name: &str, encoding: Option<RootId>) -> Result<RootId, RootedError> {
    if c.value_type_rooted(value) == Some(ValueType::String) {
        return encoded_string(c, value, encoding);
    }
    if c.has_builtin_brand_rooted(value, BuiltinBrand::AnyArrayBuffer)? || c.has_builtin_brand_rooted(value, BuiltinBrand::ArrayBufferView)? {
        return Ok(value);
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(c, value, name, BUFFER_SOURCE_TYPES)?)
}

/// A string's bytes in `encoding`, UTF-8 when it is nullish, `"buffer"` or empty.
fn encoded_string(c: &mut Context<'_>, value: RootId, encoding: Option<RootId>) -> Result<RootId, RootedError> {
    let units = c.to_string_code_units(value)?;
    let encoding = match encoding {
        Some(encoding) if !nullish(c, encoding) => c.to_string(encoding)?,
        _ => "utf8".to_owned(),
    };
    let normalized = if encoding == "buffer" || encoding.is_empty() { Some("utf8") } else { super::buffer_codec::canonical_encoding(&encoding) };
    let Some(normalized) = normalized else {
        let error = c.type_error_rooted(&format!("Unknown encoding: {encoding}"))?;
        super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_UNKNOWN_ENCODING")?;
        return Err(c.throw(error));
    };
    c.uint8_array_from_rooted(super::buffer_codec::encode_units(&units, normalized))
}

/// `hash.update(data, 'hex')`: a hex string spells whole bytes.
pub(super) fn validate_hex_length(c: &mut Context<'_>, input: RootId, encoding: RootId) -> Result<(), RootedError> {
    let hex = c.string_text(encoding)?.and_then(|name| super::buffer_codec::canonical_encoding(&name)) == Some("hex");
    if !hex {
        return Ok(());
    }
    let length = c.to_string_code_units(input)?.len();
    if length % super::buffer_codec::HEX_CHARACTERS_PER_BYTE == 0 {
        return Ok(());
    }
    // A normalized hex label is three ASCII letters: literal quoting is the exact default
    // inspection, without loading the general object inspector.
    let undefined = c.undefined();
    let inspected = super::util_shared_vm::inspect_quote(c, undefined, &[encoding])?;
    let name: Vec<_> = "encoding".encode_utf16().collect();
    let reason = format!("is invalid for data of length {length}");
    let error = super::util_shared_vm::argument_value_error_inspected_units(c, &name, &reason, inspected)?;
    if let Some(exception) = error.exception {
        super::node_errors_shared_vm::hide_type_error(c, exception, "ERR_INVALID_ARG_VALUE")?;
    }
    Err(error)
}

/// `crypto.hash`'s options: an output encoding string, or an object naming the encoding,
/// output length and cSHAKE parameters.
pub(super) struct OneShotOptions {
    pub encoding: RootId,
    pub length: RootId,
    pub parameters: [RootId; 2],
}

pub(super) fn one_shot_options(c: &mut Context<'_>, options: RootId) -> Result<OneShotOptions, RootedError> {
    let missing = c.undefined();
    let mut parsed = OneShotOptions { encoding: missing, length: missing, parameters: [missing, missing] };
    if c.value_type_rooted(options) == Some(ValueType::String) {
        parsed.encoding = options;
    } else if !undefined(c, options) {
        if c.value_type_rooted(options) != Some(ValueType::Object) || nullish(c, options) || c.is_array_rooted(options)? {
            return Err(invalid_type(c, options, "options", "Object")?);
        }
        parsed.length = get(c, options, "outputLength")?;
        parsed.encoding = get(c, options, "outputEncoding")?;
        parsed.parameters = parameters(c, options)?;
    }
    if nullish(c, parsed.encoding) {
        parsed.encoding = c.string_rooted(DEFAULT_OUTPUT_ENCODING);
    }
    Ok(parsed)
}

/// The canonical name of `crypto.hash`'s output encoding, `"buffer"` included.
pub(super) fn output_encoding(c: &mut Context<'_>, encoding: RootId) -> Result<RootId, RootedError> {
    validate_string(c, encoding, "outputEncoding")?;
    let name = c.string_text(encoding)?.expect("validated encoding");
    let normalized = if name.is_empty() {
        Some("utf8")
    } else if name.eq_ignore_ascii_case("buffer") {
        Some("buffer")
    } else {
        super::buffer_codec::canonical_encoding(&name)
    };
    match normalized {
        Some(value) => Ok(c.string_rooted(value)),
        None => Err(super::util_shared_vm::argument_value_error(c, encoding, "outputEncoding", "is invalid")?),
    }
}
