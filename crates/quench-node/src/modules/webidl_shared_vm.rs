//! Native steps of undici's WebIDL conversions (`deps/undici/undici.js`), which the bundle
//! reaches through a parameter of its module wrapper (`UNDICI_WRAPPER_PREFIX`), a binding
//! only its own code sees.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The library path of the undici bundle.
pub(crate) const UNDICI: &str = "deps/undici/undici.js";

/// The CommonJS wrapper of the undici bundle: the ordinary one, binding `byteStringEnd` too
/// (`byte_string_end_binding!`).
pub(crate) const UNDICI_WRAPPER_PREFIX: &str =
    concat!("(function (exports, require, module, __filename, __dirname, ", byte_string_end_binding!(), ") { ");

/// The largest code unit a WebIDL ByteString holds (WebIDL, "ByteString").
const BYTE_STRING_MAX_UNIT: u16 = 0xff;

/// `byteStringEnd(x)`, where undici's `webidl.converters.ByteString` starts its scan of the
/// string `x` for a code unit above `BYTE_STRING_MAX_UNIT`: the first such index, or `x`'s
/// length when there is none, so the scan raises at that index or makes no step. Its steps
/// call `x.charCodeAt`; while `String.prototype.charCodeAt` is not the realm's own
/// (`NativeContext::string_char_code_at_intact`), the scan starts at 0 and makes every call.
pub(crate) fn byte_string_end(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let text = args.first().copied().unwrap_or(undefined);
    let start = if c.string_char_code_at_intact() { c.string_units_within(text, BYTE_STRING_MAX_UNIT)?.unwrap_or(0) } else { 0 };
    Ok(c.number(start as f64))
}
