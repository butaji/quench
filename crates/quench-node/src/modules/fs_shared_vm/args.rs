//! Node's fs argument policy (`internal/fs/utils.js`, `internal/validators.js`): each public
//! argument is checked once and becomes the Rust value the operation consumes.

use crate::modules::stream_shared_vm::{get, is_nullish, is_undefined, Context};
use crate::modules::util_shared_vm as util;
use quench_runtime::{BuiltinBrand, RootId, RootedError};
use std::path::PathBuf;

/// The largest file descriptor and the largest unsigned 32-bit value Node's validators accept.
const MAX_FD: f64 = i32::MAX as f64;
pub(crate) const MAX_UINT32: f64 = u32::MAX as f64;
/// `Number.MAX_SAFE_INTEGER`, the bound of integer offsets and lengths.
pub(crate) const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const MILLISECONDS_PER_SECOND: f64 = 1000.0;
/// The radix of a mode given as a string (`parseFileMode`).
const OCTAL_RADIX: u32 = 8;

/// A validated path: the bytes the host opens, and the text Node's errors show.
pub(crate) struct FsPath {
    pub(crate) io: PathBuf,
    pub(crate) shown: String,
}

/// `getValidatedPath(value, name)`: a string, Buffer or `file:` URL without NUL bytes, which
/// the host opens relative to the working directory.
pub(crate) fn path(c: &mut Context<'_>, value: RootId, name: &str) -> Result<FsPath, RootedError> {
    let mut path = path_as_given(c, value, name)?;
    path.io = c.host_mut().shared_state().borrow().cwd.resolve_io_path(&path.io);
    Ok(path)
}

/// A validated path kept as given, unresolved: a symbolic link's target.
pub(crate) fn path_as_given(c: &mut Context<'_>, value: RootId, name: &str) -> Result<FsPath, RootedError> {
    let value = crate::modules::fs_path::validate(c, value, name)?;
    let io = crate::modules::fs_path::native_path(c, value)?;
    let shown = crate::modules::fs_path::display(c, value)?;
    Ok(FsPath { io, shown })
}

/// `getValidatedFd(fd)`: an int32 at least zero.
pub(crate) fn fd(c: &mut Context<'_>, value: RootId) -> Result<i32, RootedError> {
    Ok(integer(c, value, "fd", 0.0, MAX_FD)? as i32)
}

/// `validateInteger(value, name, minimum, maximum)`.
pub(crate) fn integer(c: &mut Context<'_>, value: RootId, name: &str, minimum: f64, maximum: f64) -> Result<f64, RootedError> {
    let maximum = c.number(maximum);
    util::validate_integer(c, value, name, minimum, maximum)
}

/// `validateFunction(value, name)`.
pub(crate) fn function(c: &mut Context<'_>, value: RootId, name: &str) -> Result<RootId, RootedError> {
    if c.is_callable_rooted(value)? {
        return Ok(value);
    }
    Err(util::argument_type_error(c, value, name, "function")?)
}

/// `validateBoolean(value, name)` of an optional field: undefined is `default`.
pub(crate) fn boolean_option(c: &mut Context<'_>, options: RootId, name: &str, default: bool) -> Result<bool, RootedError> {
    let value = get(c, options, name)?;
    if is_undefined(c, value) {
        return Ok(default);
    }
    match c.rooted_value(value).and_then(|value| value.as_bool()) {
        Some(value) => Ok(value),
        None => Err(util::argument_type_error(c, value, &format!("options.{name}"), "boolean")?),
    }
}

/// `validateObject(value, name)` allowing null and undefined.
pub(crate) fn nullable_object(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if is_nullish(c, value) || (c.is_object_rooted(value)? && !c.is_array_rooted(value)?) {
        return Ok(());
    }
    Err(util::argument_type_error(c, value, name, "Object")?)
}

