//! `buffer.isUtf8`, `buffer.isAscii` and `buffer.isLatin1` (lib/buffer.js), with the public
//! functions' argument checks: nothing else reaches the binding's.

use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The largest code unit Node's one-byte `latin1` encodes.
const LATIN1_MAX: u16 = 0xff;

/// `valid` of a typed array's or an array buffer's bytes; any other input is
/// `ERR_INVALID_ARG_TYPE`.
fn validate(c: &mut Context<'_>, args: &[RootId], valid: fn(&[u8]) -> bool) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    if !c.has_builtin_brand_rooted(value, BuiltinBrand::TypedArray)? && !c.has_builtin_brand_rooted(value, BuiltinBrand::AnyArrayBuffer)? {
        return Err(super::util_shared_vm::argument_type_error_with_expected(c, value, "input", "an instance of ArrayBuffer, Buffer, or TypedArray")?);
    }
    let result = c.with_buffer_bytes_rooted(value, valid).unwrap_or(true);
    Ok(c.boolean(result))
}

pub(crate) fn is_utf8(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    validate(c, args, |bytes| std::str::from_utf8(bytes).is_ok())
}

pub(crate) fn is_ascii(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    validate(c, args, <[u8]>::is_ascii)
}

/// `isLatin1(input)`: every code unit of the string fits one `latin1` byte.
pub(crate) fn is_latin1(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = args.first().copied().unwrap_or_else(|| c.undefined());
    super::util_shared_vm::validate_string(c, input, "input")?;
    let result = c.to_string_code_units(input)?.iter().all(|unit| *unit <= LATIN1_MAX);
    Ok(c.boolean(result))
}
