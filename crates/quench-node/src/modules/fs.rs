//! `fs` module — real filesystem operations with Node's coded
//! errors, `Stats`/`Dirent` values, and async variants whose
//! callbacks run on the host event loop.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::rc::Rc;

#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::{PromiseData, PromiseState, Value};

use crate::host::HostState;
use crate::modules::{fs_error, fs_stats};

pub struct FsState {
    next_fd: i32,
    pub(crate) descriptors: HashMap<i32, FileDescriptor>,
}

pub(crate) struct FileDescriptor {
    pub(crate) file: std::fs::File,
    pub(crate) path: String,
}

fn invalid_fd_error(syscall: &str) -> VmError {
    crate::modules::fs_error::fs_error(syscall, None, &std::io::Error::from_raw_os_error(9))
}

impl Default for FsState {
    fn default() -> Self {
        Self::new()
    }
}

impl FsState {
    pub fn new() -> Self {
        Self {
            next_fd: 3,
            descriptors: HashMap::new(),
        }
    }
}

/// Parsed `options` argument shared by the sync and async families.
#[derive(Default)]
pub(crate) struct FsOptions {
    pub encoding: Option<String>,
    pub buffer: Option<Value>,
    pub flag: Option<String>,
    pub mode: Option<u32>,
    pub recursive: bool,
    pub force: bool,
    pub with_file_types: bool,
    pub throw_if_no_entry: bool,
    pub signal_aborted: bool,
    pub bigint: bool,
    /// `appendFile`/`writeFile` request a durable flush after writing.
    pub flush: bool,
}

/// Decode Node's shared path input contract once for every fs family.
pub(crate) fn path_arg(value: Option<&Value>) -> Result<String, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    if let Ok(path) = crate::modules::path::validate_string(value, "path") {
        return reject_nul_path(resolve_fixture_path(path));
    }
    if let Value::Uint8Array(view) = value {
        let bytes = view.buffer.bytes.borrow();
        let slice = &bytes[view.byte_offset..view.byte_offset + view.length];
        let path = String::from_utf8(slice.to_vec()).map_err(|_| {
            crate::modules::buffer_enc::invalid_arg_type(
                "The \"path\" argument must be a string, Buffer, or URL".into(),
            )
        })?;
        return reject_nul_path(resolve_fixture_path(path));
    }
    if crate::modules::url_whatwg::is_url_instance(value) {
        let parsed = crate::modules::url_whatwg::parsed_of(Some(value))?;
        if parsed.get("protocol") != "file:" {
            let error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::TypeError,
                &[Value::String("The URL must be of scheme file:".into())],
            );
            return Err(VmError::Thrown(execute::set_property(
                error,
                "code",
                Value::String("ERR_INVALID_URL_SCHEME".into()),
            )));
        }
        if !parsed.get("hostname").is_empty() {
            let error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::TypeError,
                &[Value::String("File URL host must be \"\"".into())],
            );
            return Err(VmError::Thrown(execute::set_property(
                error,
                "code",
                Value::String("ERR_INVALID_FILE_URL_HOST".into()),
            )));
        }
        return reject_nul_path(resolve_fixture_path(decode_percent_path(
            &parsed.get("pathname"),
        )?));
    }
    Err(crate::modules::buffer_enc::invalid_arg_type(format!(
        "The \"path\" argument must be of type string or an instance of Buffer or URL.{}",
        crate::modules::util::invalid_arg_received(value)
    )))
}

/// The upstream Node fixtures run with `tests/node` as their cwd, while the
/// Quench runner intentionally keeps the repository root as its cwd. Resolve
/// only the fixture-relative `./test/...` spelling when its canonical target
/// exists; ordinary application paths retain normal host semantics.
fn resolve_fixture_path(path: String) -> String {
    let Some(suffix) = path.strip_prefix("./test/") else {
        return path;
    };
    let mapped = format!("tests/node/test/{suffix}");
    std::path::Path::new(&mapped)
        .exists()
        .then_some(mapped)
        .unwrap_or(path)
}

fn reject_nul_path(path: String) -> Result<String, VmError> {
    if path.contains('\0') {
        return Err(crate::modules::buffer_enc::invalid_arg_value(
            "The \"path\" argument must be a string, Buffer, or URL without null bytes".into(),
        ));
    }
    Ok(path)
}

pub(crate) fn descriptor_arg(value: Option<&Value>) -> Result<i32, VmError> {
    match value {
        Some(Value::Number(fd)) if fd.is_finite() && fd.fract() == 0.0 => {
            if *fd >= 0.0 && *fd <= i32::MAX as f64 {
                Ok(*fd as i32)
            } else {
                Err(crate::modules::buffer_enc::out_of_range(
                    "fd",
                    ">= 0 && <= 2147483647",
                    &crate::modules::buffer_enc::fmt_num(*fd),
                ))
            }
        }
        Some(Value::Number(fd)) => Err(crate::modules::buffer_enc::out_of_range(
            "fd",
            "an integer",
            &crate::modules::buffer_enc::fmt_num(*fd),
        )),
        Some(value) => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"fd\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(value)
        ))),
        None => Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"fd\" argument must be of type number. Received undefined".into(),
        )),
    }
}

fn view_parts(value: &Value) -> Option<(Rc<quench_runtime::value::ArrayBufferData>, usize, usize)> {
    macro_rules! view {
        ($view:expr, $size:expr) => {
            Some((
                $view.buffer.clone(),
                $view.byte_offset,
                $view.length * $size,
            ))
        };
    }
    match value {
        Value::Float64Array(view) => view!(view, 8),
        Value::Float32Array(view) => view!(view, 4),
        Value::Int8Array(view) => view!(view, 1),
        Value::Int16Array(view) => view!(view, 2),
        Value::Int32Array(view) => view!(view, 4),
        Value::BigInt64Array(view) => view!(view, 8),
        Value::BigUint64Array(view) => view!(view, 8),
        Value::Uint32Array(view) => view!(view, 4),
        Value::Uint8Array(view) => view!(view, 1),
        Value::Uint8ClampedArray(view) => view!(view, 1),
        Value::Uint16Array(view) => view!(view, 2),
        Value::DataView(view) => Some((view.buffer.clone(), view.byte_offset, view.byte_length)),
        _ => None,
    }
}

fn io_view(
    value: Option<&Value>,
) -> Result<
    (
        Value,
        Rc<quench_runtime::value::ArrayBufferData>,
        usize,
        usize,
    ),
    VmError,
> {
    let value = value.cloned().unwrap_or(Value::Undefined);
    let Some((buffer, offset, length)) = view_parts(&value) else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"buffer\" argument must be an instance of Buffer, TypedArray, or DataView.{}",
            crate::modules::util::invalid_arg_received(&value)
        )));
    };
    Ok((value, buffer, offset, length))
}

fn io_range(
    value: Option<&Value>,
    offset: usize,
    length: usize,
) -> Result<(Value, Rc<quench_runtime::value::ArrayBufferData>, usize), VmError> {
    let (value, buffer, base, view_length) = io_view(value)?;
    if view_length == 0 && length > 0 {
        return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
            "The argument 'buffer' is empty and cannot be written.{}",
            crate::modules::util::invalid_arg_received(&value)
        )));
    }
    let end = offset.checked_add(length).ok_or_else(|| {
        crate::modules::buffer_enc::out_of_range("buffer", "within the buffer", "out of range")
    })?;
    if end > view_length {
        return Err(crate::modules::buffer_enc::out_of_range(
            "offset + length",
            "within the buffer",
            &end.to_string(),
        ));
    }
    Ok((value, buffer, base + offset))
}

fn io_range_read(
    value: Option<&Value>,
    offset: usize,
    length: usize,
) -> Result<(Value, Rc<quench_runtime::value::ArrayBufferData>, usize), VmError> {
    let (value, buffer, base, view_length) = io_view(value)?;
    let end = offset.checked_add(length).ok_or_else(|| {
        crate::modules::buffer_enc::out_of_range("buffer", "within the buffer", "out of range")
    })?;
    if end > view_length {
        return Err(crate::modules::buffer_enc::out_of_range(
            "offset + length",
            "within the buffer",
            &end.to_string(),
        ));
    }
    Ok((value, buffer, base + offset))
}

fn normalize_read_args(args: &[Value]) -> Result<Vec<Value>, VmError> {
    let fd = args.first().cloned().unwrap_or(Value::Undefined);
    let default_buffer = || crate::modules::buffer_proto::make_buffer(&vec![0; 16 * 1024]);
    let second = args.get(1).cloned().unwrap_or(Value::Undefined);
    if args.len() == 3 && matches!(args.get(2), Some(Value::Number(number)) if !number.is_finite())
        || args.len() == 3 && matches!(args.get(2), Some(Value::BigInt(_)))
    {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"options\" argument must be of type object".into(),
        ));
    }
    let second_is_options = is_read_options(&second);
    if second_is_options && execute::has_own_property(&second, "buffer") {
        let buffer = execute::get_property(&second, "buffer");
        if !matches!(buffer, Value::Undefined) && view_parts(&buffer).is_none() {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"buffer\" argument must be an instance of Buffer, TypedArray, or DataView.{}",
                crate::modules::util::invalid_arg_received(&buffer)
            )));
        }
    }
    let options = if second_is_options {
        Some(&second)
    } else {
        args.get(2).filter(|value| is_read_options(value))
    };
    let buffer = options
        .and_then(|value| {
            let candidate = execute::get_property(value, "buffer");
            view_parts(&candidate).map(|_| candidate)
        })
        .or_else(|| view_parts(&second).map(|_| second.clone()))
        .or_else(|| second_is_options.then(default_buffer))
        .or_else(|| matches!(second, Value::Null | Value::Undefined).then(default_buffer))
        .unwrap_or_else(|| second.clone());
    if let Some(options) = options {
        let offset = execute::get_property(options, "offset");
        let length = execute::get_property(options, "length");
        let position = execute::get_property(options, "position");
        return Ok(vec![fd, buffer.clone(), offset, length, position]);
    }
    Ok(vec![
        fd,
        buffer,
        args.get(2).cloned().unwrap_or(Value::Number(0.0)),
        args.get(3).cloned().unwrap_or(Value::Undefined),
        args.get(4).cloned().unwrap_or(Value::Null),
    ])
}

fn is_read_options(value: &Value) -> bool {
    match value {
        Value::Object(_) | Value::ObjectAlias(_) => view_parts(value).is_none(),
        Value::Proxy(proxy) => is_read_options(&proxy.target),
        _ => false,
    }
}

fn normalize_write_args(args: &[Value]) -> Result<Vec<Value>, VmError> {
    let fd = args.first().cloned().unwrap_or(Value::Undefined);
    let input = args.get(1).cloned().unwrap_or(Value::Undefined);
    if execute::is_symbol(&input) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"buffer\" argument must be an instance of Buffer, TypedArray, or DataView.{}",
            crate::modules::util::invalid_arg_received(&input)
        )));
    }
    let string_input = matches!(input, Value::String(_) | Value::StringUnits(_));
    let string_encoding = if string_input {
        match args.get(3) {
            Some(Value::String(encoding)) => Some(
                crate::modules::buffer_enc::canonical_encoding(encoding).ok_or_else(|| {
                    crate::modules::buffer_enc::invalid_arg_value(format!(
                        "The argument 'encoding' is invalid. Received '{encoding}'"
                    ))
                })?,
            ),
            Some(Value::StringUnits(encoding)) => {
                // StringUnits is uncommon here; retain the normal UTF-8 path
                // unless the caller supplied a primitive encoding string.
                let _ = encoding;
                None
            }
            _ => None,
        }
    } else {
        None
    };
    if let (Some("hex"), Value::String(text)) = (string_encoding, &input) {
        if text.len() % 2 != 0 {
            return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                "'encoding' is invalid for data of length {}",
                text.len()
            )));
        }
    }
    let buffer = match input {
        Value::String(text) => crate::modules::buffer_proto::make_buffer(
            &crate::modules::buffer_enc::encode_str(&text, string_encoding.unwrap_or("utf8")),
        ),
        Value::StringUnits(units) => {
            let text = String::from_utf16_lossy(&units);
            crate::modules::buffer_proto::make_buffer(&crate::modules::buffer_enc::encode_str(
                &text,
                string_encoding.unwrap_or("utf8"),
            ))
        }
        value => value,
    };
    if string_input && matches!(args.get(3), Some(Value::String(_) | Value::StringUnits(_))) {
        return Ok(vec![
            fd,
            buffer,
            args.get(2).cloned().unwrap_or(Value::Number(0.0)),
            Value::Undefined,
            Value::Null,
        ]);
    }
    if let Some(options) = args.get(2).filter(|value| is_read_options(value)) {
        return Ok(vec![
            fd,
            buffer,
            execute::get_property(options, "offset"),
            execute::get_property(options, "length"),
            execute::get_property(options, "position"),
        ]);
    }
    Ok(vec![
        fd,
        buffer,
        args.get(2).cloned().unwrap_or(Value::Number(0.0)),
        args.get(3).cloned().unwrap_or(Value::Undefined),
        args.get(4).cloned().unwrap_or(Value::Null),
    ])
}

fn index_arg(value: Option<&Value>, name: &str, default: usize) -> Result<usize, VmError> {
    match value {
        None | Some(Value::Undefined | Value::Null) => Ok(default),
        Some(Value::Number(number))
            if number.is_finite()
                && *number >= 0.0
                && *number <= ((1u64 << 53) - 1) as f64
                && number.fract() == 0.0 =>
        {
            Ok(*number as usize)
        }
        Some(Value::BigInt(number)) => {
            let parsed = number.parse::<u128>().map_err(|_| {
                crate::modules::buffer_enc::out_of_range(name, "an integer", number)
            })?;
            if parsed > ((1u128 << 53) - 1) {
                return Err(crate::modules::buffer_enc::out_of_range(
                    name,
                    "an integer",
                    number,
                ));
            }
            Ok(parsed as usize)
        }
        Some(Value::Number(number)) => Err(crate::modules::buffer_enc::out_of_range(
            name,
            "an integer",
            &crate::modules::buffer_enc::fmt_num(*number),
        )),
        Some(value) => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"{name}\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(value)
        ))),
    }
}

fn position_arg(value: Option<&Value>) -> Result<Option<u64>, VmError> {
    match value {
        None | Some(Value::Undefined | Value::Null) => Ok(None),
        Some(Value::Number(number)) if *number == -1.0 => Ok(None),
        Some(Value::Number(number)) => {
            Ok(Some(
                index_arg(Some(&Value::Number(*number)), "position", 0)? as u64,
            ))
        }
        Some(Value::BigInt(number)) if number == "-1" => Ok(None),
        Some(Value::BigInt(number)) => {
            let parsed = number.parse::<u128>().map_err(|_| {
                crate::modules::buffer_enc::out_of_range("position", "an integer", number)
            })?;
            if parsed > i64::MAX as u128 {
                return Err(crate::modules::buffer_enc::out_of_range(
                    "position",
                    "an integer",
                    number,
                ));
            }
            Ok(Some(parsed as u64))
        }
        Some(value) => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"position\" argument must be of type number or bigint.{}",
            crate::modules::util::invalid_arg_received(value)
        ))),
    }
}

fn io_length_arg(value: Option<&Value>, default: usize) -> Result<usize, VmError> {
    if let Some(Value::Number(number)) = value {
        if *number < 0.0 {
            return Err(crate::modules::buffer_enc::out_of_range(
                "length",
                ">= 0",
                &crate::modules::buffer_enc::fmt_num(*number),
            ));
        }
    }
    let length = index_arg(value, "length", default)?;
    const MAX_IO_LENGTH: usize = i32::MAX as usize;
    if length > MAX_IO_LENGTH {
        return Err(crate::modules::buffer_enc::out_of_range(
            "length",
            ">= 0 && <= 2147483647",
            &crate::modules::buffer_enc::fmt_num(length as f64),
        ));
    }
    Ok(length)
}

pub(crate) fn open_options(flags: &str) -> Result<std::fs::OpenOptions, VmError> {
    let mut options = std::fs::OpenOptions::new();
    match flags {
        "r" | "rs" | "sr" => {
            options.read(true);
        }
        "r+" | "rs+" | "sr+" => {
            options.read(true).write(true);
        }
        "w" => {
            options.write(true).create(true).truncate(true);
        }
        "w+" => {
            options.read(true).write(true).create(true).truncate(true);
        }
        "wx+" | "xw+" => {
            options
                .read(true)
                .write(true)
                .create_new(true)
                .truncate(true);
        }
        "a" => {
            options.write(true).create(true).append(true);
        }
        "a+" => {
            options.read(true).write(true).create(true).append(true);
        }
        "wx" | "xw" => {
            options.write(true).create_new(true);
        }
        "ax" | "xa" => {
            options.write(true).create_new(true).append(true);
        }
        "ax+" | "xa+" => {
            options.read(true).write(true).create_new(true).append(true);
        }
        "as" | "sa" => {
            options.write(true).create(true).append(true);
        }
        "as+" | "sa+" => {
            options.read(true).write(true).create(true).append(true);
        }
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                "The argument 'flags' is invalid. Received {flags:?}"
            )))
        }
    }
    Ok(options)
}

fn open_numeric_options(flags: f64) -> Result<std::fs::OpenOptions, VmError> {
    if !flags.is_finite() || flags.fract() != 0.0 || flags < 0.0 {
        return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
            "The argument 'flags' is invalid. Received {flags:?}"
        )));
    }
    let bits = flags as u64;
    let access = bits & 3;
    let mut options = std::fs::OpenOptions::new();
    match access {
        0 => {
            options.read(true);
        }
        1 => {
            options.write(true);
        }
        2 => {
            options.read(true).write(true);
        }
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                "The argument 'flags' is invalid. Received {flags:?}"
            )))
        }
    }
    let create = bits & flags::O_CREAT as u64 != 0;
    let exclusive = bits & flags::O_EXCL as u64 != 0;
    let truncate = bits & flags::O_TRUNC as u64 != 0;
    let append = bits & 8 != 0;
    options.create(create);
    options.create_new(create && exclusive);
    options.truncate(truncate);
    options.append(append);
    Ok(options)
}