/// The access flags `open(2)` takes from Node's flag strings (`stringToFlags`).
#[cfg(unix)]
const FLAG_STRINGS: &[(&str, i32)] = &[
    ("r", libc::O_RDONLY),
    ("rs", libc::O_RDONLY | libc::O_SYNC),
    ("sr", libc::O_RDONLY | libc::O_SYNC),
    ("r+", libc::O_RDWR),
    ("rs+", libc::O_RDWR | libc::O_SYNC),
    ("sr+", libc::O_RDWR | libc::O_SYNC),
    ("w", libc::O_TRUNC | libc::O_CREAT | libc::O_WRONLY),
    ("wx", libc::O_TRUNC | libc::O_CREAT | libc::O_WRONLY | libc::O_EXCL),
    ("xw", libc::O_TRUNC | libc::O_CREAT | libc::O_WRONLY | libc::O_EXCL),
    ("w+", libc::O_TRUNC | libc::O_CREAT | libc::O_RDWR),
    ("wx+", libc::O_TRUNC | libc::O_CREAT | libc::O_RDWR | libc::O_EXCL),
    ("xw+", libc::O_TRUNC | libc::O_CREAT | libc::O_RDWR | libc::O_EXCL),
    ("a", libc::O_APPEND | libc::O_CREAT | libc::O_WRONLY),
    ("ax", libc::O_APPEND | libc::O_CREAT | libc::O_WRONLY | libc::O_EXCL),
    ("xa", libc::O_APPEND | libc::O_CREAT | libc::O_WRONLY | libc::O_EXCL),
    ("as", libc::O_APPEND | libc::O_CREAT | libc::O_WRONLY | libc::O_SYNC),
    ("sa", libc::O_APPEND | libc::O_CREAT | libc::O_WRONLY | libc::O_SYNC),
    ("a+", libc::O_APPEND | libc::O_CREAT | libc::O_RDWR),
    ("ax+", libc::O_APPEND | libc::O_CREAT | libc::O_RDWR | libc::O_EXCL),
    ("xa+", libc::O_APPEND | libc::O_CREAT | libc::O_RDWR | libc::O_EXCL),
    ("as+", libc::O_APPEND | libc::O_CREAT | libc::O_RDWR | libc::O_SYNC),
    ("sa+", libc::O_APPEND | libc::O_CREAT | libc::O_RDWR | libc::O_SYNC),
];

/// `stringToFlags(value)`: undefined is `default`, null is read-only, a number passes through.
pub(crate) fn flags(c: &mut Context<'_>, value: RootId, default: &str) -> Result<i32, RootedError> {
    if is_undefined(c, value) {
        return Ok(flag_string(default).expect("default flags are Node flag strings"));
    }
    if c.rooted_value(value).is_some_and(|value| value.is_null()) {
        return Ok(libc::O_RDONLY);
    }
    if let Some(number) = c.rooted_value(value).and_then(|value| value.as_number()) {
        return Ok(number as i32);
    }
    if let Some(flags) = c.string_text(value)?.as_deref().and_then(flag_string) {
        return Ok(flags);
    }
    Err(util::argument_value_error(c, value, "flags", "is invalid")?)
}

fn flag_string(text: &str) -> Option<i32> {
    FLAG_STRINGS.iter().find(|(name, _)| *name == text).map(|(_, flags)| *flags)
}

/// `parseFileMode(value, name, default)`: a uint32, or an octal string; without a default
/// the mode is required.
pub(crate) fn mode(c: &mut Context<'_>, value: RootId, name: &str, default: Option<u32>) -> Result<u32, RootedError> {
    if let Some(default) = default.filter(|_| is_undefined(c, value)) {
        return Ok(default);
    }
    let value = match c.string_text(value)? {
        Some(text) => {
            let octal = !text.is_empty() && text.bytes().all(|byte| (b'0'..=b'7').contains(&byte));
            if !octal {
                return Err(util::argument_value_error(c, value, name, "must be a 32-bit unsigned integer or an octal string")?);
            }
            let parsed = u64::from_str_radix(&text, OCTAL_RADIX).map(|mode| mode as f64).unwrap_or(f64::INFINITY);
            c.number(parsed)
        }
        None => value,
    };
    Ok(integer(c, value, name, 0.0, MAX_UINT32)? as u32)
}

/// A result encoding: bytes as a Buffer, or text under a canonical encoding name.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    Buffer,
    Text(&'static str),
}

pub(crate) const UTF8: Encoding = Encoding::Text("utf8");

