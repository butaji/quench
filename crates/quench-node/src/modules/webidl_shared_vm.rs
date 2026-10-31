//! Native steps of undici's WebIDL conversions and Headers checks (`deps/undici/undici.js`),
//! which the bundle reaches through parameters of its module wrapper
//! (`UNDICI_WRAPPER_PREFIX`), bindings only its own code sees.
use crate::host::NodeHost;
use quench_runtime::{IntactBuiltin, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The library path of the undici bundle.
pub(crate) const UNDICI: &str = "deps/undici/undici.js";

/// The CommonJS wrapper of the undici bundle: the ordinary one, binding `byteStringEnd`
/// (`byte_string_end_binding!`) and `headerFields` (`header_fields_binding!`) too.
pub(crate) const UNDICI_WRAPPER_PREFIX: &str = concat!(
    "(function (exports, require, module, __filename, __dirname, ",
    byte_string_end_binding!(),
    ", ",
    header_fields_binding!(),
    ") { "
);

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
    let start = if c.builtins_intact(&[IntactBuiltin::StringCharCodeAt]) {
        c.string_units_within(text, BYTE_STRING_MAX_UNIT)?.unwrap_or(0)
    } else {
        0
    };
    Ok(c.number(start as f64))
}

/// What undici's Headers steps call on the way `header_fields` answers in their place:
/// ByteString's scan (`charCodeAt`), the name's token test (`tokenRegExp.test`, whose
/// `RegExpExec` reads `exec`, or a `charCodeAt` scan) and the value's test (`includes`).
const HEADER_STEP_BUILTINS: [IntactBuiltin; 4] = [
    IntactBuiltin::StringCharCodeAt,
    IntactBuiltin::StringIncludes,
    IntactBuiltin::RegExpTest,
    IntactBuiltin::RegExpExec,
];

/// Whether `unit` is an HTTP token character (RFC 9110, "tchar"): the set undici's
/// `validTokenChars` and `tokenRegExp` accept in a header name.
fn is_token_unit(unit: u16) -> bool {
    u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// Whether `unit` is HTTP whitespace (Fetch, "HTTP whitespace"), which undici's
/// `headerValueNormalize` strips from a value's ends.
fn is_http_whitespace(unit: u16) -> bool {
    matches!(unit, 0x09 | 0x0a | 0x0d | 0x20)
}

/// Whether undici's `isValidHeaderValue` refuses `unit` anywhere in a value (LF, CR, NUL).
fn breaks_header_value(unit: u16) -> bool {
    matches!(unit, 0x0a | 0x0d | 0x00)
}

/// `headerFields(name[, value])`: whether undici's Headers steps
/// (lib/web/fetch/headers.js) keep header name `name`, and value `value` when passed, exactly
/// as they are. That holds for strings (which ByteString's `String(V)` returns unchanged)
/// whose code units are ByteString units, `name` being a nonempty HTTP token and `value`
/// having no HTTP whitespace at its ends (which normalization would strip) and no unit the
/// value test refuses. The patched methods then skip those steps; `false` sends a call
/// through them, to convert, normalize or raise. So does any builtin those steps call not
/// being the realm's own (`HEADER_STEP_BUILTINS`).
pub(crate) fn header_fields(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let accepted = c.builtins_intact(&HEADER_STEP_BUILTINS) && header_name_kept(c, args.first().copied())? && match args.get(1) {
        Some(value) => header_value_kept(c, *value)?,
        None => true,
    };
    Ok(c.boolean(accepted))
}

fn header_name_kept(c: &mut Context<'_>, name: Option<RootId>) -> Result<bool, RootedError> {
    let Some(name) = name else {
        return Ok(false);
    };
    let mut nonempty = false;
    let tokens = c.string_units_all(name, |_, unit, _| {
        nonempty = true;
        is_token_unit(unit)
    })?;
    Ok(tokens == Some(true) && nonempty)
}

fn header_value_kept(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    let kept = c.string_units_all(value, |index, unit, length| {
        let end = index == 0 || index + 1 == length;
        unit <= BYTE_STRING_MAX_UNIT && !breaks_header_value(unit) && !(end && is_http_whitespace(unit))
    })?;
    Ok(kept == Some(true))
}