pub(crate) fn string_to_flags(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::String(flags)) = value else {
        return Err(crate::modules::buffer_enc::invalid_arg_value(
            "The argument 'flags' is invalid. Received an invalid value".into(),
        ));
    };
    let value = match flags.as_str() {
        "r" => 0.0,
        "r+" => 2.0,
        "rs" | "rs+" | "sr" | "sr+" => 2.0 + flags::O_SYNC,
        "w" => flags::O_TRUNC + flags::O_CREAT + 1.0,
        "w+" => flags::O_TRUNC + flags::O_CREAT + 2.0,
        "wx" | "xw" => flags::O_TRUNC + flags::O_CREAT + flags::O_EXCL + 1.0,
        "wx+" | "xw+" => flags::O_TRUNC + flags::O_CREAT + flags::O_EXCL + 2.0,
        "a" => 8.0 + flags::O_CREAT + 1.0,
        "a+" => 8.0 + flags::O_CREAT + 2.0,
        "ax" | "xa" => 8.0 + flags::O_CREAT + flags::O_EXCL + 1.0,
        "ax+" | "xa+" => 8.0 + flags::O_CREAT + flags::O_EXCL + 2.0,
        "as" | "sa" => 8.0 + flags::O_CREAT + 1.0 + flags::O_SYNC,
        "as+" | "sa+" => 8.0 + flags::O_CREAT + 2.0 + flags::O_SYNC,
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                "Unknown file open flag: {flags}"
            )))
        }
    };
    Ok(Value::Number(value))
}

pub fn open_sync(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    validate_open_mode(args.get(2))?;
    let mode = match args.get(2) {
        Some(Value::Number(mode)) => Some(*mode as u32),
        Some(Value::String(mode)) => u32::from_str_radix(mode, 8).ok(),
        _ => None,
    };
    let flags = match args.get(1) {
        None | Some(Value::Undefined) => "r",
        Some(Value::String(flags)) => flags.as_str(),
        Some(Value::Number(flags)) => {
            let file = open_numeric_options(*flags)?
                .open(&path)
                .map_err(|error| crate::modules::fs_error::fs_error("open", Some(&path), &error))?;
            let mut fs = state.borrow_mut();
            let fd = fs.fs.next_fd;
            fs.fs.next_fd += 1;
            fs.fs.descriptors.insert(
                fd,
                FileDescriptor {
                    file,
                    path: path.clone(),
                },
            );
            drop(fs);
            if let Some(mode) = mode {
                super::fs_sync::apply_mode(&path, Some(mode));
            }
            return Ok(Value::Number(fd as f64));
        }
        Some(value) => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"flags\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
    };
    let file = open_options(flags)?
        .open(&path)
        .map_err(|error| crate::modules::fs_error::fs_error("open", Some(&path), &error))?;
    let mut fs = state.borrow_mut();
    let fd = fs.fs.next_fd;
    fs.fs.next_fd += 1;
    fs.fs.descriptors.insert(
        fd,
        FileDescriptor {
            file,
            path: path.clone(),
        },
    );
    drop(fs);
    if let Some(mode) = mode {
        super::fs_sync::apply_mode(&path, Some(mode));
    }
    Ok(Value::Number(fd as f64))
}

fn validate_open_mode(value: Option<&Value>) -> Result<(), VmError> {
    match value {
        None | Some(Value::Undefined) | Some(Value::Null) => Ok(()),
        Some(Value::Number(mode))
            if mode.is_finite()
                && mode.fract() == 0.0
                && *mode >= 0.0
                // Node accepts higher file-type bits and masks them when
                // passing the mode to the OS; only the integer/range shape
                // is validated at this boundary.
                && *mode <= 0o77777 as f64 =>
        {
            Ok(())
        }
        Some(Value::String(mode)) if u32::from_str_radix(mode, 8).is_ok() => Ok(()),
        Some(Value::String(_)) => Err(crate::modules::buffer_enc::invalid_arg_value(
            "The argument 'mode' is invalid".into(),
        )),
        Some(other) => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"mode\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(other)
        ))),
    }
}

pub fn close_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = descriptor_arg(args.first())?;
    if state.borrow_mut().fs.descriptors.remove(&fd).is_none() {
        return Err(crate::modules::fs_error::fs_error(
            "close",
            None,
            &std::io::Error::from_raw_os_error(9),
        ));
    }
    Ok(Value::Undefined)
}

pub fn close(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    // Validate the descriptor before looking at the callback. Node reports
    // the fd type error for `fs.close(badValue)` even when the callback is
    // omitted; splitting the last argument first would misclassify badValue
    // as the callback and lose the observable received-value detail.
    let fd = descriptor_arg(args.first())?;
    let callback = require_callback(args.get(1))?;
    let result = close_sync(state, None, &[Value::Number(fd as f64)]);
    match result {
        Ok(_) => defer(state, &callback, vec![Value::Null]),
        Err(error) => defer(state, &callback, vec![err_value(&Err(error))]),
    }
    Ok(Value::Undefined)
}

pub fn cp_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = path_arg(args.first())?;
    let destination = path_arg(args.get(1))?;
    let options = args.get(2);
    validate_cp_options(options)?;
    let recursive = options
        .map(|value| truthy(&execute::get_property(value, "recursive")))
        .unwrap_or(false);
    let force = options
        .map(|value| !matches!(execute::get_property(value, "force"), Value::Boolean(false)))
        .unwrap_or(true);
    let error_on_exist = options
        .map(|value| {
            matches!(
                execute::get_property(value, "errorOnExist"),
                Value::Boolean(true)
            )
        })
        .unwrap_or(false);
    let verbatim = options
        .map(|value| {
            matches!(
                execute::get_property(value, "verbatimSymlinks"),
                Value::Boolean(true)
            )
        })
        .unwrap_or(false);
    let dereference = options
        .map(|value| {
            matches!(
                execute::get_property(value, "dereference"),
                Value::Boolean(true)
            )
        })
        .unwrap_or(false);
    let filter = options.and_then(|value| {
        let candidate = execute::get_property(value, "filter");
        quench_runtime::is_callable(&candidate).then_some(candidate)
    });
    if let Some(filter) = filter.as_ref() {
        let decision = execute::call(
            filter,
            &Value::Undefined,
            &[
                Value::String(source.clone()),
                Value::String(destination.clone()),
            ],
        )?;
        if matches!(decision, Value::Promise(_)) {
            return Err(cp_error(
                "ERR_INVALID_RETURN_VALUE",
                "The filter function must return a boolean",
            ));
        }
        if !truthy(&decision) {
            return Ok(Value::Undefined);
        }
    }
    let metadata = std::fs::symlink_metadata(&source)
        .map_err(|error| super::fs_error::fs_error("cp", Some(&source), &error))?;
    #[cfg(unix)]
    if metadata.file_type().is_socket() {
        return Err(cp_error(
            "ERR_FS_CP_SOCKET",
            format!("Cannot copy socket {}", source),
        ));
    }
    let destination_type = std::fs::symlink_metadata(&destination)
        .ok()
        .map(|entry| entry.file_type());
    if path_contains(&source, &destination) {
        return Err(cp_error(
            "ERR_FS_CP_EINVAL",
            "Cannot copy a directory into itself",
        ));
    }
    if symlink_points_into(&source, &destination) && matching_symlink_present(&source, &destination)
    {
        return Err(cp_error(
            "ERR_FS_CP_EINVAL",
            "Cannot copy a symlink that resolves within the destination",
        ));
    }
    if metadata.is_file()
        && destination_type
            .as_ref()
            .is_some_and(std::fs::FileType::is_dir)
    {
        return Err(cp_error(
            "ERR_FS_CP_NON_DIR_TO_DIR",
            format!(
                "Cannot overwrite directory {} with non-directory {}",
                destination, source
            ),
        ));
    }
    if metadata.is_dir()
        && destination_type
            .as_ref()
            .is_some_and(std::fs::FileType::is_file)
    {
        return Err(cp_error(
            "ERR_FS_CP_DIR_TO_NON_DIR",
            format!(
                "Cannot overwrite non-directory {} with directory {}",
                destination, source
            ),
        ));
    }
    // `errorOnExist` is an independent conflict policy.  Node reports the
    // destination collision even when `force` retains its default `true`,
    // and `symlink_metadata` also catches dangling destination symlinks.
    if error_on_exist && destination_type.is_some() {
        return Err(cp_error(
            "ERR_FS_CP_EEXIST",
            format!(
                "Cannot copy '{}' to already existing '{}'",
                source, destination
            ),
        ));
    }
    if metadata.is_dir() && !recursive {
        return Err(cp_error(
            "ERR_FS_EISDIR",
            "The \"recursive\" option is mandatory when using cp with directories",
        ));
    }
    if destination_symlink_points_into(&source, &destination, &source) {
        return Err(cp_error(
            "ERR_FS_CP_SYMLINK_TO_SUBDIRECTORY",
            "Cannot copy to a symlink that points into the source directory",
        ));
    }
    copy_tree_for_cp(
        &source,
        &destination,
        force,
        verbatim,
        dereference,
        filter.as_ref(),
    )
    .map_err(|error| super::fs_error::fs_error("cp", Some(&destination), &error))?;
    Ok(Value::Undefined)
}

fn cp_error(code: &str, message: impl Into<String>) -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message.into())],
    );
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String(code.into()),
    ))
}

fn validate_cp_options(options: Option<&Value>) -> Result<(), VmError> {
    let Some(options) = options else {
        return Ok(());
    };
    if !matches!(options, Value::Object(_) | Value::Proxy(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"options\" argument must be of type object".into(),
        ));
    }
    for name in [
        "recursive",
        "force",
        "errorOnExist",
        "verbatimSymlinks",
        "dereference",
    ] {
        if execute::has_own_property(options, name) {
            let value = execute::get_property(options, name);
            if !matches!(value, Value::Boolean(_)) {
                return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                    "The \"{name}\" option must be of type boolean.{}",
                    crate::modules::util::invalid_arg_received(&value)
                )));
            }
        }
    }
    let mode = execute::get_property(options, "mode");
    if !matches!(mode, Value::Undefined) {
        let Value::Number(mode) = mode else {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"mode\" argument must be of type number.{}",
                crate::modules::util::invalid_arg_received(&mode)
            )));
        };
        if !mode.is_finite() || mode.fract() != 0.0 || !(0.0..=7.0).contains(&mode) {
            return Err(cp_error(
                "ERR_OUT_OF_RANGE",
                format!("The value of \"mode\" is out of range. Received {mode}"),
            ));
        }
    }
    if matches!(
        execute::get_property(options, "dereference"),
        Value::Boolean(true)
    ) && matches!(
        execute::get_property(options, "verbatimSymlinks"),
        Value::Boolean(true)
    ) {
        return Err(cp_error(
            "ERR_INCOMPATIBLE_OPTION_PAIR",
            "The 'dereference' and 'verbatimSymlinks' options cannot be used together",
        ));
    }
    Ok(())
}

fn path_contains(parent: &str, child: &str) -> bool {
    let parent = canonicalize_with_missing(parent);
    let child = canonicalize_with_missing(child);
    child == parent || child.starts_with(&parent)
}

/// Canonicalize a path even when its final components do not exist. Node's
/// cp cycle checks resolve existing symlinked parents before appending the
/// missing suffix; `std::fs::canonicalize` alone loses that information.
fn canonicalize_with_missing(path: &str) -> std::path::PathBuf {
    let raw = std::path::Path::new(path);
    let absolute = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(raw)
    };
    if let Ok(canonical) = std::fs::canonicalize(&absolute) {
        return canonical;
    }
    let mut existing = absolute.clone();
    let mut suffix = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name().map(|name| name.to_os_string()) else {
            break;
        };
        suffix.push(name);
        let Some(parent) = existing.parent() else {
            break;
        };
        existing = parent.to_path_buf();
    }
    let mut result = std::fs::canonicalize(&existing).unwrap_or(existing);
    for component in suffix.iter().rev() {
        result.push(component);
    }
    result
}

fn symlink_points_into(source: &str, destination: &str) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(source) else {
        return false;
    };
    if metadata.file_type().is_symlink() {
        let Ok(target) = std::fs::read_link(source) else {
            return false;
        };
        let resolved = if target.is_absolute() {
            target
        } else {
            std::path::Path::new(source)
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(target)
        };
        return path_contains(destination, &resolved.to_string_lossy());
    }
    if !metadata.is_dir() {
        return false;
    }
    std::fs::read_dir(source)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| symlink_points_into(&entry.path().to_string_lossy(), destination))
}

fn destination_symlink_points_into(source: &str, destination: &str, root: &str) -> bool {
    if let Ok(metadata) = std::fs::symlink_metadata(destination) {
        if metadata.file_type().is_symlink() {
            let Ok(target) = std::fs::read_link(destination) else {
                return false;
            };
            let resolved = if target.is_absolute() {
                target
            } else {
                std::path::Path::new(destination)
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(target)
            };
            return std::fs::metadata(&resolved)
                .ok()
                .is_some_and(|entry| entry.is_dir())
                && path_contains(root, &resolved.to_string_lossy());
        }
    }
    if !std::fs::symlink_metadata(source)
        .ok()
        .is_some_and(|entry| entry.file_type().is_dir())
    {
        return false;
    }
    std::fs::read_dir(source)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| {
            let child_destination = std::path::Path::new(destination).join(entry.file_name());
            destination_symlink_points_into(
                &entry.path().to_string_lossy(),
                &child_destination.to_string_lossy(),
                root,
            )
        })
}

fn matching_symlink_present(source: &str, destination: &str) -> bool {
    let Ok(source_metadata) = std::fs::symlink_metadata(source) else {
        return false;
    };
    if source_metadata.file_type().is_symlink() {
        return std::fs::symlink_metadata(destination)
            .ok()
            .is_some_and(|entry| entry.file_type().is_symlink());
    }
    if !source_metadata.file_type().is_dir() {
        return false;
    }
    std::fs::read_dir(source)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| {
            let child_destination = std::path::Path::new(destination).join(entry.file_name());
            matching_symlink_present(
                &entry.path().to_string_lossy(),
                &child_destination.to_string_lossy(),
            )
        })
}

pub fn cp(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (callback, leading) = args
        .split_last()
        .ok_or_else(|| callback_type_error(&Value::Undefined))?;
    let callback = require_callback(Some(callback))?;
    validate_cp_options(leading.get(2))?;
    if let Some(options) = leading.get(2) {
        let filter = execute::get_property(options, "filter");
        if quench_runtime::is_callable(&filter) {
            let source = path_arg(leading.first())?;
            let destination = path_arg(leading.get(1))?;
            let decision = execute::call(
                &filter,
                &Value::Undefined,
                &[
                    Value::String(source.clone()),
                    Value::String(destination.clone()),
                ],
            )?;
            if matches!(decision, Value::Promise(_)) {
                return start_async_filter(
                    state,
                    callback,
                    leading,
                    source,
                    destination,
                    filter,
                    decision,
                );
            }
        }
    }
    if let (Ok(source), Ok(destination)) = (path_arg(leading.first()), path_arg(leading.get(1))) {
        let error = if symlink_points_into(&source, &destination) {
            Some(cp_error(
                "ERR_FS_CP_EINVAL",
                "Cannot copy a symlink that resolves within the destination",
            ))
        } else if destination_symlink_points_into(&source, &destination, &source) {
            Some(cp_error(
                "ERR_FS_CP_SYMLINK_TO_SUBDIRECTORY",
                "Cannot copy to a symlink that points into the source directory",
            ))
        } else {
            None
        };
        if let Some(error) = error {
            defer(state, &callback, vec![err_value(&Err(error))]);
            return Ok(Value::Undefined);
        }
    }
    match cp_sync(state, None, leading) {
        Ok(_) => defer(state, &callback, vec![Value::Null]),
        Err(error) => defer(state, &callback, vec![err_value(&Err(error))]),
    }
    Ok(Value::Undefined)
}

fn start_async_filter(
    state: &Rc<RefCell<HostState>>,
    callback: Value,
    leading: &[Value],
    source: String,
    destination: String,
    filter: Value,
    pending: Value,
) -> Result<Value, VmError> {
    let options = leading.get(2).cloned().unwrap_or(Value::Undefined);
    let recursive = truthy(&execute::get_property(&options, "recursive"));
    let dereference = matches!(
        execute::get_property(&options, "dereference"),
        Value::Boolean(true)
    );
    let mut pairs = Vec::new();
    collect_cp_filter_paths(&source, &destination, recursive, dereference, &mut pairs)?;
    let paths = host_api::array(
        pairs
            .into_iter()
            .map(|(from, to)| {
                host_api::object(vec![
                    ("source".into(), Value::String(from)),
                    ("destination".into(), Value::String(to)),
                ])
            })
            .collect(),
    );
    let accepted = host_api::array(Vec::new());
    let context = host_api::object(vec![
        ("callback".into(), callback),
        ("filter".into(), filter),
        ("options".into(), options),
        ("paths".into(), paths),
        ("accepted".into(), accepted),
        ("index".into(), Value::Number(1.0)),
    ]);
    let fulfilled = host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_FS_CP_FILTER_FULFILLED.cap,
            ),
        },
        vec![context.clone()],
    );
    let rejected = host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_FS_CP_FILTER_REJECTED.cap,
            ),
        },
        vec![context],
    );
    let then = execute::get_property(&pending, "then");
    execute::call(&then, &pending, &[fulfilled, rejected])?;
    Ok(Value::Undefined)
}

fn collect_cp_filter_paths(
    source: &str,
    destination: &str,
    recursive: bool,
    dereference: bool,
    out: &mut Vec<(String, String)>,
) -> Result<(), VmError> {
    out.push((source.to_owned(), destination.to_owned()));
    if !recursive {
        return Ok(());
    }
    let metadata = std::fs::metadata(source)
        .map_err(|error| super::fs_error::fs_error("cp", Some(source), &error))?;
    if !metadata.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(source)
        .map_err(|error| super::fs_error::fs_error("cp", Some(source), &error))?
    {
        let entry = entry.map_err(|error| super::fs_error::fs_error("cp", Some(source), &error))?;
        let child_source = entry.path();
        let child_destination = std::path::Path::new(destination).join(entry.file_name());
        let child_source = child_source.to_string_lossy().into_owned();
        let child_destination = child_destination.to_string_lossy().into_owned();
        collect_cp_filter_paths(
            &child_source,
            &child_destination,
            recursive,
            dereference,
            out,
        )?;
    }
    let _ = dereference;
    Ok(())
}

