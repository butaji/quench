//! Shared-VM TextDecoder facade over the Node-owned decoder semantics.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};


pub(crate) const GLOBAL_NAMES: &[&str] = &["TextDecoder"];

/// Installs `TextDecoder` and `TextEncoder` as lazy data properties, built on first use.
pub(crate) fn install_global(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let initializer =
        context.host_function(crate::host::shared_vm::operation!("textDecoderInitialize"))?;
    let global = context.global_root()?;
    context.define_lazy_data_properties_rooted(
        global,
        GLOBAL_NAMES,
        false,
        initializer,
    )?;
    context.release_root(initializer);
    let initializer = context.host_function(crate::host::shared_vm::operation!("textEncoderInitialize"))?;
    context.define_lazy_data_properties_rooted(global, &["TextEncoder"], false, initializer)?;
    context.release_root(initializer);
    Ok(())
}

pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let operation = crate::host::shared_vm::operation;
    let decoder = context.host_constructor_named(crate::host::shared_vm::operation!("textDecoderConstructor"), "TextDecoder")?;
    let decoder_prototype = context.get_named_property_rooted(decoder, "prototype")?;
    method(context, decoder_prototype, "decode", crate::host::shared_vm::operation!("textDecoderDecodeMethod"))?;
    for (name, getter) in [("encoding", "textDecoderEncodingGet"), ("fatal", "textDecoderFatalGet"), ("ignoreBOM", "textDecoderIgnoreBomGet")] {
        accessor(context, decoder_prototype, name, operation(getter))?;
    }
    to_string_tag(context, decoder_prototype, "TextDecoder")?;
    let global = context.global_root()?;
    context.define_named_data_property_rooted(global, "TextDecoder", decoder, true, false, true)?;
    Ok(context.undefined())
}

/// The private slot holding a TextDecoder's `[encoding, fatal, ignoreBOM, pending, bomSeen]`:
/// a streaming decode's undecoded tail bytes and whether its output has begun.
const DECODER_SLOT: &str = "\0slot:TextDecoder";
const PENDING: &str = "3";
const BOM_SEEN: &str = "4";

/// Ends a stream: no bytes pending and the next output may start with a BOM.
fn reset_stream(context: &mut NativeContext<'_, NodeHost>, state: RootId) -> Result<(), RootedError> {
    let empty = context.uint8_array_from_rooted(Vec::new())?;
    context.assign_named_property_rooted(state, PENDING, empty)?;
    let no = context.boolean(false);
    context.assign_named_property_rooted(state, BOM_SEEN, no)
}

fn accessor(context: &mut NativeContext<'_, NodeHost>, prototype: RootId, name: &str, operation: quench_runtime::HostFunctionId) -> Result<(), RootedError> {
    let getter = context.host_function_named(operation, &format!("get {name}"))?;
    let descriptor = context.null_object_rooted()?;
    context.assign_named_property_rooted(descriptor, "get", getter)?;
    let yes = context.boolean(true);
    context.assign_named_property_rooted(descriptor, "enumerable", yes)?;
    context.assign_named_property_rooted(descriptor, "configurable", yes)?;
    let key = context.string_rooted(name);
    context.define_property_descriptor_rooted(prototype, key, descriptor)?;
    Ok(())
}

fn method(context: &mut NativeContext<'_, NodeHost>, prototype: RootId, name: &str, operation: quench_runtime::HostFunctionId) -> Result<(), RootedError> {
    let function = context.host_function_named(operation, name)?;
    let key = context.string_rooted(name);
    context.define_data_property_rooted(prototype, key, function, true, true, true)?;
    Ok(())
}

fn to_string_tag(context: &mut NativeContext<'_, NodeHost>, prototype: RootId, tag: &str) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let symbol = context.get_named_property_rooted(global, "Symbol")?;
    let key = context.get_named_property_rooted(symbol, "toStringTag")?;
    let value = context.string_rooted(tag);
    context.define_data_property_rooted(prototype, key, value, false, false, true)?;
    Ok(())
}

fn argument(context: &mut NativeContext<'_, NodeHost>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => context.undefined(),
    }
}

fn illegal_invocation(context: &mut NativeContext<'_, NodeHost>) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted("Illegal invocation")?;
    super::node_errors_shared_vm::decorate_type_error(context, error, "ERR_INVALID_THIS")?;
    Ok(context.throw(error))
}