impl Encoding {
    /// `bytes` as the value an operation returns under this encoding.
    pub(crate) fn project(self, c: &mut Context<'_>, bytes: Vec<u8>) -> Result<RootId, RootedError> {
        match self {
            Self::Buffer => crate::modules::buffer_shared_vm::from_vec(c, bytes),
            Self::Text(name) => crate::modules::buffer_binding_shared_vm::decode_bytes(c, &bytes, name),
        }
    }
}

/// `getOptions(value)`: the options object an operation reads its fields from. A string
/// names the encoding; null and undefined are no options.
pub(crate) fn options(c: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    let options = if is_nullish(c, value) {
        c.object_rooted()?
    } else if c.string_text(value)?.is_some() {
        let options = c.object_rooted()?;
        crate::modules::stream_shared_vm::put(c, options, "encoding", value)?;
        options
    } else if c.is_object_rooted(value)? {
        value
    } else {
        return Err(util::argument_type_error_with_expected(c, value, "options", "one of type string or object")?);
    };
    let encoding = get(c, options, "encoding")?;
    assert_encoding(c, encoding)?;
    let signal = get(c, options, "signal")?;
    util::validate_abort_signal(c, signal, "options.signal")?;
    Ok(options)
}

/// `assertEncoding(encoding)`: a falsy encoding or `'buffer'` is the default.
fn assert_encoding(c: &mut Context<'_>, encoding: RootId) -> Result<(), RootedError> {
    if !c.truthy_rooted(encoding)? || c.string_text(encoding)?.as_deref() == Some("buffer") {
        return Ok(());
    }
    let known = c
        .string_text(encoding)?
        .is_some_and(|name| crate::modules::buffer_codec::canonical_encoding(&name).is_some());
    if known {
        return Ok(());
    }
    Err(util::argument_value_error(c, encoding, "encoding", "is invalid encoding")?)
}

/// `options.encoding`, already asserted by `options`; absent is `default`.
pub(crate) fn encoding(c: &mut Context<'_>, options: RootId, default: Encoding) -> Result<Encoding, RootedError> {
    let value = get(c, options, "encoding")?;
    encoding_value(c, value, default)
}

pub(crate) fn encoding_value(c: &mut Context<'_>, value: RootId, default: Encoding) -> Result<Encoding, RootedError> {
    if !c.truthy_rooted(value)? {
        return Ok(default);
    }
    let name = c.to_string(value)?;
    if name == "buffer" {
        return Ok(Encoding::Buffer);
    }
    Ok(crate::modules::buffer_codec::canonical_encoding(&name).map_or(default, Encoding::Text))
}

/// `toUnixTimestamp(time, name)`: seconds since the epoch.
pub(crate) fn timestamp(c: &mut Context<'_>, value: RootId, name: &str) -> Result<f64, RootedError> {
    // `+time == time` holds for every string whose number is not NaN.
    if c.string_text(value)?.is_some() {
        let number = c.to_number(value)?;
        if !number.is_nan() {
            return Ok(number);
        }
    }
    if let Some(number) = c.rooted_value(value).and_then(|value| value.as_number()) {
        if number.is_finite() {
            return Ok(if number < 0.0 { now_seconds() } else { number });
        }
    }
    if c.has_builtin_brand_rooted(value, BuiltinBrand::Date)? {
        let time = crate::modules::stream_shared_vm::call_method(c, value, "getTime", &[])?;
        return Ok(c.to_number(time)? / MILLISECONDS_PER_SECOND);
    }
    Err(util::argument_type_error_with_expected(c, value, name, "an instance of Date or an Time in seconds")?)
}

fn now_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

/// An `ArrayBufferView`'s bytes are where a read lands or a write comes from.
pub(crate) fn is_view(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    c.has_builtin_brand_rooted(value, BuiltinBrand::ArrayBufferView)
}

/// The length in bytes of an `ArrayBufferView`.
pub(crate) fn byte_length(c: &mut Context<'_>, view: RootId) -> Result<usize, RootedError> {
    let length = get(c, view, "byteLength")?;
    Ok(c.to_number(length)? as usize)
}