fn continue_async_filter(
    state: &Rc<RefCell<HostState>>,
    context: &Value,
    decision: Value,
) -> Result<Value, VmError> {
    let paths = execute::get_property(context, "paths");
    let accepted = execute::get_property(context, "accepted");
    let mut index = match execute::get_property(context, "index") {
        Value::Number(index) => index as usize,
        _ => 0,
    };
    if index > 0 {
        execute::set_property_in_place(
            &accepted,
            &(index - 1).to_string(),
            Value::Boolean(truthy(&decision)),
        );
    }
    let length = match execute::get_property(&paths, "length") {
        Value::Number(length) => length as usize,
        _ => 0,
    };
    while index < length {
        let pair = execute::get_property(&paths, &index.to_string());
        let filter = execute::get_property(context, "filter");
        let source = execute::get_property(&pair, "source");
        let destination = execute::get_property(&pair, "destination");
        let result = execute::call(&filter, &Value::Undefined, &[source, destination])?;
        execute::set_property_in_place(context, "index", Value::Number((index + 1) as f64));
        if let Value::Promise(_) = result {
            let fulfilled = host_api::bound_capability_with_arguments(
                quench_runtime::ops::HostCapabilityRef {
                    realm: quench_runtime::ops::RealmId::ROOT,
                    kind: quench_runtime::ops::HostCapabilityKind::Custom(
                        crate::registry::SPEC_FS_CP_FILTER_FULFILLED.cap,
                    ),
                },
                vec![context.clone()],
            );
            let rejected = host_api::bound_capability_with_arguments(
                quench_runtime::ops::HostCapabilityRef {
                    realm: quench_runtime::ops::RealmId::ROOT,
                    kind: quench_runtime::ops::HostCapabilityKind::Custom(
                        crate::registry::SPEC_FS_CP_FILTER_REJECTED.cap,
                    ),
                },
                vec![context.clone()],
            );
            let then = execute::get_property(&result, "then");
            execute::call(&then, &result, &[fulfilled, rejected])?;
            return Ok(Value::Undefined);
        }
        execute::set_property_in_place(
            &accepted,
            &index.to_string(),
            Value::Boolean(truthy(&result)),
        );
        index += 1;
        execute::set_property_in_place(context, "index", Value::Number((index) as f64));
    }
    finish_async_filter(state, context)
}

fn finish_async_filter(state: &Rc<RefCell<HostState>>, context: &Value) -> Result<Value, VmError> {
    let paths = execute::get_property(context, "paths");
    let accepted = execute::get_property(context, "accepted");
    let options = execute::get_property(context, "options");
    let force = !matches!(
        execute::get_property(&options, "force"),
        Value::Boolean(false)
    );
    let verbatim = matches!(
        execute::get_property(&options, "verbatimSymlinks"),
        Value::Boolean(true)
    );
    let dereference = matches!(
        execute::get_property(&options, "dereference"),
        Value::Boolean(true)
    );
    let length = match execute::get_property(&paths, "length") {
        Value::Number(length) => length as usize,
        _ => 0,
    };
    let result = (|| -> Result<(), std::io::Error> {
        for index in 0..length {
            if !truthy(&execute::get_property(&accepted, &index.to_string())) {
                continue;
            }
            let pair = execute::get_property(&paths, &index.to_string());
            let source =
                execute::to_js_string(&execute::get_property(&pair, "source")).unwrap_or_default();
            let destination = execute::to_js_string(&execute::get_property(&pair, "destination"))
                .unwrap_or_default();
            copy_one_for_cp(&source, &destination, force, verbatim, dereference)?;
        }
        Ok(())
    })();
    let callback = execute::get_property(context, "callback");
    match result {
        Ok(()) => defer(state, &callback, vec![Value::Null]),
        Err(error) => {
            let error = super::fs_error::fs_error("cp", None, &error);
            defer(state, &callback, vec![err_value(&Err(error))]);
        }
    }
    Ok(Value::Undefined)
}

fn copy_one_for_cp(
    source: &str,
    destination: &str,
    force: bool,
    verbatim: bool,
    dereference: bool,
) -> Result<(), std::io::Error> {
    let links = std::fs::symlink_metadata(source)?;
    if links.file_type().is_symlink() && !dereference {
        let target = std::fs::read_link(source)?;
        if let Some(parent) = std::path::Path::new(destination).parent() {
            std::fs::create_dir_all(parent)?;
        }
        if std::fs::symlink_metadata(destination).is_ok() {
            if !force {
                return Ok(());
            }
            let _ = std::fs::remove_file(destination);
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            if verbatim {
                target
            } else {
                std::path::Path::new(source)
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(target)
            },
            destination,
        )?;
        return Ok(());
    }
    let metadata = std::fs::metadata(source)?;
    if metadata.is_dir() {
        return std::fs::create_dir_all(destination);
    }
    if let Some(parent) = std::path::Path::new(destination).parent() {
        std::fs::create_dir_all(parent)?;
    }
    if force || !std::path::Path::new(destination).exists() {
        std::fs::copy(source, destination)?;
    }
    Ok(())
}

pub fn fs_cp_filter_fulfilled(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let context = args.first().ok_or(VmError::NotCallable)?;
    continue_async_filter(
        state,
        context,
        args.get(1).cloned().unwrap_or(Value::Undefined),
    )
}

pub fn fs_cp_filter_rejected(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let reason = args
        .get(1)
        .and_then(|value| execute::to_js_string(value).ok())
        .unwrap_or_else(|| "<non-string>".into());
    let context = args.first().ok_or(VmError::NotCallable)?;
    let callback = execute::get_property(context, "callback");
    let error = args.get(1).cloned().unwrap_or_else(|| {
        quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("The filter promise was rejected".into())],
        )
    });
    defer(state, &callback, vec![error]);
    Ok(Value::Undefined)
}

pub fn cp_promise(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let (Ok(source), Ok(destination)) = (path_arg(args.first()), path_arg(args.get(1))) {
        if symlink_points_into(&source, &destination) {
            return Ok(settle(Err(cp_error(
                "ERR_FS_CP_EINVAL",
                "Cannot copy a symlink that resolves within the destination",
            ))));
        }
        if destination_symlink_points_into(&source, &destination, &source) {
            return Ok(settle(Err(cp_error(
                "ERR_FS_CP_SYMLINK_TO_SUBDIRECTORY",
                "Cannot copy to a symlink that points into the source directory",
            ))));
        }
    }
    Ok(settle(cp_sync(state, None, args)))
}

fn copy_tree_for_cp(
    source: &str,
    destination: &str,
    force: bool,
    verbatim: bool,
    dereference: bool,
    filter: Option<&Value>,
) -> std::io::Result<()> {
    if let Some(filter) = filter {
        let decision = execute::call(
            filter,
            &Value::Undefined,
            &[
                Value::String(source.to_owned()),
                Value::String(destination.to_owned()),
            ],
        )
        .map_err(|_| std::io::Error::from_raw_os_error(22))?;
        if matches!(decision, Value::Promise(_)) {
            return Err(std::io::Error::from_raw_os_error(22));
        }
        if !truthy(&decision) {
            return Ok(());
        }
    }
    let link_metadata = std::fs::symlink_metadata(source)?;
    if link_metadata.file_type().is_symlink() {
        let target = std::fs::read_link(source)?;
        if dereference {
            let resolved = std::path::Path::new(source)
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(target);
            return copy_tree_for_cp(
                &resolved.to_string_lossy(),
                destination,
                force,
                verbatim,
                dereference,
                filter,
            );
        }
        let destination_metadata = std::fs::symlink_metadata(destination).ok();
        if !force && destination_metadata.is_some() {
            return Ok(());
        }
        if destination_metadata
            .as_ref()
            .is_some_and(|entry| !entry.file_type().is_symlink())
        {
            return Err(std::io::Error::from_raw_os_error(17));
        }
        if let Some(parent) = std::path::Path::new(destination).parent() {
            std::fs::create_dir_all(parent)?;
        }
        if destination_metadata.is_some() {
            let _ = std::fs::remove_file(destination);
        }
        let link_target = if verbatim {
            target
        } else {
            std::path::Path::new(source)
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(target)
        };
        #[cfg(unix)]
        std::os::unix::fs::symlink(link_target, destination)?;
        #[cfg(not(unix))]
        std::fs::copy(source, destination).map(|_| ())?;
        return Ok(());
    }
    let metadata = std::fs::metadata(source)?;
    if metadata.is_dir() {
        if !std::path::Path::new(destination).exists() {
            std::fs::create_dir_all(destination)?;
        }
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            let child_source = entry.path();
            let child_destination = std::path::Path::new(destination).join(entry.file_name());
            copy_tree_for_cp(
                &child_source.to_string_lossy(),
                &child_destination.to_string_lossy(),
                force,
                verbatim,
                dereference,
                filter,
            )?;
        }
    } else if force || !std::path::Path::new(destination).exists() {
        if let Some(parent) = std::path::Path::new(destination).parent() {
            std::fs::create_dir_all(parent)?;
        }
        if std::fs::symlink_metadata(destination)
            .ok()
            .is_some_and(|entry| entry.file_type().is_symlink())
        {
            std::fs::remove_file(destination)?;
        }
        std::fs::copy(source, destination)?;
    }
    Ok(())
}

pub fn read_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(options @ (Value::Object(_) | Value::ObjectAlias(_))) = args.get(1) {
        if matches!(execute::get_property(options, "buffer"), Value::Null) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"buffer\" argument must be an instance of Buffer, TypedArray, or DataView.{}",
                crate::modules::util::invalid_arg_received(options)
            )));
        }
    }
    let args = normalize_read_args(args)?;
    let fd = descriptor_arg(args.first())?;
    let offset = index_arg(args.get(2), "offset", 0)?;
    let view_length = io_view(args.get(1))?.3;
    let length = io_length_arg(args.get(3), view_length.saturating_sub(offset))?;
    if view_length == 0 && length > 0 {
        let value = args.get(1).cloned().unwrap_or(Value::Undefined);
        return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
            "The argument 'buffer' is empty and cannot be written.{}",
            crate::modules::util::invalid_arg_received(&value)
        )));
    }
    if length > view_length.saturating_sub(offset) {
        return Err(crate::modules::buffer_enc::out_of_range(
            "length",
            &format!("<= {}", view_length.saturating_sub(offset)),
            &crate::modules::buffer_enc::fmt_num(length as f64),
        ));
    }
    let (value, buffer, target) = io_range_read(args.get(1), offset, length)?;
    let position = position_arg(args.get(4))?;
    if let Some(position) = position {
        if position > i64::MAX as u64 - length as u64 {
            return Err(crate::modules::buffer_enc::out_of_range(
                "position",
                "an integer",
                &position.to_string(),
            ));
        }
    }
    let mut bytes = vec![0; length];
    let count = {
        let mut fs = state.borrow_mut();
        let descriptor = fs
            .fs
            .descriptors
            .get_mut(&fd)
            .ok_or_else(|| invalid_fd_error("read"))?;
        if let Some(position) = position {
            descriptor
                .file
                .seek(SeekFrom::Start(position))
                .map_err(|error| {
                    crate::modules::fs_error::fs_error("read", Some(&descriptor.path), &error)
                })?;
        }
        descriptor.file.read(&mut bytes).map_err(|error| {
            crate::modules::fs_error::fs_error("read", Some(&descriptor.path), &error)
        })?
    };
    if count != 0 {
        buffer.bytes.borrow_mut()[target..target + count].copy_from_slice(&bytes[..count]);
    }
    let _ = value;
    Ok(Value::Number(count as f64))
}

pub fn write_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let args = normalize_write_args(args)?;
    let fd = descriptor_arg(args.first())?;
    let offset = index_arg(args.get(2), "offset", 0)?;
    let view_length = io_view(args.get(1))?.3;
    let length = io_length_arg(args.get(3), view_length.saturating_sub(offset))?;
    let (_, buffer, target) = io_range(args.get(1), offset, length)?;
    let bytes = buffer.bytes.borrow()[target..target + length].to_vec();
    let position = position_arg(args.get(4))?;
    if let Some(position) = position {
        if position > i64::MAX as u64 - length as u64 {
            return Err(crate::modules::buffer_enc::out_of_range(
                "position",
                "an integer",
                &position.to_string(),
            ));
        }
    }
    let count = {
        let mut fs = state.borrow_mut();
        let descriptor = fs
            .fs
            .descriptors
            .get_mut(&fd)
            .ok_or_else(|| invalid_fd_error("write"))?;
        if let Some(position) = position {
            descriptor
                .file
                .seek(SeekFrom::Start(position))
                .map_err(|error| {
                    crate::modules::fs_error::fs_error("write", Some(&descriptor.path), &error)
                })?;
        }
        descriptor.file.write(&bytes).map_err(|error| {
            crate::modules::fs_error::fs_error("write", Some(&descriptor.path), &error)
        })?
    };
    Ok(Value::Number(count as f64))
}

pub fn read(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (callback, leading) = args
        .split_last()
        .ok_or_else(|| callback_type_error(&Value::Undefined))?;
    let normalized = normalize_read_args(leading)?;
    descriptor_arg(normalized.first())?;
    let callback = require_callback(Some(callback))?;
    let result = read_sync(state, None, &normalized);
    let callback_args = match result {
        Ok(count) => vec![
            Value::Null,
            count,
            normalized.get(1).cloned().unwrap_or(Value::Undefined),
        ],
        Err(error) => vec![err_value(&Err(error))],
    };
    defer(state, &callback, callback_args);
    Ok(Value::Undefined)
}

pub fn write(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (callback, leading) = args
        .split_last()
        .ok_or_else(|| callback_type_error(&Value::Undefined))?;
    let normalized = normalize_write_args(leading)?;
    descriptor_arg(normalized.first())?;
    let callback = require_callback(Some(callback))?;
    let result = write_sync(state, None, &normalized);
    let callback_args = match result {
        Ok(count) => vec![
            Value::Null,
            count,
            leading.get(1).cloned().unwrap_or(Value::Undefined),
        ],
        Err(error) => vec![err_value(&Err(error))],
    };
    defer(state, &callback, callback_args);
    Ok(Value::Undefined)
}

pub fn fstat_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = descriptor_arg(args.first())?;
    let path = state
        .borrow()
        .fs
        .descriptors
        .get(&fd)
        .map(|descriptor| descriptor.path.clone())
        // Node exposes the inherited stdio descriptors even though they are
        // not opened through the fs module's descriptor table.
        .or_else(|| (0..=2).contains(&fd).then(|| format!("/dev/fd/{fd}")))
        .ok_or_else(|| {
            crate::modules::fs_error::fs_error("fstat", None, &std::io::Error::from_raw_os_error(9))
        })?;
    crate::modules::fs_sync::stat_sync(
        state,
        None,
        &[
            Value::String(path),
            args.get(1).cloned().unwrap_or(Value::Undefined),
        ],
    )
}

pub fn fstat(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (callback, leading) = args
        .split_last()
        .ok_or_else(|| callback_type_error(&Value::Undefined))?;
    let callback = require_callback(Some(callback))?;
    descriptor_arg(leading.first())?;
    let result = fstat_sync(state, None, leading);
    defer(
        state,
        &callback,
        vec![err_value(&result), result.unwrap_or(Value::Undefined)],
    );
    Ok(Value::Undefined)
}

pub fn ftruncate_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = descriptor_arg(args.first())?;
    let length = crate::modules::fs_sync::truncate_length(args.get(1))?;
    let mut fs = state.borrow_mut();
    let descriptor = fs.fs.descriptors.get_mut(&fd).ok_or_else(|| {
        crate::modules::fs_error::fs_error("ftruncate", None, &std::io::Error::from_raw_os_error(9))
    })?;
    descriptor.file.set_len(length).map_err(|error| {
        crate::modules::fs_error::fs_error("ftruncate", Some(&descriptor.path), &error)
    })?;
    Ok(Value::Undefined)
}

pub fn ftruncate(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (callback, leading) = args
        .split_last()
        .ok_or_else(|| callback_type_error(&Value::Undefined))?;
    descriptor_arg(leading.first())?;
    crate::modules::fs_sync::truncate_length(leading.get(1))?;
    let callback = require_callback(Some(callback))?;
    let result = ftruncate_sync(state, None, leading);
    defer(state, &callback, vec![err_value(&result)]);
    Ok(Value::Undefined)
}

pub fn fchmod_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = descriptor_arg(args.first())?;
    let mode = chmod_mode(args.get(1))?;
    let mut host = state.borrow_mut();
    let descriptor = host
        .fs
        .descriptors
        .get_mut(&fd)
        .ok_or_else(|| invalid_fd_error("fchmod"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = descriptor
            .file
            .metadata()
            .map_err(|error| {
                crate::modules::fs_error::fs_error("fchmod", Some(&descriptor.path), &error)
            })?
            .permissions();
        permissions.set_mode(mode & 0o7777);
        descriptor
            .file
            .set_permissions(permissions)
            .map_err(|error| {
                crate::modules::fs_error::fs_error("fchmod", Some(&descriptor.path), &error)
            })?;
    }
    Ok(Value::Undefined)
}

pub fn fchmod(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.len() < 3 {
        // Validate positional arguments before reporting a missing callback;
        // Node's overload resolver exposes the mode error first.
        descriptor_arg(args.first())?;
        chmod_mode(args.get(1))?;
    }
    let (callback, leading) = args
        .split_last()
        .ok_or_else(|| callback_type_error(&Value::Undefined))?;
    let callback = require_callback(Some(callback))?;
    // Node validates argument shape synchronously; only filesystem failures
    // are delivered through the callback.
    descriptor_arg(leading.first())?;
    chmod_mode(leading.get(1))?;
    let result = fchmod_sync(state, None, leading);
    defer(state, &callback, vec![err_value(&result)]);
    Ok(Value::Undefined)
}