/// A TextDecoder's state, or `ERR_INVALID_THIS`.
fn decoder_state(context: &mut NativeContext<'_, NodeHost>, receiver: RootId) -> Result<RootId, RootedError> {
    match super::private_slots::read(context, receiver, DECODER_SLOT)? {
        Some(state) => Ok(state),
        None => Err(illegal_invocation(context)?),
    }
}

/// `new TextDecoder([label[, options]])`.
pub(crate) fn decoder_constructor(context: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if context.new_target_rooted().is_none() {
        let error = context.type_error_rooted("Class constructor TextDecoder cannot be invoked without 'new'")?;
        return Err(context.throw(error));
    }
    let label = argument(context, args, 0);
    let label = if context.rooted_value(label).is_some_and(|value| value.is_undefined()) {
        context.string_rooted("utf-8")
    } else {
        let text = context.to_string_code_units(label)?;
        context.string_units_rooted(&text)
    };
    let missing = context.undefined();
    let encoding = canonical_encoding(context, missing, &[label])?;
    let options = argument(context, args, 1);
    let (fatal, ignore_bom) = if context.is_object_rooted(options)? {
        let fatal = context.get_named_property_rooted(options, "fatal")?;
        let ignore = context.get_named_property_rooted(options, "ignoreBOM")?;
        (context.truthy_rooted(fatal)?, context.truthy_rooted(ignore)?)
    } else if context.rooted_value(options).is_some_and(|value| value.is_undefined() || value.is_null()) {
        (false, false)
    } else {
        return Err(super::util_shared_vm::argument_type_error(context, options, "options", "object")?);
    };
    let fatal = context.boolean(fatal);
    let ignore_bom = context.boolean(ignore_bom);
    let state = context.array_rooted(&[encoding, fatal, ignore_bom])?;
    reset_stream(context, state)?;
    super::private_slots::write(context, this, DECODER_SLOT, state)?;
    Ok(missing)
}

fn state_field(context: &mut NativeContext<'_, NodeHost>, this: RootId, index: &str) -> Result<RootId, RootedError> {
    let state = decoder_state(context, this)?;
    context.get_named_property_rooted(state, index)
}

pub(crate) fn decoder_encoding(context: &mut NativeContext<'_, NodeHost>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    state_field(context, this, "0")
}

pub(crate) fn decoder_fatal(context: &mut NativeContext<'_, NodeHost>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    state_field(context, this, "1")
}

pub(crate) fn decoder_ignore_bom(context: &mut NativeContext<'_, NodeHost>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    state_field(context, this, "2")
}

/// `textDecoder.decode([input[, options]])`.
pub(crate) fn decoder_decode(context: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = decoder_state(context, this)?;
    let input = argument(context, args, 0);
    let input = if context.rooted_value(input).is_some_and(|value| value.is_undefined()) {
        context.uint8_array_from_rooted(Vec::new())?
    } else if context.has_builtin_brand_rooted(input, quench_runtime::BuiltinBrand::ArrayBufferView)?
        || context.has_builtin_brand_rooted(input, quench_runtime::BuiltinBrand::AnyArrayBuffer)?
    {
        let bytes = context.with_buffer_bytes_rooted(input, <[u8]>::to_vec).unwrap_or_default();
        context.uint8_array_from_rooted(bytes)?
    } else {
        // Node's decoder binding names its argument "list" and reports no received value.
        let error = context.type_error_rooted(
            "The \"list\" argument must be an instance of SharedArrayBuffer, ArrayBuffer or ArrayBufferView.",
        )?;
        super::node_errors_shared_vm::decorate_type_error(context, error, "ERR_INVALID_ARG_TYPE")?;
        return Err(context.throw(error));
    };
    let options = argument(context, args, 1);
    let stream = if context.is_object_rooted(options)? {
        let stream = context.get_named_property_rooted(options, "stream")?;
        context.truthy_rooted(stream)?
    } else {
        false
    };
    let encoding_root = context.get_named_property_rooted(state, "0")?;
    let encoding = context.string_text(encoding_root)?.unwrap_or_default();
    let fatal = context.get_named_property_rooted(state, "1")?;
    let fatal = context.truthy_rooted(fatal)?;
    let ignore_bom = context.get_named_property_rooted(state, "2")?;
    let ignore_bom = context.truthy_rooted(ignore_bom)?;
    let pending = context.get_named_property_rooted(state, PENDING)?;
    let mut bytes = context.view_bytes_rooted(pending).unwrap_or_default();
    bytes.extend(context.view_bytes_rooted(input).unwrap_or_default());
    let complete = if stream { bytes.len() - crate::modules::text_decoder_codec::incomplete_tail(&encoding, &bytes) } else { bytes.len() };
    let tail = bytes.split_off(complete);
    let bom_seen = context.get_named_property_rooted(state, BOM_SEEN)?;
    let bom_seen = context.truthy_rooted(bom_seen)?;
    let Ok(text) = crate::modules::text_decoder_codec::decode_bytes(&encoding, &bytes, fatal) else {
        reset_stream(context, state)?;
        return Err(invalid_encoded_data(context, &encoding)?);
    };
    let strips_bom = !ignore_bom && !bom_seen && matches!(encoding.as_str(), "utf-8" | "utf-16le" | "utf-16be");
    let text = if strips_bom { text.strip_prefix('\u{feff}').unwrap_or(&text) } else { &text };
    let result = context.string_rooted(text);
    if stream {
        let tail = context.uint8_array_from_rooted(tail)?;
        context.assign_named_property_rooted(state, PENDING, tail)?;
        let seen = context.boolean(bom_seen || !bytes.is_empty());
        context.assign_named_property_rooted(state, BOM_SEEN, seen)?;
    } else {
        reset_stream(context, state)?;
    }
    Ok(result)
}

