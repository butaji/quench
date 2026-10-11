//! One normalized stdio specification for OS descriptors and guest projections.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::process::Stdio;

pub(crate) const STANDARD_CHANNELS: usize = 3;

#[derive(Clone, Copy)]
pub(crate) enum Specification {
    Pipe,
    Ignore,
    Inherit,
    Descriptor(i32),
}

impl Specification {
    pub(crate) fn piped(self) -> bool {
        matches!(self, Self::Pipe)
    }

    pub(crate) fn os(self) -> std::io::Result<Stdio> {
        match self {
            Self::Pipe => Ok(Stdio::piped()),
            Self::Ignore => Ok(Stdio::null()),
            Self::Inherit => Ok(Stdio::inherit()),
            Self::Descriptor(fd) => descriptor(fd),
        }
    }
}

#[cfg(unix)]
fn descriptor(fd: i32) -> std::io::Result<Stdio> {
    use std::os::fd::{FromRawFd, OwnedFd};
    // Command owns the duplicate; the guest's original descriptor stays open.
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicate < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Stdio::from(unsafe { OwnedFd::from_raw_fd(duplicate) }))
}

#[cfg(not(unix))]
fn descriptor(_: i32) -> std::io::Result<Stdio> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "numeric child descriptors are unavailable on this platform",
    ))
}

pub(crate) fn normalize(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<[Specification; STANDARD_CHANNELS], RootedError> {
    let mut result = [Specification::Pipe; STANDARD_CHANNELS];
    let Some(options) = options else {
        return Ok(result);
    };
    let key = context.string_rooted("stdio");
    let value = context.get_property_rooted(options, key)?;
    if !context.truthy_rooted(value)? {
        return Ok(result);
    }
    if let Some(name) = context.string_text(value)? {
        let specification = match name.as_str() {
            "pipe" | "overlapped" => Specification::Pipe,
            "ignore" => Specification::Ignore,
            "inherit" => Specification::Inherit,
            _ => return Err(invalid(context, value, false)?),
        };
        return Ok([specification; STANDARD_CHANNELS]);
    }
    if !context.is_array_rooted(value)? {
        return Err(invalid(context, value, false)?);
    }
    let key = context.string_rooted("length");
    let length = context.get_property_rooted(value, key)?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default() as usize;
    for (index, specification) in result.iter_mut().enumerate() {
        let key = context.string_rooted(&index.to_string());
        let item = if index >= length {
            let undefined = context.undefined();
            context.set_property_rooted(value, key, undefined, value)?;
            undefined
        } else {
            context.get_property_rooted(value, key)?
        };
        *specification = channel(context, item)?;
    }
    Ok(result)
}

fn channel(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<Specification, RootedError> {
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(Specification::Pipe);
    }
    if let Some(name) = context.string_text(value)? {
        return match name.as_str() {
            "pipe" | "overlapped" => Ok(Specification::Pipe),
            "ignore" => Ok(Specification::Ignore),
            "inherit" => Ok(Specification::Inherit),
            _ => Err(invalid(context, value, true)?),
        };
    }
    if let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    {
        return if number < 0.0 {
            Ok(Specification::Pipe)
        } else {
            numeric_descriptor(context, value)
        };
    }
    if context.is_object_rooted(value)? {
        let key = context.string_rooted("fd");
        let fd = context.get_property_rooted(value, key)?;
        if context
            .rooted_value(fd)
            .is_some_and(|value| value.as_number().is_some())
        {
            // Node's type test and descriptor projection are separate observable gets.
            let key = context.string_rooted("fd");
            let fd = context.get_property_rooted(value, key)?;
            return numeric_descriptor(context, fd);
        }
    }
    let bytes =
        context.has_builtin_brand_rooted(value, quench_runtime::BuiltinBrand::ArrayBufferView)?;
    Err(invalid(context, value, bytes)?)
}

fn numeric_descriptor(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<Specification, RootedError> {
    let zero = context.number(0.0);
    let integer = context.bitwise_or_rooted(value, zero)?;
    let fd = context
        .rooted_value(integer)
        .and_then(|value| value.as_number())
        .expect("numeric descriptor coercion") as i32;
    Ok(Specification::Descriptor(fd))
}

fn invalid(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    input: bool,
) -> Result<RootedError, RootedError> {
    let util = crate::host::shared_vm::commonjs::util_module(context)?;
    let key = context.string_rooted("inspect");
    let inspect = context.get_property_rooted(util, key)?;
    let undefined = context.undefined();
    let text = context.call_rooted(inspect, undefined, &[value])?;
    let text = context.to_string(text)?;
    let (code, message) = if input {
        (
            "ERR_INVALID_SYNC_FORK_INPUT",
            format!(
                "Asynchronous forks do not support Buffer, TypedArray, DataView or string input: {text}"
            ),
        )
    } else {
        (
            "ERR_INVALID_ARG_VALUE",
            format!("The argument 'stdio' is invalid. Received {text}"),
        )
    };
    let error = context.type_error_rooted(&message)?;
    let key = context.string_rooted("code");
    let code = context.string_rooted(code);
    context.set_property_rooted(error, key, code, error)?;
    Ok(context.throw(error))
}