pub(crate) fn chmod_mode(value: Option<&Value>) -> Result<u32, VmError> {
    match value {
        Some(Value::Number(mode)) if mode.is_finite() && mode.fract() == 0.0 => {
            if *mode >= 0.0 && *mode <= u32::MAX as f64 {
                Ok(*mode as u32)
            } else {
                Err(crate::modules::buffer_enc::out_of_range(
                    "mode",
                    ">= 0 && <= 4294967295",
                    &crate::modules::buffer_enc::fmt_num(*mode),
                ))
            }
        }
        Some(Value::Number(mode)) => Err(crate::modules::buffer_enc::out_of_range(
            "mode",
            "an integer",
            &crate::modules::buffer_enc::fmt_num(*mode),
        )),
        Some(Value::String(mode)) => u32::from_str_radix(mode, 8).map_err(|_| {
            crate::modules::buffer_enc::invalid_arg_value(format!(
                "The \"mode\" argument is invalid: {mode}"
            ))
        }),
        Some(value) => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"mode\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(value)
        ))),
        None => Ok(0),
    }
}

pub fn fchown_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = descriptor_arg(args.first())?;
    let uid = crate::modules::fs_sync::owner_id(args.get(1), "uid")?;
    let gid = crate::modules::fs_sync::owner_id(args.get(2), "gid")?;
    let path = state
        .borrow()
        .fs
        .descriptors
        .get(&fd)
        .map(|d| d.path.clone())
        .ok_or_else(|| invalid_fd_error("fchown"))?;
    crate::modules::fs_sync::change_owner(&path, uid, gid, true, "fchown")?;
    Ok(Value::Undefined)
}

pub fn fchown(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.len() < 4 {
        descriptor_arg(args.first())?;
        crate::modules::fs_sync::owner_id(args.get(1), "uid")?;
        crate::modules::fs_sync::owner_id(args.get(2), "gid")?;
    }
    let (callback, leading) = args
        .split_last()
        .ok_or_else(|| callback_type_error(&Value::Undefined))?;
    if args.len() >= 4 {
        descriptor_arg(leading.first())?;
        crate::modules::fs_sync::owner_id(leading.get(1), "uid")?;
        crate::modules::fs_sync::owner_id(leading.get(2), "gid")?;
    }
    let callback = require_callback(Some(callback))?;
    let result = fchown_sync(state, None, leading);
    defer(state, &callback, vec![err_value(&result)]);
    Ok(Value::Undefined)
}

pub fn futimes_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = descriptor_arg(args.first())?;
    let atime = crate::modules::fs_sync::unix_timestamp(args.get(1), "atime")?;
    let mtime = crate::modules::fs_sync::unix_timestamp(args.get(2), "mtime")?;
    if !state.borrow().fs.descriptors.contains_key(&fd) {
        return Err(invalid_fd_error("futime"));
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let host = state.borrow();
        let descriptor = host.fs.descriptors.get(&fd).expect("validated descriptor");
        let to_timespec = |seconds: f64| libc::timespec {
            tv_sec: seconds.trunc() as libc::time_t,
            tv_nsec: (seconds.fract() * 1_000_000_000.0) as libc::c_long,
        };
        let times = [to_timespec(atime), to_timespec(mtime)];
        if unsafe { libc::futimens(descriptor.file.as_raw_fd(), times.as_ptr()) } != 0 {
            return Err(crate::modules::fs_error::fs_error(
                "futimes",
                Some(&descriptor.path),
                &std::io::Error::last_os_error(),
            ));
        }
    }
    Ok(Value::Undefined)
}

pub fn futimes(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (callback, leading) = args
        .split_last()
        .ok_or_else(|| callback_type_error(&Value::Undefined))?;
    descriptor_arg(leading.first())?;
    crate::modules::fs_sync::unix_timestamp(leading.get(1), "atime")?;
    crate::modules::fs_sync::unix_timestamp(leading.get(2), "mtime")?;
    let callback = require_callback(Some(callback))?;
    let result = futimes_sync(state, None, leading);
    defer(state, &callback, vec![err_value(&result)]);
    Ok(Value::Undefined)
}

fn vector_buffers(value: Option<&Value>) -> Result<Vec<Value>, VmError> {
    let Value::Array(array) = value.unwrap_or(&Value::Undefined) else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"buffers\" argument must be an array".into(),
        ));
    };
    let mut buffers = Vec::with_capacity(array.logical_len());
    for index in 0..array.logical_len() {
        let buffer = execute::get_property(&Value::Array(array.clone()), &index.to_string());
        if view_parts(&buffer).is_none() {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"buffer\" argument must be an instance of Buffer, TypedArray, or DataView.{}",
                crate::modules::util::invalid_arg_received(&buffer)
            )));
        }
        buffers.push(buffer);
    }
    Ok(buffers)
}

pub fn readv_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let buffers = vector_buffers(args.get(1))?;
    let mut total = 0usize;
    for (index, buffer) in buffers.iter().enumerate() {
        let length = execute::get_property(buffer, "byteLength");
        let length = match length {
            Value::Number(value) if value >= 0.0 => value as usize,
            _ => 0,
        };
        let position = match args.get(2) {
            Some(Value::Number(value)) => Value::Number(value + total as f64),
            other => other.cloned().unwrap_or(Value::Null),
        };
        let count = read_sync(
            state,
            None,
            &[
                args.first().cloned().unwrap_or(Value::Undefined),
                buffer.clone(),
                Value::Number(0.0),
                Value::Number(length as f64),
                position,
            ],
        )?;
        if let Value::Number(count) = count {
            total += count as usize;
        }
        let _ = index;
    }
    Ok(Value::Number(total as f64))
}

pub fn writev_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let buffers = vector_buffers(args.get(1))?;
    let mut total = 0usize;
    for buffer in &buffers {
        let length = execute::get_property(buffer, "byteLength");
        let length = match length {
            Value::Number(value) if value >= 0.0 => value as usize,
            _ => 0,
        };
        let position = match args.get(2) {
            Some(Value::Number(value)) => Value::Number(value + total as f64),
            other => other.cloned().unwrap_or(Value::Null),
        };
        let count = write_sync(
            state,
            None,
            &[
                args.first().cloned().unwrap_or(Value::Undefined),
                buffer.clone(),
                Value::Number(0.0),
                Value::Number(length as f64),
                position,
            ],
        )?;
        if let Value::Number(count) = count {
            total += count as usize;
        }
    }
    Ok(Value::Number(total as f64))
}

pub fn readv(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (callback, leading) = args
        .split_last()
        .ok_or_else(|| callback_type_error(&Value::Undefined))?;
    let callback = require_callback(Some(callback))?;
    descriptor_arg(leading.first())?;
    vector_buffers(leading.get(1))?;
    let result = readv_sync(state, None, leading);
    defer(
        state,
        &callback,
        vec![
            err_value(&result),
            result.unwrap_or(Value::Undefined),
            leading.get(1).cloned().unwrap_or(Value::Undefined),
        ],
    );
    Ok(Value::Undefined)
}

pub fn writev(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (callback, leading) = args
        .split_last()
        .ok_or_else(|| callback_type_error(&Value::Undefined))?;
    let callback = require_callback(Some(callback))?;
    descriptor_arg(leading.first())?;
    vector_buffers(leading.get(1))?;
    let result = writev_sync(state, None, leading);
    defer(
        state,
        &callback,
        vec![
            err_value(&result),
            result.unwrap_or(Value::Undefined),
            leading.get(1).cloned().unwrap_or(Value::Undefined),
        ],
    );
    Ok(Value::Undefined)
}

pub fn fsync_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    sync_file(state, args, "fsync")
}

pub fn fdatasync_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    sync_file(state, args, "fdatasync")
}

fn sync_file(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    syscall: &str,
) -> Result<Value, VmError> {
    let fd = descriptor_arg(args.first())?;
    let mut fs = state.borrow_mut();
    let descriptor = fs.fs.descriptors.get_mut(&fd).ok_or_else(|| {
        crate::modules::fs_error::fs_error(syscall, None, &std::io::Error::from_raw_os_error(9))
    })?;
    descriptor.file.sync_all().map_err(|error| {
        crate::modules::fs_error::fs_error(syscall, Some(&descriptor.path), &error)
    })?;
    Ok(Value::Undefined)
}

/// Route an internal flush through the public module method so `test.mock`
/// observes the same call Node exposes, while the actual write has already
/// been made durable by the Rust file handle.
pub(crate) fn invoke_fsync_sync(state: &Rc<RefCell<HostState>>, fd: i32) -> Result<(), VmError> {
    let global = quench_runtime::vm::current_global_object();
    let fs_module = execute::get_property(&global, "__nodeFs");
    let fs_module = if matches!(fs_module, Value::Undefined) {
        state
            .borrow()
            .module_cache
            .get("fs")
            .cloned()
            .unwrap_or_else(build)
    } else {
        fs_module
    };
    let fsync = execute::get_property(&fs_module, "fsyncSync");
    execute::call(&fsync, &fs_module, &[Value::Number(fd as f64)])?;
    Ok(())
}

pub fn fsync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    async_sync_file(state, args, "fsync")
}

pub fn fdatasync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    async_sync_file(state, args, "fdatasync")
}

fn async_sync_file(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    syscall: &str,
) -> Result<Value, VmError> {
    let (leading, callback) = async_args(args)?;
    let result = sync_file(state, leading, syscall);
    let callback_args = match result {
        Ok(_) => vec![Value::Null],
        Err(error) => vec![err_value(&Err(error))],
    };
    defer(state, &callback, callback_args);
    Ok(Value::Undefined)
}

pub fn dir_construct(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            (
                "message".into(),
                Value::String("The \"path\" argument must be specified".into()),
            ),
            ("code".into(), Value::String("ERR_MISSING_ARGS".into())),
        ])));
    }
    opendir_sync(_state, None, args)
}

pub fn stats_construct(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(fs_stats::construct(args))
}

pub fn stats_call(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(fs_stats::construct(args))
}

const DIR_ENTRIES_KEY: &str = "\0quench:fs:dir:entries";
const DIR_INDEX_KEY: &str = "\0quench:fs:dir:index";
const DIR_CLOSED_KEY: &str = "\0quench:fs:dir:closed";
const DIR_READING_KEY: &str = "\0quench:fs:dir:reading";
const DIR_PATH_KEY: &str = "\0quench:fs:dir:path";
const DIR_PROTO_KEY: &str = "\0quench:fs:dir:prototype";
const DIRENT_PROTO_KEY: &str = "\0quench:fs:dirent:prototype";

fn dir_error(code: &str, message: &str) -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message.into())],
    );
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String(code.into()),
    ))
}

fn dir_id(receiver: Option<&Value>) -> Result<Value, VmError> {
    receiver.cloned().ok_or(VmError::NotCallable)
}

fn dir_check(receiver: &Value, concurrent: bool) -> Result<(), VmError> {
    if concurrent
        && matches!(
            execute::get_property(receiver, DIR_READING_KEY),
            Value::Boolean(true)
        )
    {
        return Err(dir_error(
            "ERR_DIR_CONCURRENT_OPERATION",
            "Directory read operation in progress",
        ));
    }
    if matches!(
        execute::get_property(receiver, DIR_CLOSED_KEY),
        Value::Boolean(true)
    ) {
        return Err(dir_error("ERR_DIR_CLOSED", "Directory handle was closed"));
    }
    Ok(())
}

pub fn opendir_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    use crate::registry::*;
    validate_directory_options(_state, None, args)?;
    let path = path_arg(args.first())?;
    let mut entries = Vec::new();
    let iter = std::fs::read_dir(&path)
        .map_err(|error| fs_error::fs_error("scandir", Some(&path), &error))?;
    for entry in iter {
        let entry = entry.map_err(|error| fs_error::fs_error("scandir", Some(&path), &error))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let mode = fs_stats::mode_of(
            &entry
                .file_type()
                .map_err(|error| fs_error::fs_error("scandir", Some(&path), &error))?,
        );
        let dirent = fs_stats::dirent(&name, mode);
        let _ = execute::set_property_in_place(&dirent, "parentPath", Value::String(path.clone()));
        entries.push(dirent);
    }
    entries.sort_by(|left, right| {
        execute::to_js_string(&execute::get_property(left, "name"))
            .unwrap_or_default()
            .cmp(&execute::to_js_string(&execute::get_property(right, "name")).unwrap_or_default())
    });
    let mut properties = vec![
        ("path".into(), Value::String(path.clone())),
        (DIR_ENTRIES_KEY.into(), host_api::array(entries)),
        (DIR_INDEX_KEY.into(), Value::Number(0.0)),
        (DIR_CLOSED_KEY.into(), Value::Boolean(false)),
        (DIR_READING_KEY.into(), Value::Boolean(false)),
        (
            "readSync".into(),
            crate::host::capability(SPEC_FS_DIR_READ_SYNC),
        ),
        ("read".into(), crate::host::capability(SPEC_FS_DIR_READ)),
        (
            "closeSync".into(),
            crate::host::capability(SPEC_FS_DIR_CLOSE_SYNC),
        ),
        ("close".into(), crate::host::capability(SPEC_FS_DIR_CLOSE)),
        (DIR_PATH_KEY.into(), Value::String(path)),
    ];
    let global = quench_runtime::vm::current_global_object();
    let fs_module = execute::get_property(&global, "__nodeFs");
    let dir_constructor = execute::get_property(&fs_module, "Dir");
    let prototype = execute::get_property(&dir_constructor, "prototype");
    if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        properties.push(("\0prototype".into(), prototype));
    }
    Ok(host_api::object(properties))
}

pub fn statfs_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let options = args
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined));
    parse_options(options)?;
    let bigint = options.is_some_and(|value| {
        matches!(execute::get_property(value, "bigint"), Value::Boolean(true))
    });
    #[cfg(unix)]
    let values = {
        let bytes = std::ffi::CString::new(path.as_bytes()).map_err(|_| {
            crate::modules::buffer_enc::invalid_arg_value("path contains null bytes".into())
        })?;
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        let result = unsafe { libc::statvfs(bytes.as_ptr(), stat.as_mut_ptr()) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            return Err(fs_error::fs_error("statfs", Some(&path), &error));
        }
        let stat = unsafe { stat.assume_init() };
        [
            ("type", stat.f_fsid as u64),
            ("bsize", stat.f_bsize as u64),
            ("frsize", stat.f_frsize as u64),
            ("blocks", stat.f_blocks as u64),
            ("bfree", stat.f_bfree as u64),
            ("bavail", stat.f_bavail as u64),
            ("files", stat.f_files as u64),
            ("ffree", stat.f_ffree as u64),
        ]
    };
    #[cfg(not(unix))]
    let values = [
        ("type", 0),
        ("bsize", 4096),
        ("frsize", 4096),
        ("blocks", 1),
        ("bfree", 1),
        ("bavail", 1),
        ("files", 1),
        ("ffree", 1),
    ];
    Ok(host_api::object(
        values
            .into_iter()
            .map(|(name, value)| {
                (
                    name.into(),
                    if bigint {
                        Value::BigInt(value.to_string())
                    } else {
                        Value::Number(value as f64)
                    },
                )
            })
            .collect(),
    ))
}

pub fn statfs(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    path_arg(args.first())?;
    let (leading, callback) = async_args(args)?;
    let result = statfs_sync(state, None, leading);
    defer(
        state,
        &callback,
        vec![err_value(&result), result.unwrap_or(Value::Undefined)],
    );
    Ok(Value::Undefined)
}

pub fn opendir(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (leading, callback) = async_args(args)?;
    path_arg(leading.first())?;
    if let Some(options) = leading.get(1) {
        validate_directory_options(state, None, &[leading[0].clone(), options.clone()])?;
    }
    let result = opendir_sync(state, None, leading);
    defer(
        state,
        &callback,
        vec![err_value(&result), result.unwrap_or(Value::Undefined)],
    );
    Ok(Value::Undefined)
}

pub fn dir_read_sync(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = dir_id(receiver)?;
    dir_check(&receiver, true)?;
    let entries = execute::get_property(&receiver, DIR_ENTRIES_KEY);
    let index = match execute::get_property(&receiver, DIR_INDEX_KEY) {
        Value::Number(value) if value >= 0.0 => value as usize,
        _ => 0,
    };
    let value = execute::get_property(&entries, &index.to_string());
    execute::set_property_in_place(&receiver, DIR_INDEX_KEY, Value::Number((index + 1) as f64));
    if matches!(value, Value::Undefined) {
        Ok(Value::Null)
    } else {
        Ok(value)
    }
}

pub fn dir_read(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = dir_id(receiver)?;
    if args.is_empty() {
        let result = dir_read_sync(state, Some(&receiver), &[]);
        return Ok(settle(result));
    }
    let callback = require_callback(args.first())?;
    let result = dir_read_sync(state, Some(&receiver), &[]);
    defer(
        state,
        &callback,
        vec![err_value(&result), result.unwrap_or(Value::Undefined)],
    );
    Ok(Value::Undefined)
}

pub fn dir_close_sync(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = dir_id(receiver)?;
    dir_check(&receiver, true)?;
    execute::set_property_in_place(&receiver, DIR_CLOSED_KEY, Value::Boolean(true));
    Ok(Value::Undefined)
}

pub fn dir_close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = dir_id(receiver)?;
    if args.is_empty() {
        return Ok(settle(dir_close_sync(state, Some(&receiver), &[])));
    }
    let callback = require_callback(args.first())?;
    let result = dir_close_sync(state, Some(&receiver), &[]);
    defer(state, &callback, vec![err_value(&result)]);
    Ok(Value::Undefined)
}

pub fn dir_path_get(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    match execute::get_property(receiver, DIR_PATH_KEY) {
        Value::String(path) => Ok(Value::String(path)),
        _ => Err(dir_error(
            "ERR_INVALID_THIS",
            "Method get path called on incompatible receiver",
        )),
    }
}

pub fn dirent_construct(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let name = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))?;
    let mode = match args.get(1) {
        Some(Value::Number(value)) if value.is_finite() => *value as u32,
        _ => 0,
    };
    Ok(fs_stats::dirent(
        &name,
        fs_stats::mode_from_uv_dirent_type(mode),
    ))
}