/// `ERR_ENCODING_INVALID_ENCODED_DATA` for `encoding`.
fn invalid_encoded_data(context: &mut NativeContext<'_, NodeHost>, encoding: &str) -> Result<RootedError, RootedError> {
    // Node shares the ISO-8859-8 decoder with its logical-order alias,
    // including that decoder's canonical name in fatal diagnostics.
    let failure_encoding = if encoding == "iso-8859-8-i" { "iso-8859-8" } else { encoding };
    let error = context.type_error_rooted(&format!("The encoded data was not valid for encoding {failure_encoding}"))?;
    super::node_errors_shared_vm::decorate_type_error(context, error, "ERR_ENCODING_INVALID_ENCODED_DATA")?;
    Ok(context.throw(error))
}


pub(crate) fn canonical_encoding(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(root) = args.first().copied() else {
        return Ok(context.undefined());
    };
    let Some(label) = context.string_text(root)? else {
        return Ok(context.undefined());
    };
    match crate::modules::text_decoder_codec::canonical_encoding(&label) {
        Some(encoding) if encoding != "replacement" => Ok(context.string_rooted(&encoding)),
        encoding => {
            let label = if encoding.is_some() {
                "replacement".encode_utf16().collect()
            } else {
                context.to_string_code_units(root)?
            };
            let mut units = "The \"".encode_utf16().collect::<Vec<_>>();
            units.extend(label);
            units.extend("\" encoding is not supported".encode_utf16());
            let message = context.string_units_rooted(&units);
            let error = context.range_error_rooted("")?;
            context.assign_named_property_rooted(error, "message", message)?;
            super::node_errors_shared_vm::decorate_range_error(
                context,
                error,
                "ERR_ENCODING_NOT_SUPPORTED",
            )?;
            Err(context.throw(error))
        }
    }
}

pub(crate) fn decode(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let bytes = byte_array(context, args.first().copied())?;
    let Some(encoding_root) = args.get(1).copied() else {
        return Err(RootedError::host("TextDecoder encoding is missing"));
    };
    let encoding = context
        .string_text(encoding_root)?
        .ok_or_else(|| RootedError::host("TextDecoder encoding is not a string"))?;
    let fatal = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let Ok(text) = crate::modules::text_decoder_codec::decode_bytes(&encoding, &bytes, fatal) else {
        return Err(invalid_encoded_data(context, &encoding)?);
    };
    let ignore_bom = args
        .get(3)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let text = if !ignore_bom && matches!(encoding.as_str(), "utf-8" | "utf-16le" | "utf-16be") {
        text.strip_prefix('\u{feff}').unwrap_or(&text)
    } else {
        &text
    };
    Ok(context.string_rooted(text))
}

/// The bytes `decode` reads: the Uint8Array the TextDecoder's `decode` makes over its input,
/// read in place.
fn byte_array(
    context: &mut NativeContext<'_, NodeHost>,
    root: Option<RootId>,
) -> Result<Vec<u8>, RootedError> {
    let Some(root) = root else {
        return Ok(Vec::new());
    };
    context
        .view_bytes_rooted(root)
        .ok_or_else(|| RootedError::host("TextDecoder input is not a byte view"))
}
