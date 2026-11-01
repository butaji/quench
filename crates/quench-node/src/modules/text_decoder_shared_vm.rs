//! Shared-VM TextDecoder facade over the Node-owned decoder semantics.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};


pub(crate) const GLOBAL_NAMES: &[&str] = &["TextDecoder"];

/// Installs `TextDecoder` and `TextEncoder` as lazy data properties, built on first use.
pub(crate) fn install_global(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let initializer =
        context.host_function(crate::host::shared_vm::operation("textDecoderInitialize"))?;
    let global = context.global_root()?;
    context.define_lazy_data_properties_rooted(
        global,
        GLOBAL_NAMES,
        false,
        initializer,
    )?;
    context.release_root(initializer);
    let initializer = context.host_function(crate::host::shared_vm::operation("textEncoderInitialize"))?;
    context.define_lazy_data_properties_rooted(global, &["TextEncoder"], false, initializer)?;
    context.release_root(initializer);
    Ok(())
}

pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    #[cfg(quench_icu)]
    let constructors = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "util.js",
        "internal/encoding.js",
    )?;
    #[cfg(not(quench_icu))]
    let constructors = {
        let canonical = context.host_function(crate::host::shared_vm::operation(
            "textDecoderCanonicalEncoding",
        ))?;
        let decode =
            context.host_function(crate::host::shared_vm::operation("textDecoderDecode"))?;
        let factory = crate::host::removed_javascript("node:text-decoder/shared-vm.js")?;
        let undefined = context.undefined();
        context.call_rooted(factory, undefined, &[canonical, decode])?
    };
    let undefined = context.undefined();
    let decoder_key = context.string_rooted("TextDecoder");
    let constructor = context.get_property_rooted(constructors, decoder_key)?;
    let encoder_key = context.string_rooted("TextEncoder");
    let encoder = context.get_property_rooted(constructors, encoder_key)?;
    let install = crate::host::removed_javascript("node:text-decoder/install-global.js")?;
    context.call_rooted(install, undefined, &[constructor, encoder])?;
    Ok(undefined)
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
    let Some(text) =
        crate::modules::text_decoder_codec::decode_bytes(&encoding, &bytes, fatal).ok()
    else {
        // Node shares the ISO-8859-8 decoder with its logical-order alias,
        // including that decoder's canonical name in fatal diagnostics.
        let failure_encoding = if encoding == "iso-8859-8-i" {
            "iso-8859-8"
        } else {
            &encoding
        };
        let error = context.type_error_rooted(&format!(
            "The encoded data was not valid for encoding {failure_encoding}"
        ))?;
        super::node_errors_shared_vm::decorate_type_error(
            context,
            error,
            "ERR_ENCODING_INVALID_ENCODED_DATA",
        )?;
        return Err(context.throw(error));
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