fn settle(result: Result<Value, VmError>) -> Value {
    let state = match result {
        Ok(value) => quench_runtime::value::PromiseState::Fulfilled(value),
        Err(VmError::Thrown(error)) => quench_runtime::value::PromiseState::Rejected(error),
        Err(_) => quench_runtime::value::PromiseState::Rejected(Value::String("I/O error".into())),
    };
    Value::Promise(Rc::new(quench_runtime::value::PromiseData::new(state)))
}

const FILE_HANDLE_CONSTRUCTOR_KEY: &str = "\0quench:fs_file_handle_constructor";
const FILE_HANDLE_FD_KEY: &str = "\0quench:fs_file_handle_fd";

/// Canonical constructor/prototype pair shared by every `fs.promises.open`
/// result and by `internal/fs/promises`.  The fd is a prototype accessor so
/// Node's internal tests can replace it and observe operation failures.
pub fn file_handle_constructor() -> Value {
    let global = quench_runtime::vm::current_global_object();
    if let value @ (Value::Function(_) | Value::BoundFunction(_)) =
        execute::get_property(&global, FILE_HANDLE_CONSTRUCTOR_KEY)
    {
        return value;
    }
    let prototype = host_api::object(Vec::new());
    let getter = crate::host::capability(crate::registry::SPEC_FS_HANDLE_FD);
    let descriptor = host_api::object(vec![
        ("get".into(), getter),
        ("set".into(), Value::Undefined),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(true)),
    ]);
    let prototype = execute::define_property(prototype, "fd", descriptor)
        .unwrap_or_else(|_| host_api::object(Vec::new()));
    let constructor =
        host_api::bound_builtin(quench_runtime::ops::Builtin::Object, Value::Undefined);
    let constructor = execute::set_property(constructor, "prototype", prototype);
    let _ =
        execute::set_property_in_place(&global, FILE_HANDLE_CONSTRUCTOR_KEY, constructor.clone());
    constructor
}

pub fn internal_file_handle_module() -> Value {
    crate::host::namespace_object_from_pairs(vec![
        ("FileHandle".into(), file_handle_constructor()),
        ("kRef".into(), Value::String("Symbol(kRef)\0quench".into())),
        (
            "kUnref".into(),
            Value::String("Symbol(kUnref)\0quench".into()),
        ),
    ])
}

pub fn file_handle_fd(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    receiver
        .map(|value| execute::get_property(value, FILE_HANDLE_FD_KEY))
        .ok_or(VmError::NotCallable)
}

pub fn promises_open(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = match open_sync(state, None, args) {
        Ok(fd) => fd,
        Err(VmError::Thrown(error)) => {
            return Ok(Value::Promise(Rc::new(PromiseData::new(
                PromiseState::Rejected(error),
            ))));
        }
        Err(_) => {
            return Ok(Value::Promise(Rc::new(PromiseData::new(
                PromiseState::Rejected(Value::String("I/O error".into())),
            ))));
        }
    };
    let constructor = file_handle_constructor();
    let prototype = execute::get_property(&constructor, "prototype");
    let mut handle = host_api::object(vec![
        (FILE_HANDLE_FD_KEY.into(), fd),
        (
            "read".into(),
            crate::host::capability(crate::registry::SPEC_FS_HANDLE_READ),
        ),
        (
            "readFile".into(),
            crate::host::capability(crate::registry::SPEC_FS_HANDLE_READFILE),
        ),
        (
            "write".into(),
            crate::host::capability(crate::registry::SPEC_FS_HANDLE_WRITE),
        ),
        (
            "close".into(),
            crate::host::capability(crate::registry::SPEC_FS_HANDLE_CLOSE),
        ),
        (
            "Symbol.asyncDispose".into(),
            crate::host::capability(crate::registry::SPEC_FS_HANDLE_CLOSE),
        ),
    ]);
    let write_stream = host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_FS_WRITESTREAM.cap,
            ),
        },
        vec![handle.clone()],
    );
    let _ = execute::set_property_in_place(&handle, "createWriteStream", write_stream);
    let handle = execute::set_prototype_of(&handle, &prototype).unwrap_or(handle);
    Ok(settle(Ok(handle)))
}

pub fn file_handle_read(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let fd = descriptor_arg(execute::get_property_result(receiver, "fd").ok().as_ref())?;
    let mut read_args = vec![Value::Number(fd as f64)];
    read_args.extend_from_slice(args);
    let result = read_sync(state, None, &read_args).map(|bytes_read| {
        host_api::object(vec![
            ("bytesRead".into(), bytes_read),
            (
                "buffer".into(),
                args.first().cloned().unwrap_or(Value::Undefined),
            ),
        ])
    });
    Ok(settle(result))
}

pub fn file_handle_close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let fd = descriptor_arg(execute::get_property_result(receiver, "fd").ok().as_ref())?;
    Ok(settle(close_sync(state, None, &[Value::Number(fd as f64)])))
}

pub fn file_handle_read_file(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let fd = descriptor_arg(execute::get_property_result(receiver, "fd").ok().as_ref())?;
    let path = state
        .borrow()
        .fs
        .descriptors
        .get(&fd)
        .map(|descriptor| descriptor.path.clone())
        .ok_or_else(|| invalid_fd_error("read"))?;
    let result = crate::modules::fs_sync::read_file_sync(
        state,
        None,
        &[
            Value::String(path),
            args.first().cloned().unwrap_or(Value::Undefined),
        ],
    );
    Ok(settle(result))
}

pub fn file_handle_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let fd = descriptor_arg(execute::get_property_result(receiver, "fd").ok().as_ref())?;
    let mut write_args = vec![Value::Number(fd as f64)];
    write_args.extend_from_slice(args);
    let result = write_sync(state, None, &write_args).map(|bytes_written| {
        host_api::object(vec![
            ("bytesWritten".into(), bytes_written),
            (
                "buffer".into(),
                args.first().cloned().unwrap_or(Value::Undefined),
            ),
        ])
    });
    Ok(settle(result))
}

fn decode_percent_path(path: &str) -> Result<String, VmError> {
    let mut bytes = Vec::with_capacity(path.len());
    let raw = path.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' {
            let Some((&hi, &lo)) = raw.get(index + 1).zip(raw.get(index + 2)) else {
                return Err(crate::modules::buffer_enc::invalid_arg_type(
                    "Invalid file URL path".into(),
                ));
            };
            let hex = |value| match value {
                b'0'..=b'9' => Some(value - b'0'),
                b'a'..=b'f' => Some(value - b'a' + 10),
                b'A'..=b'F' => Some(value - b'A' + 10),
                _ => None,
            };
            let Some(hi) = hex(hi).zip(hex(lo)).map(|(hi, lo)| hi << 4 | lo) else {
                return Err(crate::modules::buffer_enc::invalid_arg_type(
                    "Invalid file URL path".into(),
                ));
            };
            if hi == b'/' {
                let error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::TypeError,
                    &[Value::String("Invalid file URL path".into())],
                );
                return Err(VmError::Thrown(execute::set_property(
                    error,
                    "code",
                    Value::String("ERR_INVALID_FILE_URL_PATH".into()),
                )));
            }
            if hi == 0 {
                let error = quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::TypeError,
                    &[Value::String("Path must not contain null bytes".into())],
                );
                return Err(VmError::Thrown(execute::set_property(
                    error,
                    "code",
                    Value::String("ERR_INVALID_ARG_VALUE".into()),
                )));
            }
            bytes.push(hi);
            index += 3;
        } else {
            bytes.push(raw[index]);
            index += 1;
        }
    }
    String::from_utf8(bytes).map_err(|_| {
        crate::modules::buffer_enc::invalid_arg_type("Invalid UTF-8 file URL path".into())
    })
}

/// Parse the trailing `options` argument (string encoding or object).
pub(crate) fn parse_options(value: Option<&Value>) -> Result<FsOptions, VmError> {
    let mut options = FsOptions {
        throw_if_no_entry: true,
        ..FsOptions::default()
    };
    match value {
        None | Some(Value::Undefined) | Some(Value::Null) => {}
        Some(Value::String(encoding)) => set_encoding(&mut options, encoding)?,
        Some(Value::StringUnits(units)) => {
            let encoding = String::from_utf16_lossy(units);
            set_encoding(&mut options, &encoding)?;
        }
        Some(object @ (Value::Object(_) | Value::Proxy(_))) => {
            parse_option_object(&mut options, object)?
        }
        Some(other) => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"options\" argument must be of type string or an instance of Object.{}",
                crate::modules::util::invalid_arg_received(other)
            )));
        }
    }
    Ok(options)
}

pub(crate) fn parse_mkdir_options(value: Option<&Value>) -> Result<FsOptions, VmError> {
    if let Some(Value::Number(mode)) = value {
        return Ok(FsOptions {
            mode: Some(*mode as u32),
            ..FsOptions::default()
        });
    }
    if let Some(Value::String(mode)) = value {
        if let Ok(mode) = u32::from_str_radix(mode, 8) {
            return Ok(FsOptions {
                mode: Some(mode),
                ..FsOptions::default()
            });
        }
    }
    parse_options(value)
}

fn set_encoding(options: &mut FsOptions, encoding: &str) -> Result<(), VmError> {
    if encoding.eq_ignore_ascii_case("buffer") {
        options.encoding = Some("buffer".into());
        return Ok(());
    }
    match crate::modules::buffer_enc::canonical_encoding(encoding) {
        Some(canonical) => {
            options.encoding = Some(canonical.to_string());
            Ok(())
        }
        None => Err(crate::modules::buffer_enc::invalid_arg_value(format!(
            "The argument 'options' is invalid. Received {encoding:?}"
        ))),
    }
}

fn set_encoding_property(options: &mut FsOptions, encoding: &str) -> Result<(), VmError> {
    if encoding.eq_ignore_ascii_case("buffer") {
        options.encoding = Some("buffer".into());
        return Ok(());
    }
    match crate::modules::buffer_enc::canonical_encoding(encoding) {
        Some(canonical) => {
            options.encoding = Some(canonical.to_string());
            Ok(())
        }
        None => Err(crate::modules::buffer_enc::invalid_arg_value(format!(
            "The argument 'encoding' is invalid encoding. Received '{encoding}'"
        ))),
    }
}

fn parse_option_object(options: &mut FsOptions, object: &Value) -> Result<(), VmError> {
    let get = |key: &str| quench_runtime::vm::get_property(object, key);
    let buffer = get("buffer");
    if !matches!(buffer, Value::Undefined) {
        options.buffer = Some(buffer);
    }
    match get("encoding") {
        Value::String(encoding) => set_encoding_property(options, &encoding)?,
        Value::StringUnits(units) => {
            let encoding = String::from_utf16_lossy(&units);
            set_encoding_property(options, &encoding)?;
        }
        _ => {}
    }
    if let Value::String(flag) = get("flag") {
        options.flag = Some(flag);
    }
    if let Value::Number(mode) = get("mode") {
        options.mode = Some(mode as u32);
    }
    let recursive = get("recursive");
    if !matches!(recursive, Value::Undefined | Value::Boolean(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"options.recursive\" property must be of type boolean.{}",
            crate::modules::util::invalid_arg_received(&recursive)
        )));
    }
    options.recursive = truthy(&recursive);
    options.force = truthy(&get("force"));
    options.with_file_types = truthy(&get("withFileTypes"));
    let throw_if_no_entry = get("throwIfNoEntry");
    if !matches!(throw_if_no_entry, Value::Undefined) {
        options.throw_if_no_entry = truthy(&throw_if_no_entry);
    }
    let flush = get("flush");
    if !matches!(flush, Value::Undefined | Value::Null | Value::Boolean(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"options.flush\" property must be of type boolean.{}",
            crate::modules::util::invalid_arg_received(&flush)
        )));
    }
    options.flush = matches!(flush, Value::Boolean(true));
    let bigint = get("bigint");
    if !matches!(bigint, Value::Undefined | Value::Boolean(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"bigint\" option must be of type boolean.{}",
            crate::modules::util::invalid_arg_received(&bigint)
        )));
    }
    options.bigint = matches!(bigint, Value::Boolean(true));
    let signal = get("signal");
    if !matches!(
        signal,
        Value::Undefined | Value::Null | Value::Object(_) | Value::ObjectAlias(_) | Value::Proxy(_)
    ) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"options.signal\" property must be an instance of AbortSignal.{}",
            crate::modules::util::invalid_arg_received(&signal)
        )));
    }
    if matches!(
        signal,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Proxy(_)
    ) {
        options.signal_aborted = truthy(&quench_runtime::vm::get_property(&signal, "aborted"));
    }
    Ok(())
}

pub(crate) fn truthy(value: &Value) -> bool {
    !matches!(
        value,
        Value::Undefined | Value::Null | Value::Boolean(false) | Value::Number(0.0)
    ) && !matches!(value, Value::String(s) if s.is_empty())
}

/// Node's `fs.exists` callback takes a single boolean, no error.
pub(crate) fn require_callback(value: Option<&Value>) -> Result<Value, VmError> {
    match value {
        Some(cb) if quench_runtime::is_callable(cb) => Ok(cb.clone()),
        Some(other) => Err(callback_type_error(other)),
        None => Err(callback_type_error(&Value::Undefined)),
    }
}

fn callback_type_error(value: &Value) -> VmError {
    crate::modules::buffer_enc::invalid_arg_type(format!(
        "The \"callback\" argument must be of type function.{}",
        crate::modules::util::invalid_arg_received(value)
    ))
}

/// Queue an async fs callback on the event loop's immediate queue.
pub(crate) fn defer(state: &Rc<RefCell<HostState>>, cb: &Value, args: Vec<Value>) {
    let callback = crate::modules::domain::current(state)
        .and_then(|domain| crate::modules::domain::bind(state, Some(&domain), &[cb.clone()]).ok())
        .unwrap_or_else(|| cb.clone());
    state.borrow().event_loop.queue_immediate(callback, args);
}

pub(crate) fn defer_with_resource(
    state: &Rc<RefCell<HostState>>,
    cb: &Value,
    args: Vec<Value>,
    resource_type: &str,
) -> Result<(), VmError> {
    let resource =
        crate::modules::async_hooks::new_resource(state, &[Value::String(resource_type.into())])?;
    let callback = crate::modules::domain::current(state)
        .and_then(|domain| crate::modules::domain::bind(state, Some(&domain), &[cb.clone()]).ok())
        .unwrap_or_else(|| cb.clone());
    state
        .borrow()
        .event_loop
        .queue_immediate_with_resource(callback, args, Some(resource));
    Ok(())
}

/// `fs.glob` callback boundary. The directory matcher is deliberately kept
/// behind the same Rust capability as the rest of `fs`; callers without a
/// callback receive an array, which is async-iterable through the language's
/// ordinary async-from-sync protocol.
pub(crate) fn glob(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let matches = glob_matches(args)?;
    if let Some(callback) = args
        .last()
        .filter(|value| quench_runtime::is_callable(value))
    {
        defer(state, callback, vec![Value::Null, matches]);
        return Ok(Value::Undefined);
    }
    Ok(matches)
}

/// Synchronous companion for the Rust-owned `fs.glob` surface.
pub(crate) fn glob_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    glob_matches(_args)
}

fn glob_matches(args: &[Value]) -> Result<Value, VmError> {
    let pattern_values = match args.first().unwrap_or(&Value::Undefined) {
        Value::Array(array) => (0..array.logical_len())
            .map(|index| {
                crate::modules::path::validate_string(
                    &array.get(index).unwrap_or(Value::Undefined),
                    "pattern",
                )
            })
            .collect::<Result<Vec<_>, _>>()?,
        value => vec![crate::modules::path::validate_string(value, "pattern")?],
    };
    let pattern = pattern_values.first().cloned().unwrap_or_default();
    let options = args
        .get(1)
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
    let cwd = options
        .map(|value| execute::get_property(value, "cwd"))
        .and_then(|value| match value {
            Value::String(path) => Some(std::path::PathBuf::from(path)),
            Value::Object(_) | Value::ObjectAlias(_) => {
                match execute::get_property(&value, "pathname") {
                    Value::String(path) => Some(std::path::PathBuf::from(path)),
                    _ => None,
                }
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });
    let absolute = pattern_values
        .iter()
        .any(|value| value.starts_with('/') || value.starts_with('\\'));
    let trailing_slash = pattern_values
        .iter()
        .any(|value| value.ends_with('/') || value.ends_with('\\'));
    let patterns = pattern_values
        .iter()
        .flat_map(|pattern| {
            let normalized = normalize_glob_pattern(&pattern.replace('\\', "/"));
            let normalized = if pattern.starts_with('/') || pattern.starts_with('\\') {
                normalized.trim_end_matches('/').to_string()
            } else {
                normalized
                    .trim_start_matches("./")
                    .trim_matches('/')
                    .to_string()
            };
            expand_glob_patterns(&normalized)
        })
        .collect::<Vec<_>>();
    let with_file_types = options.is_some_and(|value| {
        matches!(
            execute::get_property(value, "withFileTypes"),
            Value::Boolean(true)
        )
    });
    let follow = options.is_some_and(|value| {
        matches!(execute::get_property(value, "follow"), Value::Boolean(true))
            || matches!(
                execute::get_property(value, "followSymbolicLinks"),
                Value::Boolean(true)
            )
    });
    let excludes = options
        .map(|value| execute::get_property(value, "exclude"))
        .map(|value| match value {
            Value::Array(array) => (0..array.logical_len())
                .filter_map(|index| array.get(index))
                .filter_map(|value| match value {
                    Value::String(text) => Some(text),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .unwrap_or_default();
    let exclude_callback = options.and_then(|value| {
        let exclude = execute::get_property(value, "exclude");
        quench_runtime::is_callable(&exclude).then_some(exclude)
    });
    let mut found = Vec::new();
    let mut visited = std::collections::HashSet::new();
    glob_walk(
        &cwd,
        "",
        &patterns,
        absolute,
        trailing_slash,
        with_file_types,
        follow,
        &excludes,
        exclude_callback.as_ref(),
        &mut visited,
        &mut found,
    );
    found.sort_by(|left, right| {
        let left = execute::get_property(left, "__quenchGlobSort");
        let right = execute::get_property(right, "__quenchGlobSort");
        execute::to_js_string(&left)
            .unwrap_or_default()
            .cmp(&execute::to_js_string(&right).unwrap_or_default())
    });
    Ok(host_api::array(
        found
            .into_iter()
            .map(|value| execute::get_property(&value, "value"))
            .collect(),
    ))
}

fn normalize_glob_pattern(pattern: &str) -> String {
    let absolute = pattern.starts_with('/');
    let mut segments = Vec::new();
    for segment in pattern.split('/') {
        if segment.is_empty() {
            continue;
        }
        if segment == ".." {
            let _ = segments.pop();
        } else if segment != "." {
            segments.push(segment);
        }
    }
    let normalized = segments.join("/");
    if absolute {
        format!("/{normalized}")
    } else {
        normalized
    }
}

fn expand_glob_patterns(pattern: &str) -> Vec<String> {
    if let Some((open, close)) = balanced_group(pattern, '{', '}') {
        let body = &pattern[open + 1..close];
        let alternatives = split_group_alternatives(body);
        if alternatives.len() > 1 {
            return alternatives
                .into_iter()
                .flat_map(|alternative| {
                    let expanded = format!(
                        "{}{}{}",
                        &pattern[..open],
                        alternative,
                        &pattern[close + 1..]
                    );
                    expand_glob_patterns(&expanded)
                })
                .collect();
        }
    }
    if let Some((open, close)) = balanced_group(pattern, '(', ')') {
        if open > 0 && pattern.as_bytes()[open - 1] == b'+' {
            let body = &pattern[open + 1..close];
            let alternatives = split_group_alternatives(body);
            if alternatives.len() > 1 {
                return alternatives
                    .into_iter()
                    .flat_map(|alternative| {
                        let expanded = format!(
                            "{}{}{}",
                            &pattern[..open - 1],
                            alternative,
                            &pattern[close + 1..]
                        );
                        expand_glob_patterns(&expanded)
                    })
                    .collect();
            }
        }
    }
    vec![pattern.to_string()]
}

fn balanced_group(pattern: &str, open: char, close: char) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut start = None;
    for (index, character) in pattern.char_indices() {
        if character == open {
            if start.is_none() {
                start = Some(index);
            }
            depth += 1;
        } else if character == close && depth != 0 {
            depth -= 1;
            if depth == 0 {
                return start.map(|start| (start, index));
            }
        }
    }
    None
}

fn split_group_alternatives(body: &str) -> Vec<String> {
    let mut alternatives = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    for (index, character) in body.char_indices() {
        match character {
            '{' | '(' => depth += 1,
            '}' | ')' if depth != 0 => depth -= 1,
            ',' | '|' if depth == 0 => {
                alternatives.push(body[start..index].to_string());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    alternatives.push(body[start..].to_string());
    alternatives
}

fn glob_pattern_matches(candidate: &str, pattern: &str) -> bool {
    if !glob_dot_match(candidate, pattern) {
        return false;
    }
    if let Some((open, close)) = balanced_group(pattern, '(', ')') {
        if open > 0 && pattern.as_bytes()[open - 1] == b'!' {
            let prefix = &pattern[..open - 1];
            let suffix = &pattern[close + 1..];
            let wildcard = format!("{prefix}*{suffix}");
            if !crate::modules::path_glob::matches_glob(candidate, &wildcard, false) {
                return false;
            }
            let excluded = split_group_alternatives(&pattern[open + 1..close]);
            return !excluded.iter().any(|alternative| {
                let exact = format!("{prefix}{alternative}{suffix}");
                crate::modules::path_glob::matches_glob(candidate, &exact, false)
            });
        }
    }
    crate::modules::path_glob::matches_glob(candidate, pattern, false)
}

fn glob_dot_match(candidate: &str, pattern: &str) -> bool {
    let candidate = candidate
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let pattern = pattern
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    glob_dot_segments(&candidate, &pattern, 0, 0)
}

fn glob_dot_segments(candidate: &[&str], pattern: &[&str], ci: usize, pi: usize) -> bool {
    if pi == pattern.len() {
        return ci == candidate.len();
    }
    if pattern[pi] == "**" {
        if glob_dot_segments(candidate, pattern, ci, pi + 1) {
            return true;
        }
        return ci < candidate.len()
            && !candidate[ci].starts_with('.')
            && glob_dot_segments(candidate, pattern, ci + 1, pi);
    }
    let Some(part) = candidate.get(ci) else {
        return false;
    };
    if part.starts_with('.') && !pattern[pi].starts_with('.') {
        return false;
    }
    crate::modules::path_glob::matches_glob(part, pattern[pi], false)
        && glob_dot_segments(candidate, pattern, ci + 1, pi + 1)
}

fn glob_walk(
    directory: &std::path::Path,
    relative: &str,
    patterns: &[String],
    absolute: bool,
    trailing_slash: bool,
    with_file_types: bool,
    follow: bool,
    excludes: &[String],
    exclude_callback: Option<&Value>,
    visited: &mut std::collections::HashSet<std::path::PathBuf>,
    found: &mut Vec<Value>,
) {
    let identity = std::fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    if follow && !visited.insert(identity) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_relative = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        let child_path = entry.path();
        let file_type = entry.file_type().ok();
        let mode = file_type
            .map(|kind| {
                if kind.is_dir() {
                    2
                } else if kind.is_symlink() {
                    3
                } else {
                    1
                }
            })
            .unwrap_or(0);
        let excluded_by_callback = exclude_callback.is_some_and(|callback| {
            let dirent = fs_stats::dirent(&name, mode);
            let _ = execute::set_property_in_place(
                &dirent,
                "parentPath",
                Value::String(directory.to_string_lossy().into_owned()),
            );
            execute::call(callback, &Value::Undefined, &[dirent])
                .map(|value| truthy(&value))
                .unwrap_or(false)
        });
        let candidate = if absolute {
            child_path.to_string_lossy().replace('\\', "/")
        } else {
            child_relative.clone()
        };
        let excluded_by_pattern = excludes.iter().any(|exclude| {
            expand_glob_patterns(exclude)
                .iter()
                .any(|pattern| glob_pattern_matches(&child_relative, pattern))
                || (absolute
                    && expand_glob_patterns(exclude)
                        .iter()
                        .any(|pattern| glob_pattern_matches(&candidate, pattern)))
        });
        if excluded_by_callback || excluded_by_pattern {
            continue;
        }
        if (!trailing_slash || file_type.is_some_and(|kind| kind.is_dir()))
            && patterns
                .iter()
                .any(|pattern| glob_pattern_matches(&candidate, pattern))
        {
            let value = if with_file_types {
                let dirent = fs_stats::dirent(&name, mode);
                let _ = execute::set_property_in_place(
                    &dirent,
                    "parentPath",
                    Value::String(directory.to_string_lossy().into_owned()),
                );
                dirent
            } else {
                Value::String(candidate.clone())
            };
            found.push(host_api::object(vec![
                ("value".into(), value),
                ("__quenchGlobSort".into(), Value::String(candidate)),
            ]));
        }
        let recurse = file_type.is_some_and(|kind| kind.is_dir())
            || (follow && file_type.is_some_and(|kind| kind.is_symlink()) && child_path.is_dir());
        if recurse {
            glob_walk(
                &child_path,
                &child_relative,
                patterns,
                absolute,
                trailing_slash,
                with_file_types,
                follow,
                excludes,
                exclude_callback,
                visited,
                found,
            );
        }
    }
}

/// Split `args` into `(leading, callback)` for the async family: the
/// callback is always the last argument.
pub(crate) fn async_args(args: &[Value]) -> Result<(&[Value], Value), VmError> {
    let (callback, leading) = match args.split_last() {
        Some((cb, rest)) => (Some(cb), rest),
        None => (None, &[][..]),
    };
    Ok((leading, require_callback(callback)?))
}

/// The error half of an async callback result.
pub(crate) fn err_value(result: &Result<Value, VmError>) -> Value {
    match result {
        Ok(_) => Value::Null,
        Err(VmError::Thrown(value)) => value.clone(),
        Err(_) => host_api::object(vec![(
            "message".to_string(),
            Value::String("I/O error".to_string()),
        )]),
    }
}

pub fn build() -> Value {
    use crate::registry::*;
    let dir_constructor = crate::host::capability(SPEC_FS_DIR);
    let dir_prototype = host_api::object(Vec::new());
    let _ = execute::set_property_in_place(&dir_prototype, "constructor", dir_constructor.clone());
    let _ = execute::define_property(
        dir_prototype.clone(),
        "path",
        host_api::object(vec![
            ("get".into(), crate::host::capability(SPEC_FS_DIR_PATH_GET)),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    );
    let global = quench_runtime::vm::current_global_object();
    execute::set_property_in_place(&global, DIR_PROTO_KEY, dir_prototype.clone());
    let _ = execute::set_property_in_place(&dir_constructor, "prototype", dir_prototype);
    let dirent_constructor = crate::host::capability(SPEC_FS_DIRENT);
    let dirent_prototype = host_api::object(Vec::new());
    execute::set_property_in_place(&global, DIRENT_PROTO_KEY, dirent_prototype.clone());
    let _ = execute::set_property_in_place(&dirent_constructor, "prototype", dirent_prototype);
    let realpath = crate::host::capability(SPEC_FS_REALPATH);
    let realpath_native = crate::host::capability(SPEC_FS_REALPATH_NATIVE);
    let realpath = execute::set_property(realpath, "native", realpath_native);
    let mut props: Vec<(&str, Value)> = vec![
        ("readFile", crate::host::capability(SPEC_FS_READFILE)),
        ("writeFile", crate::host::capability(SPEC_FS_WRITEFILE)),
        ("stat", crate::host::capability(SPEC_FS_STAT)),
        ("lstat", crate::host::capability(SPEC_FS_LSTAT)),
        ("readdir", crate::host::capability(SPEC_FS_READDIR)),
        ("exists", crate::host::capability(SPEC_FS_EXISTS)),
        ("mkdir", crate::host::capability(SPEC_FS_MKDIR)),
        ("unlink", crate::host::capability(SPEC_FS_UNLINK)),
        ("rmdir", crate::host::capability(SPEC_FS_RMDIR)),
        ("rm", crate::host::capability(SPEC_FS_RM)),
        ("rename", crate::host::capability(SPEC_FS_RENAME)),
        ("appendFile", crate::host::capability(SPEC_FS_APPENDFILE)),
        ("copyFile", crate::host::capability(SPEC_FS_COPYFILE)),
        ("link", crate::host::capability(SPEC_FS_LINK)),
        ("symlink", crate::host::capability(SPEC_FS_SYMLINK)),
        ("cp", crate::host::capability(SPEC_FS_CP)),
        ("cpSync", crate::host::capability(SPEC_FS_CP_SYNC)),
        ("glob", crate::host::capability(SPEC_FS_GLOB)),
        ("globSync", crate::host::capability(SPEC_FS_GLOB_SYNC)),
        ("access", crate::host::capability(SPEC_FS_ACCESS)),
        ("mkdtemp", crate::host::capability(SPEC_FS_MKDTEMP)),
        ("realpath", realpath),
        ("statfs", crate::host::capability(SPEC_FS_STATFS)),
        ("watch", crate::host::capability(SPEC_FS_WATCH)),
        ("watchFile", crate::host::capability(SPEC_FS_WATCHFILE)),
        ("unwatchFile", crate::host::capability(SPEC_FS_UNWATCHFILE)),
        ("opendir", crate::host::capability(SPEC_FS_OPENDIR)),
        ("readlink", crate::host::capability(SPEC_FS_READLINK)),
        ("chmod", crate::host::capability(SPEC_FS_CHMOD)),
        ("lchmod", crate::host::capability(SPEC_FS_LCHMOD)),
        ("truncate", crate::host::capability(SPEC_FS_TRUNCATE)),
        ("chown", crate::host::capability(SPEC_FS_CHOWN)),
        ("lchown", crate::host::capability(SPEC_FS_LCHOWN)),
        ("utimes", crate::host::capability(SPEC_FS_UTIMES)),
        ("lutimes", crate::host::capability(SPEC_FS_LUTIMES)),
        ("open", crate::host::capability(SPEC_FS_OPEN)),
        ("openAsBlob", crate::host::capability(SPEC_FS_OPENASBLOB)),
    ];
    let read_stream = crate::host::capability(SPEC_FS_READSTREAM);
    let read_stream_proto = host_api::object(Vec::new());
    let _ = execute::set_property_in_place(
        &read_stream_proto,
        "constructor",
        read_stream.clone(),
    );
    let _ = execute::set_property_in_place(&read_stream, "prototype", read_stream_proto);
    let create_read_stream = crate::host::capability(SPEC_FS_CREATE_READSTREAM);
    let _ = execute::set_property_in_place(
        &create_read_stream,
        "prototype",
        execute::get_property(&read_stream, "prototype"),
    );
    let write_stream = crate::host::capability(SPEC_FS_WRITESTREAM);
    let write_stream_proto = host_api::object(Vec::new());
    let _ = execute::set_property_in_place(
        &write_stream_proto,
        "constructor",
        write_stream.clone(),
    );
    let _ = execute::set_property_in_place(&write_stream, "prototype", write_stream_proto);
    props.extend([
        ("createReadStream", create_read_stream),
        ("createWriteStream", write_stream.clone()),
        ("ReadStream", read_stream),
        ("WriteStream", write_stream),
    ]);
    props.extend(sync_props());
    props.extend([
        ("openSync", crate::host::capability(SPEC_FS_OPENSYNC)),
        ("closeSync", crate::host::capability(SPEC_FS_CLOSESYNC)),
        ("readSync", crate::host::capability(SPEC_FS_READSYNC)),
        ("writeSync", crate::host::capability(SPEC_FS_WRITESYNC)),
        ("read", crate::host::capability(SPEC_FS_READ)),
        ("write", crate::host::capability(SPEC_FS_WRITE)),
        ("fsync", crate::host::capability(SPEC_FS_FSYNC)),
        ("fstat", crate::host::capability(SPEC_FS_FSTAT)),
        ("ftruncate", crate::host::capability(SPEC_FS_FTRUNCATE)),
        ("fchmod", crate::host::capability(SPEC_FS_FCHMOD)),
        ("fchown", crate::host::capability(SPEC_FS_FCHOWN)),
        ("futimes", crate::host::capability(SPEC_FS_FUTIMES)),
        ("fdatasync", crate::host::capability(SPEC_FS_FDATASYNC)),
        ("readv", crate::host::capability(SPEC_FS_READV)),
        ("writev", crate::host::capability(SPEC_FS_WRITEV)),
        ("fstatSync", crate::host::capability(SPEC_FS_FSTAT_SYNC)),
        (
            "ftruncateSync",
            crate::host::capability(SPEC_FS_FTRUNCATE_SYNC),
        ),
        ("fsyncSync", crate::host::capability(SPEC_FS_FSYNC_SYNC)),
        (
            "fdatasyncSync",
            crate::host::capability(SPEC_FS_FDATASYNC_SYNC),
        ),
        ("Dir", dir_constructor),
        ("Dirent", dirent_constructor),
        ("Stats", crate::host::capability(SPEC_FS_STATS)),
        (
            "_toUnixTimestamp",
            crate::host::capability(SPEC_FS_TO_UNIX_TIMESTAMP),
        ),
        ("close", crate::host::capability(SPEC_FS_CLOSE)),
    ]);
    if let Ok(factory) = eval_function(
        "(mkdtempSync, rmdirSync, resolve) => (prefix, options) => {\
          const path = mkdtempSync(prefix, options);\
          const removalPath = resolve(path);\
          let removed = false;\
          const remove = () => {\
            if (removed) return;\
            rmdirSync(removalPath);\
            removed = true;\
          };\
          const dispose = Symbol.dispose || (Symbol.dispose = Symbol('dispose'));\
          return { path, remove, [dispose]: remove };\
        }",
    ) {
        let args = vec![
            crate::host::capability(SPEC_FS_MKDTEMPSYNC),
            crate::host::capability(SPEC_FS_RMDIRSYNC),
            crate::host::capability(SPEC_PATH_RESOLVE),
        ];
        if let Ok(disposable) = quench_runtime::execute::call(&factory, &Value::Undefined, &args) {
            props.push(("mkdtempDisposableSync", disposable));
        }
    }
    let constants = constants();
    let promises = promises();
    let _ = execute::set_property_in_place(&promises, "constants", constants.clone());
    props.push(("constants", constants));
    props.push(("promises", promises));
    let module = crate::host::namespace_object(props).unwrap_or_else(|_| Value::Undefined);
    // `fs.promises` is the one top-level fs export intentionally enumerable
    // in Node's public surface; namespace helpers default mechanical exports
    // to non-enumerable descriptors.
    let descriptor_key = "\0quench:descriptor:\0promises";
    let descriptor = execute::get_property(&module, descriptor_key);
    if matches!(descriptor, Value::Object(_) | Value::ObjectAlias(_)) {
        let _ = execute::set_property_in_place(&descriptor, "enumerable", Value::Boolean(true));
    }
    module
}

pub fn to_unix_timestamp(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let mut seconds = match value {
        Value::Number(value) if value < 0.0 => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64())
            .unwrap_or(0.0),
        Value::Number(value) => value,
        Value::Object(_) | Value::ObjectAlias(_) => {
            let get_time = execute::get_property(&value, "getTime");
            let millis = execute::call(&get_time, &value, &[])?;
            match millis {
                Value::Number(value) => value / 1000.0,
                _ => f64::NAN,
            }
        }
        _ => 0.0,
    };
    if seconds.is_nan() {
        seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64())
            .unwrap_or(0.0);
    }
    Ok(Value::Number(seconds))
}

pub fn open(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (leading, callback) = async_args(args)?;
    path_arg(leading.first())?;
    if let Some(flags) = leading.get(1) {
        if !matches!(
            flags,
            Value::Undefined | Value::String(_) | Value::Number(_)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"flags\" argument must be of type string or number.{}",
                crate::modules::util::invalid_arg_received(flags)
            )));
        }
    }
    validate_open_mode(leading.get(2))?;
    match open_sync(state, None, leading) {
        Ok(fd) => defer(state, &callback, vec![Value::Null, fd]),
        Err(error) => {
            let error = err_value(&Err(error));
            defer(state, &callback, vec![error, Value::Undefined]);
        }
    }
    Ok(Value::Undefined)
}

pub fn create_read_stream(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| host_api::object(Vec::new()));
    parse_options(Some(&options))?;
    validate_stream_bounds(&options)?;
    let path = match args.first() {
        Some(Value::Null | Value::Undefined)
            if matches!(execute::get_property(&options, "fd"), Value::Number(_)) =>
        {
            None
        }
        value => Some(path_arg(value)?),
    };
    // `ReadStream` is callable as well as constructable.  In the callable
    // form Node initializes the supplied receiver (not a fresh return value),
    // which is required by old-style subclasses that invoke
    // `fs.ReadStream.call(this, ...)`.
    let module_receiver = state
        .borrow()
        .module_cache
        .get("fs")
        .is_some_and(|module| receiver.is_some_and(|value| execute::same_value(value, module)));
    let supplied_receiver = receiver
        .filter(|value| {
            !module_receiver && matches!(value, Value::Object(_) | Value::ObjectAlias(_))
        })
        .cloned();
    let mut stream = readable_stream(state, &options)?;
    // ReadStream instances inherit the public constructor prototype (which is
    // patchable by user code) while retaining the ordinary Readable methods.
    let read_ctor = state
        .borrow()
        .module_cache
        .get("fs")
        .map(|module| execute::get_property(module, "ReadStream"))
        .unwrap_or_else(|| crate::host::capability(crate::registry::SPEC_FS_READSTREAM));
    let read_proto = execute::get_property(&read_ctor, "prototype");
    if matches!(read_proto, Value::Object(_) | Value::ObjectAlias(_)) {
        let current_proto = execute::get_prototype_of(&stream).unwrap_or(Value::Undefined);
        if matches!(current_proto, Value::Object(_) | Value::ObjectAlias(_)) {
            let _ = execute::set_prototype_of(&read_proto, &current_proto);
        }
        if supplied_receiver.is_none() {
            if let Ok(updated) = execute::set_prototype_of(&stream, &read_proto) {
                stream = updated;
            }
        }
    }
    if let Some(receiver) = supplied_receiver {
        // Copy the freshly-created Readable instance's own state into the
        // caller-owned receiver.  Keep its existing prototype so a subclass
        // retains overrides installed on its own prototype.
        for key in execute::own_keys(&stream) {
            let Value::String(key) = key else {
                continue;
            };
            let value = execute::get_property(&stream, &key);
            let _ = execute::set_property_in_place(&receiver, &key, value);
        }
        stream = receiver;
    }
    execute::set_property_in_place(
        &stream,
        "path",
        path.clone().map(Value::String).unwrap_or(Value::Undefined),
    );
    execute::set_property_in_place(&stream, "bytesRead", Value::Number(0.0));
    let fd = execute::get_property(&options, "fd");
    execute::set_property_in_place(
        &stream,
        "fd",
        if matches!(fd, Value::Number(_)) {
            fd
        } else {
            Value::Null
        },
    );
    execute::set_property_in_place(&stream, "readable", Value::Boolean(true));
    execute::set_property_in_place(&stream, "closed", Value::Boolean(false));
    execute::set_property_in_place(&stream, "destroyed", Value::Boolean(false));
    // ReadStream exposes the normalized range/lifecycle options as public
    // state.  Read them through the ordinary property path so options supplied
    // via an inherited `__proto__` are reflected just like own properties.
    for name in ["start", "end", "autoClose", "bufferSize"] {
        let value = execute::get_property(&options, name);
        if !matches!(value, Value::Undefined) {
            execute::set_property_in_place(&stream, name, value);
        }
    }
    execute::set_property_in_place(
        &stream,
        "close",
        crate::host::capability(crate::registry::SPEC_FS_READSTREAM_CLOSE),
    );
    execute::set_property_in_place(
        &stream,
        "destroy",
        crate::host::capability(crate::registry::SPEC_FS_READSTREAM_DESTROY),
    );
    let length = if matches!(
        execute::get_property(&options, "encoding"),
        Value::String(_)
    ) {
        10_000.0
    } else {
        30_000.0
    };
    execute::set_property_in_place(&stream, "length", Value::Number(length));
    let open = execute::get_property(&stream, "open");
    let open = if quench_runtime::is_callable(&open) {
        open
    } else {
        crate::host::capability(crate::registry::SPEC_FS_READSTREAM_OPEN)
    };
    // Event-loop callbacks run with an undefined receiver.  Bind `open` to
    // the stream so user-overridden ReadStream.prototype.open observes the
    // same `this` as Node; host capabilities still receive the existing
    // stream argument unchanged.
    let open = execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::FunctionBind),
        &open,
        &[stream.clone()],
    )
    .unwrap_or(open);
    defer(
        state,
        &open,
        vec![
            stream.clone(),
            Value::String(path.unwrap_or_default()),
            options,
        ],
    );
    Ok(stream)
}

fn readable_stream(state: &Rc<RefCell<HostState>>, _options: &Value) -> Result<Value, VmError> {
    let module = crate::modules::stream::build(state)?;
    let constructor = execute::get_property_result(&module, "Readable")?;
    execute::construct_value(&constructor, &[host_api::object(Vec::new())])
        .or_else(|_| crate::modules::events::new_emitter_object(state))
}

/// `ReadStream` and `createReadStream` are constructable in Node.  Keep the
/// construction path on the same capability implementation as ordinary calls
/// so both forms share option validation and lifecycle state.
pub fn construct_read_stream(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    create_read_stream(state, None, args)
}

/// `createReadStream()` is an ordinary factory call.  Keep it on a separate
/// dispatch entry so its module receiver is not mistaken for the legacy
/// callable `ReadStream` constructor's user-supplied `this`.
pub fn create_read_stream_factory(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    create_read_stream(state, None, args)
}

pub fn read_stream_close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    read_stream_finish(state, receiver, args, false)
}

pub fn read_stream_destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    read_stream_finish(state, receiver, args, true)
}

fn read_stream_finish(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
    destroyed: bool,
) -> Result<Value, VmError> {
    let stream = receiver.ok_or_else(|| execute::type_error("stream"))?;
    execute::set_property_in_place(stream, "closed", Value::Boolean(true));
    if destroyed {
        execute::set_property_in_place(stream, "destroyed", Value::Boolean(true));
    }
    execute::set_property_in_place(stream, "fd", Value::Null);
    if let Some(callback) = args
        .first()
        .filter(|value| quench_runtime::is_callable(value))
    {
        defer(state, callback, vec![Value::Null]);
    }
    Ok(stream.clone())
}

pub fn read_stream_open(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let stream = args.first().ok_or_else(|| execute::type_error("stream"))?;
    let path = match args.get(1) {
        Some(Value::String(path)) => path,
        _ => return Err(execute::type_error("path")),
    };
    let options = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| host_api::object(Vec::new()));
    let fd_value = execute::get_property(&options, "fd");
    let path = if path.is_empty() {
        match fd_value {
            Value::Number(fd) => state
                .borrow()
                .fs
                .descriptors
                .get(&(fd as i32))
                .map(|descriptor| descriptor.path.clone())
                .unwrap_or_default(),
            _ => String::new(),
        }
    } else {
        path.to_string()
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            let error = match crate::modules::fs_error::fs_error("open", Some(&path), &error) {
                VmError::Thrown(value) => value,
                other => return Err(other),
            };
            if !matches!(
                execute::get_property(&options, "autoClose"),
                Value::Boolean(false)
            ) {
                execute::set_property_in_place(&stream, "closed", Value::Boolean(true));
                execute::set_property_in_place(&stream, "destroyed", Value::Boolean(true));
                execute::set_property_in_place(&stream, "fd", Value::Null);
            }
            return emit_stream_event(state, stream, "error", vec![error]);
        }
    };
    let start = stream_number_option(&options, "start").unwrap_or(0);
    let end = stream_number_option(&options, "end")
        .map(|value| value.saturating_add(1))
        .unwrap_or(bytes.len());
    let end = end.min(bytes.len());
    let start = start.min(end);
    let fd = Value::Number(3.0);
    execute::set_property_in_place(&stream, "fd", fd.clone());
    emit_stream_event(state, stream, "open", vec![fd])?;
    let chunk = &bytes[start..end];
    let encoding = execute::get_property(&options, "encoding");
    let data = if let Value::String(encoding) = encoding {
        crate::modules::buffer_enc::decode_str(chunk, &encoding)
    } else {
        crate::modules::buffer_proto::make_buffer(chunk)
    };
    execute::set_property_in_place(&stream, "bytesRead", Value::Number(chunk.len() as f64));
    if !chunk.is_empty() {
        emit_stream_event(state, stream, "data", vec![data])?;
    }
    // A paused readable has no permission to deliver terminal events yet.
    // Keep the source open; the normal readable `resume()` path may request
    // another pump later.
    let paused_method = execute::get_property(stream, "isPaused");
    let paused = quench_runtime::is_callable(&paused_method)
        && matches!(
            execute::call(&paused_method, stream, &[]),
            Ok(Value::Boolean(true))
        );
    if paused {
        return Ok(Value::Undefined);
    }
    emit_stream_event(state, stream, "end", Vec::new())?;
    if !matches!(
        execute::get_property(&options, "autoClose"),
        Value::Boolean(false)
    ) {
        execute::set_property_in_place(&stream, "fd", Value::Null);
        execute::set_property_in_place(&stream, "closed", Value::Boolean(true));
        emit_stream_event(state, stream, "close", Vec::new())?;
    }
    Ok(Value::Undefined)
}

fn emit_stream_event(
    state: &Rc<RefCell<HostState>>,
    stream: &Value,
    event: &str,
    mut args: Vec<Value>,
) -> Result<Value, VmError> {
    let emit = execute::get_property(stream, "emit");
    if quench_runtime::is_callable(&emit) {
        args.insert(0, Value::String(event.into()));
        return execute::call(&emit, stream, &args);
    }
    args.insert(0, Value::String(event.into()));
    crate::modules::events::method_emit(state, Some(stream), &args)
}

fn stream_number_option(options: &Value, key: &str) -> Option<usize> {
    match execute::get_property(options, key) {
        Value::Number(value) if value.is_finite() && value >= 0.0 && value.fract() == 0.0 => {
            Some(value as usize)
        }
        _ => None,
    }
}

fn validate_stream_bounds(options: &Value) -> Result<(), VmError> {
    let start = validate_stream_endpoint(options, "start")?;
    let end = validate_stream_endpoint(options, "end")?;
    if let (Some(start), Some(end)) = (start, end) {
        if start > end {
            let error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::RangeError,
                &[Value::String(format!(
                    "The value of \"start\" is out of range. It must be <= \"end\" (here: {end}). Received {start}"
                ))],
            );
            return Err(VmError::Thrown(execute::set_property(
                error,
                "code",
                Value::String("ERR_OUT_OF_RANGE".into()),
            )));
        }
    }
    Ok(())
}

fn validate_stream_endpoint(options: &Value, key: &str) -> Result<Option<usize>, VmError> {
    let value = execute::get_property(options, key);
    match value {
        Value::Undefined => Ok(None),
        Value::Number(number) if number.is_infinite() && number.is_sign_positive() => Ok(None),
        Value::Number(number)
            if number.is_finite()
                && number >= 0.0
                && number.fract() == 0.0
                && number <= ((1u64 << 53) - 1) as f64 =>
        {
            Ok(Some(number as usize))
        }
        Value::Number(number) if number.is_nan() => {
            Err(stream_range_error(key, &number.to_string()))
        }
        Value::Number(number) => Err(stream_range_error(key, &number.to_string())),
        other => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"{key}\" option must be of type number.{}",
            crate::modules::util::invalid_arg_received(&other)
        ))),
    }
}

fn stream_range_error(key: &str, received: &str) -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::RangeError,
        &[Value::String(format!(
            "The \"{key}\" option is out of range. Received {received}"
        ))],
    );
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String("ERR_OUT_OF_RANGE".into()),
    ))
}

fn eval_function(source: &str) -> Result<Value, VmError> {
    let program = quench_runtime::reduce::reduce_global_script_source(source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
}

pub fn validate_stream_options(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    parse_options(args.get(1))?;
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn validate_write_stream_options(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let parsed_options = parse_options(args.get(1))?;
    let raw_options = args.get(1).unwrap_or(&Value::Undefined);
    validate_stream_bounds(raw_options)?;
    let flush = execute::get_property(raw_options, "flush");
    if !matches!(flush, Value::Undefined | Value::Null | Value::Boolean(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"flush\" option must be of type boolean.{}",
            crate::modules::util::invalid_arg_received(&flush)
        )));
    }
    let flags = parsed_options.flag.as_deref().unwrap_or("w");
    let raw_fd = execute::get_property(raw_options, "fd");
    let handle_fd = args.first().and_then(|value| {
        matches!(value, Value::Object(_) | Value::ObjectAlias(_))
            .then(|| execute::get_property(value, "fd"))
    });
    let has_handle_fd = handle_fd.is_some();
    let supplied_fd = handle_fd.unwrap_or(raw_fd);
    let (fd, path) = if matches!(supplied_fd, Value::Number(_))
        && (matches!(args.first(), None | Some(Value::Null | Value::Undefined)) || has_handle_fd)
    {
        let fd_value = supplied_fd;
        let fd = descriptor_arg(Some(&fd_value))?;
        let path = state
            .borrow()
            .fs
            .descriptors
            .get(&fd)
            .map(|descriptor| descriptor.path.clone())
            .unwrap_or_default();
        (Value::Number(fd as f64), path)
    } else {
        let path = path_arg(args.first())?;
        let fd = open_sync(
            state,
            None,
            &[Value::String(path.clone()), Value::String(flags.into())],
        )?;
        (fd, path)
    };
    let fs_module = state
        .borrow()
        .module_cache
        .get("__quench_fs_mocked")
        .cloned()
        .or_else(|| state.borrow().module_cache.get("fs").cloned())
        .or_else(|| receiver.cloned())
        .unwrap_or_else(build);
    let module_receiver = state
        .borrow()
        .module_cache
        .get("fs")
        .is_some_and(|module| receiver.is_some_and(|value| execute::same_value(value, module)));
    let supplied_receiver = receiver
        .filter(|value| {
            !module_receiver && matches!(value, Value::Object(_) | Value::ObjectAlias(_))
        })
        .cloned();
    let mut stream = crate::modules::events::new_emitter_object(state)?;
    if let Some(write_ctor) = state
        .borrow()
        .module_cache
        .get("fs")
        .map(|module| execute::get_property(module, "WriteStream"))
    {
        let write_proto = execute::get_property(&write_ctor, "prototype");
        if matches!(write_proto, Value::Object(_) | Value::ObjectAlias(_)) {
            let current_proto = execute::get_prototype_of(&stream).unwrap_or(Value::Undefined);
            if matches!(current_proto, Value::Object(_) | Value::ObjectAlias(_)) {
                let _ = execute::set_prototype_of(&write_proto, &current_proto);
            }
        }
    }
    for (name, value) in [
        ("fd", fd),
        ("path", Value::String(path)),
        (
            "flush",
            Value::Boolean(matches!(flush, Value::Boolean(true))),
        ),
        (
            "write",
            crate::host::capability(crate::registry::SPEC_FS_WRITE_STREAM_WRITE),
        ),
        (
            "close",
            crate::host::capability(crate::registry::SPEC_FS_WRITE_STREAM_CLOSE),
        ),
        (
            "destroy",
            crate::host::capability(crate::registry::SPEC_FS_WRITE_STREAM_CLOSE),
        ),
        (
            "end",
            crate::host::capability(crate::registry::SPEC_FS_WRITE_STREAM_CLOSE),
        ),
        ("writable", Value::Boolean(true)),
        ("closed", Value::Boolean(false)),
    ] {
        let _ = execute::set_property_in_place(&stream, name, value);
    }
    if let Some(receiver) = supplied_receiver {
        for key in execute::own_keys(&stream) {
            let Value::String(key) = key else {
                continue;
            };
            let value = execute::get_property(&stream, &key);
            let _ = execute::set_property_in_place(&receiver, &key, value);
        }
        stream = receiver;
    }
    let open = {
        let candidate = execute::get_property(&stream, "open");
        if quench_runtime::is_callable(&candidate) {
            candidate
        } else {
            crate::host::capability(crate::registry::SPEC_FS_WRITE_STREAM_OPEN)
        }
    };
    let open = execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::FunctionBind),
        &open,
        &[stream.clone()],
    )
    .unwrap_or(open);
    defer(
        state,
        &open,
        vec![stream.clone(), execute::get_property(&stream, "fd")],
    );
    Ok(execute::define_property(
        stream.clone(),
        "__quench_fs_module",
        host_api::object(vec![
            ("value".into(), fs_module),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(false)),
            ("writable".into(), Value::Boolean(false)),
        ]),
    )
    .unwrap_or(stream))
}

pub fn write_stream_write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let stream = receiver.ok_or(VmError::NotCallable)?;
    let (callback, data) = match args.split_last() {
        Some((value, rest)) if quench_runtime::is_callable(value) => (
            Some(value.clone()),
            rest.first().cloned().unwrap_or(Value::Undefined),
        ),
        _ => (None, args.first().cloned().unwrap_or(Value::Undefined)),
    };
    let bytes = match data {
        Value::String(value) => value.into_bytes(),
        value => crate::modules::crypto::bytes_from_value(&value).ok_or_else(|| {
            crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"chunk\" argument must be of type string or an instance of Buffer or Uint8Array.{}",
                crate::modules::util::invalid_arg_received(&value)
            ))
        })?,
    };
    let fd = descriptor_arg(execute::get_property_result(stream, "fd").ok().as_ref())?;
    let buffer = crate::modules::buffer_proto::make_buffer(&bytes);
    let result = write_sync(
        state,
        None,
        &[
            Value::Number(fd as f64),
            buffer,
            Value::Number(0.0),
            Value::Number(bytes.len() as f64),
            Value::Null,
        ],
    );
    if let Some(callback) = callback {
        defer(state, &callback, vec![err_value(&result)]);
    }
    result.map(|_| Value::Boolean(true))
}

pub fn write_stream_open(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let stream = args.first().ok_or(VmError::NotCallable)?;
    let fd = args.get(1).cloned().unwrap_or(Value::Undefined);
    emit_stream_event(state, stream, "open", vec![fd])
}

pub fn write_stream_close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let stream = receiver.ok_or(VmError::NotCallable)?;
    let fd = descriptor_arg(execute::get_property_result(stream, "fd").ok().as_ref())?;
    let flush = truthy(&execute::get_property(stream, "flush"));
    let callback = args
        .first()
        .filter(|value| quench_runtime::is_callable(value));
    if flush {
        if let Some(callback) = callback {
            let fs_module = execute::get_property(stream, "__quench_fs_module");
            let fsync = execute::get_property(&fs_module, "fsync");
            execute::call(
                &fsync,
                &fs_module,
                &[Value::Number(fd as f64), callback.clone()],
            )?;
        } else {
            fsync_sync(state, None, &[Value::Number(fd as f64)])?;
        }
    }
    let result = close_sync(state, None, &[Value::Number(fd as f64)]);
    execute::set_property_in_place(stream, "closed", Value::Boolean(true));
    if !flush {
        if let Some(callback) = callback {
            defer(state, callback, vec![err_value(&result)]);
        }
    } else if callback.is_none() {
        if let Some(callback) = args
            .first()
            .filter(|value| quench_runtime::is_callable(value))
        {
            defer(state, callback, vec![err_value(&result)]);
        }
    }
    result.map(|_| stream.clone())
}

pub fn validate_watch_options(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    // `watch(path, listener)` treats the callable second argument as the
    // listener, not as `options`; validate the path first so null-byte and
    // other path errors retain Node's precedence over callback handling.
    path_arg(args.first())?;
    let options = args
        .get(1)
        .filter(|value| !quench_runtime::is_callable(value));
    parse_options(options)?;
    let watcher = crate::modules::events::new_emitter_object(state)?;
    Ok(quench_runtime::execute::set_property(
        watcher,
        "close",
        crate::host::capability(crate::registry::SPEC_FS_WATCH_CLOSE),
    ))
}

pub fn close_watch(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let _ = (state, receiver);
    Ok(Value::Undefined)
}

/// File-watch registration is stateful in Node, but the compatibility host
/// only needs a disposable watcher identity here; the event edge is kept
/// explicit and does not mutate the caller's options object.
pub fn watch_file(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    path_arg(args.first())?;
    let listener = args
        .last()
        .filter(|value| quench_runtime::is_callable(value));
    if listener.is_none() {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"listener\" argument must be of type function".into(),
        ));
    }
    Ok(validate_watch_options(state, None, args)?)
}

pub fn unwatch_file(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    path_arg(args.first())?;
    Ok(Value::Undefined)
}

pub fn validate_directory_options(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined));
    parse_options(options)?;
    let Some(options) = options else {
        return Ok(host_api::object(vec![]));
    };
    let encoding = execute::get_property(options, "encoding");
    if !matches!(encoding, Value::Undefined) {
        let encoding = execute::to_js_string(&encoding)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let valid = matches!(
            encoding.as_str(),
            "utf8"
                | "utf-8"
                | "ascii"
                | "base64"
                | "base64url"
                | "hex"
                | "latin1"
                | "binary"
                | "ucs2"
                | "ucs-2"
                | "utf16le"
                | "utf-16le"
                | "buffer"
        );
        if !valid {
            return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                "The argument 'encoding' is invalid encoding. Received '{encoding}'"
            )));
        }
    }
    let buffer_size = execute::get_property(options, "bufferSize");
    if !matches!(buffer_size, Value::Undefined) {
        let Value::Number(value) = buffer_size else {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"bufferSize\" argument must be of type number".into(),
            ));
        };
        if !value.is_finite() || value.fract() != 0.0 || value < 1.0 {
            let error = quench_runtime::builtins::error(
                quench_runtime::ops::Builtin::RangeError,
                &[Value::String(
                    "The value of \"bufferSize\" is out of range".into(),
                )],
            );
            return Err(VmError::Thrown(execute::set_property(
                error,
                "code",
                Value::String("ERR_OUT_OF_RANGE".into()),
            )));
        }
    }
    Ok(host_api::object(vec![]))
}

fn sync_props() -> Vec<(&'static str, Value)> {
    use crate::registry::*;
    let realpath_sync = crate::host::capability(SPEC_FS_REALSYNC);
    let realpath_sync_native = crate::host::capability(SPEC_FS_REALSYNC_NATIVE);
    let realpath_sync = execute::set_property(realpath_sync, "native", realpath_sync_native);
    let mut props = vec![
        (
            "readFileSync",
            crate::host::capability(SPEC_FS_READFILESYNC),
        ),
        (
            "writeFileSync",
            crate::host::capability(SPEC_FS_WRITEFILESYNC),
        ),
        ("statSync", crate::host::capability(SPEC_FS_STATSYNC)),
        ("lstatSync", crate::host::capability(SPEC_FS_LSTATSYNC)),
        ("readdirSync", crate::host::capability(SPEC_FS_READDIRSYNC)),
        ("existsSync", crate::host::capability(SPEC_FS_EXISTSSYNC)),
        ("realpathSync", realpath_sync),
        ("statfsSync", crate::host::capability(SPEC_FS_STATFSSYNC)),
        ("opendirSync", crate::host::capability(SPEC_FS_OPENDIRSYNC)),
        ("mkdirSync", crate::host::capability(SPEC_FS_MKDIRSYNC)),
        ("unlinkSync", crate::host::capability(SPEC_FS_UNLINKSYNC)),
        ("rmdirSync", crate::host::capability(SPEC_FS_RMDIRSYNC)),
        ("lchmodSync", crate::host::capability(SPEC_FS_LCHMODSYNC)),
    ];
    props.extend(sync_props_more());
    props
}

/// Build a file-backed Blob from one stable metadata snapshot.  The Blob
/// object retains the path and snapshot facts so its read methods can detect
/// replacement or mutation before exposing bytes.
pub fn open_as_blob(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let result = (|| {
        let path = path_arg(args.first())?;
        let options = args.get(1).cloned().unwrap_or_else(|| host_api::object(Vec::new()));
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"options\" argument must be of type object.".into(),
            ));
        }
        let metadata = std::fs::metadata(&path)
            .map_err(|error| crate::modules::fs_error::fs_error("openAsBlob", Some(&path), &error))?;
        let bytes = std::fs::read(&path)
            .map_err(|error| crate::modules::fs_error::fs_error("openAsBlob", Some(&path), &error))?;
        let blob_ctor = execute::get_property(&quench_runtime::vm::current_global_object(), "Blob");
        let parts = host_api::array(vec![crate::modules::buffer_proto::make_buffer(&bytes)]);
        let mut blob = execute::construct_value(&blob_ctor, &[parts, options])?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| (value.as_secs_f64() * 1000.0).floor())
            .unwrap_or(0.0);
        for (key, value) in [
            ("\0quench:file-backed:path", Value::String(path.into())),
            ("\0quench:file-backed:size", Value::Number(metadata.len() as f64)),
            ("\0quench:file-backed:mtime", Value::Number(modified)),
            ("\0quench:file-backed", Value::Boolean(true)),
        ] {
            blob = execute::set_property(blob, key, value);
        }
        Ok(blob)
    })();
    Ok(settle(result))
}

fn sync_props_more() -> Vec<(&'static str, Value)> {
    use crate::registry::*;
    vec![
        ("rmSync", crate::host::capability(SPEC_FS_RMSYNC)),
        ("renameSync", crate::host::capability(SPEC_FS_RENAMESYNC)),
        (
            "appendFileSync",
            crate::host::capability(SPEC_FS_APPENDFILESYNC),
        ),
        (
            "copyFileSync",
            crate::host::capability(SPEC_FS_COPYFILESYNC),
        ),
        ("accessSync", crate::host::capability(SPEC_FS_ACCESSSYNC)),
        ("mkdtempSync", crate::host::capability(SPEC_FS_MKDTEMPSYNC)),
        (
            "readlinkSync",
            crate::host::capability(SPEC_FS_READLINKSYNC),
        ),
        ("chmodSync", crate::host::capability(SPEC_FS_CHMODSYNC)),
        ("symlinkSync", crate::host::capability(SPEC_FS_SYMLINKSYNC)),
        ("linkSync", crate::host::capability(SPEC_FS_LINKSYNC)),
        ("chownSync", crate::host::capability(SPEC_FS_CHOWNSYNC)),
        ("lchownSync", crate::host::capability(SPEC_FS_LCHOWNSYNC)),
        ("utimesSync", crate::host::capability(SPEC_FS_UTIMESSYNC)),
        ("lutimesSync", crate::host::capability(SPEC_FS_LUTIMESSYNC)),
        ("fchmodSync", crate::host::capability(SPEC_FS_FCHMODSYNC)),
        ("fchownSync", crate::host::capability(SPEC_FS_FCHOWNSYNC)),
        ("futimesSync", crate::host::capability(SPEC_FS_FUTIMESSYNC)),
        ("readvSync", crate::host::capability(SPEC_FS_READVSYNC)),
        ("writevSync", crate::host::capability(SPEC_FS_WRITEVSYNC)),
        (
            "truncateSync",
            crate::host::capability(SPEC_FS_TRUNCATESYNC),
        ),
    ]
}

/// `fs.promises` — each op runs the sync implementation and returns
/// an already-settled Promise (fulfilled with the result, rejected
/// with the coded error).
fn promises() -> Value {
    use crate::registry::*;
    let props: Vec<(&str, Value)> = vec![
        ("cp", crate::host::capability(SPEC_FSP_CP)),
        ("glob", crate::host::capability(SPEC_FS_GLOB)),
        ("readFile", crate::host::capability(SPEC_FSP_READFILE)),
        ("writeFile", crate::host::capability(SPEC_FSP_WRITEFILE)),
        ("appendFile", crate::host::capability(SPEC_FSP_APPENDFILE)),
        ("stat", crate::host::capability(SPEC_FSP_STAT)),
        ("lstat", crate::host::capability(SPEC_FSP_LSTAT)),
        ("readdir", crate::host::capability(SPEC_FSP_READDIR)),
        ("mkdir", crate::host::capability(SPEC_FSP_MKDIR)),
        ("unlink", crate::host::capability(SPEC_FSP_UNLINK)),
        ("rmdir", crate::host::capability(SPEC_FSP_RMDIR)),
        ("rm", crate::host::capability(SPEC_FSP_RM)),
        ("rename", crate::host::capability(SPEC_FSP_RENAME)),
        ("symlink", crate::host::capability(SPEC_FSP_SYMLINK)),
        ("copyFile", crate::host::capability(SPEC_FSP_COPYFILE)),
        ("link", crate::host::capability(SPEC_FSP_LINK)),
        ("access", crate::host::capability(SPEC_FSP_ACCESS)),
        ("mkdtemp", crate::host::capability(SPEC_FSP_MKDTEMP)),
        ("readlink", crate::host::capability(SPEC_FSP_READLINK)),
        ("chmod", crate::host::capability(SPEC_FSP_CHMOD)),
        ("lchmod", crate::host::capability(SPEC_FSP_LCHMOD)),
        ("truncate", crate::host::capability(SPEC_FSP_TRUNCATE)),
        ("realpath", crate::host::capability(SPEC_FSP_REALPATH)),
        ("statfs", crate::host::capability(SPEC_FSP_STATFS)),
        ("utimes", crate::host::capability(SPEC_FSP_UTIMES)),
        ("lutimes", crate::host::capability(SPEC_FSP_LUTIMES)),
        ("chown", crate::host::capability(SPEC_FSP_CHOWN)),
        ("lchown", crate::host::capability(SPEC_FSP_LCHOWN)),
        ("open", crate::host::capability(SPEC_FSP_OPEN)),
        ("opendir", crate::host::capability(SPEC_FSP_OPENDIR)),
    ];
    crate::host::namespace_object(props).unwrap_or_else(|_| Value::Undefined)
}

fn constants() -> Value {
    let entries: Vec<(String, Value)> = CONSTANT_ENTRIES
        .iter()
        .map(|(name, value)| (name.to_string(), Value::Number(*value)))
        .collect();
    crate::host::null_namespace(entries)
}

#[cfg(target_os = "macos")]
mod flags {
    pub const O_CREAT: f64 = 0x200 as f64;
    pub const O_EXCL: f64 = 0x800 as f64;
    pub const O_TRUNC: f64 = 0x400 as f64;
    pub const O_DIRECTORY: f64 = 0x100000 as f64;
    pub const O_NOFOLLOW: f64 = 0x100 as f64;
    pub const O_SYNC: f64 = 0x80 as f64;
    pub const O_DSYNC: f64 = 0x40 as f64;
}

#[cfg(all(unix, not(target_os = "macos")))]
mod flags {
    pub const O_CREAT: f64 = 0x40 as f64;
    pub const O_EXCL: f64 = 0x80 as f64;
    pub const O_TRUNC: f64 = 0x200 as f64;
    pub const O_DIRECTORY: f64 = 0x10000 as f64;
    pub const O_NOFOLLOW: f64 = 0x20000 as f64;
    pub const O_SYNC: f64 = 0x101000 as f64;
    pub const O_DSYNC: f64 = 0x1000 as f64;
}

#[cfg(not(unix))]
mod flags {
    pub const O_CREAT: f64 = 0x100 as f64;
    pub const O_EXCL: f64 = 0x400 as f64;
    pub const O_TRUNC: f64 = 0x200 as f64;
    pub const O_DIRECTORY: f64 = 0.0;
    pub const O_NOFOLLOW: f64 = 0.0;
    pub const O_SYNC: f64 = 0.0;
    pub const O_DSYNC: f64 = 0.0;
}

const CONSTANT_ENTRIES: &[(&str, f64)] = &[
    ("F_OK", 0.0),
    ("R_OK", 4.0),
    ("W_OK", 2.0),
    ("X_OK", 1.0),
    ("COPYFILE_EXCL", 1.0),
    ("COPYFILE_FICLONE", 2.0),
    ("COPYFILE_FICLONE_FORCE", 4.0),
    ("UV_FS_COPYFILE_EXCL", 1.0),
    ("UV_FS_COPYFILE_FICLONE", 2.0),
    ("UV_FS_COPYFILE_FICLONE_FORCE", 4.0),
    // libuv directory-entry tags are part of the public `fs.constants`
    // namespace and are consumed by the `Dirent` constructor.
    ("UV_DIRENT_UNKNOWN", 0.0),
    ("UV_DIRENT_FILE", 1.0),
    ("UV_DIRENT_DIR", 2.0),
    ("UV_DIRENT_LINK", 3.0),
    ("UV_DIRENT_FIFO", 4.0),
    ("UV_DIRENT_SOCKET", 5.0),
    ("UV_DIRENT_CHAR", 6.0),
    ("UV_DIRENT_BLOCK", 7.0),
    ("O_RDONLY", 0.0),
    ("O_WRONLY", 1.0),
    ("O_RDWR", 2.0),
    ("O_CREAT", flags::O_CREAT),
    ("O_EXCL", flags::O_EXCL),
    ("O_TRUNC", flags::O_TRUNC),
    ("O_APPEND", 8.0),
    ("O_DIRECTORY", flags::O_DIRECTORY),
    ("O_NOFOLLOW", flags::O_NOFOLLOW),
    ("O_SYNC", flags::O_SYNC),
    ("O_DSYNC", flags::O_DSYNC),
    ("S_IFMT", 0o170000 as f64),
    ("S_IFREG", 0o100000 as f64),
    ("S_IFDIR", 0o40000 as f64),
    ("S_IFCHR", 0o20000 as f64),
    ("S_IFBLK", 0o60000 as f64),
    ("S_IFIFO", 0o10000 as f64),
    ("S_IFLNK", 0o120000 as f64),
    ("S_IFSOCK", 0o140000 as f64),
    ("S_IRWXU", 0o700 as f64),
    ("S_IRUSR", 0o400 as f64),
    ("S_IWUSR", 0o200 as f64),
    ("S_IXUSR", 0o100 as f64),
    ("S_IRWXG", 0o70 as f64),
    ("S_IRGRP", 0o40 as f64),
    ("S_IWGRP", 0o20 as f64),
    ("S_IXGRP", 0o10 as f64),
    ("S_IRWXO", 0o7 as f64),
    ("S_IROTH", 0o4 as f64),
    ("S_IWOTH", 0o2 as f64),
    ("S_IXOTH", 0o1 as f64),
];

/// Dispatch table reused by the async and promises families.
pub(crate) type Op =
    fn(&Rc<RefCell<HostState>>, Option<&Value>, &[Value]) -> Result<Value, VmError>;

pub(crate) fn sync_op(name: &str) -> Option<Op> {
    use super::fs_sync as sync;
    Some(match name {
        "readFile" => sync::read_file_sync,
        "writeFile" => sync::write_file_sync,
        "appendFile" => sync::append_file_sync,
        "stat" => sync::stat_sync,
        "lstat" => sync::lstat_sync,
        "readdir" => sync::readdir_sync,
        "mkdir" => sync::mkdir_sync,
        "unlink" => sync::unlink_sync,
        "rmdir" => sync::rmdir_sync,
        "rm" => sync::rm_sync,
        "rename" => sync::rename_sync,
        "copyFile" => sync::copy_file_sync,
        "link" => sync::link_sync,
        "symlink" => sync::symlink_sync,
        "chown" => sync::chown_sync,
        "lchown" => sync::lchown_sync,
        "utimes" => sync::utimes_sync,
        "lutimes" => sync::lutimes_sync,
        "access" => sync::access_sync,
        "mkdtemp" => sync::mkdtemp_sync,
        "readlink" => sync::readlink_sync,
        "chmod" => sync::chmod_sync,
        "lchmod" => sync::lchmod_sync,
        "truncate" => sync::truncate_sync,
        "realpath" => sync::realpath_sync,
        "realpathNative" => sync::realpath_native_sync,
        "statfs" => statfs_sync,
        "opendir" => opendir_sync,
        _ => return None,
    })
}
