//! `fs` module — real filesystem operations with Node's coded
//! errors, `Stats`/`Dirent` values, and async variants whose
//! callbacks run on the host event loop.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub struct FsState {
    next_fd: i32,
    pub(crate) descriptors: HashMap<i32, FileDescriptor>,
}

pub(crate) struct FileDescriptor {
    pub(crate) file: std::fs::File,
    pub(crate) path: String,
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
}

/// Decode Node's shared path input contract once for every fs family.
pub(crate) fn path_arg(value: Option<&Value>) -> Result<String, VmError> {
    let value = value.unwrap_or(&Value::Undefined);
    if let Ok(path) = crate::modules::path::validate_string(value, "path") {
        return Ok(path);
    }
    if let Value::Uint8Array(view) = value {
        let bytes = view.buffer.bytes.borrow();
        let slice = &bytes[view.byte_offset..view.byte_offset + view.length];
        return String::from_utf8(slice.to_vec()).map_err(|_| {
            crate::modules::buffer_enc::invalid_arg_type(
                "The \"path\" argument must be a string, Buffer, or URL".into(),
            )
        });
    }
    if crate::modules::url_whatwg::is_url_instance(value) {
        let parsed = crate::modules::url_whatwg::parsed_of(Some(value))?;
        if parsed.get("protocol") != "file:" || !parsed.get("hostname").is_empty() {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"path\" argument must be a file URL".into(),
            ));
        }
        return decode_percent_path(&parsed.get("pathname"));
    }
    Err(crate::modules::buffer_enc::invalid_arg_type(format!(
        "The \"path\" argument must be of type string or an instance of Buffer or URL.{}",
        crate::modules::util::invalid_arg_received(value)
    )))
}

pub(crate) fn descriptor_arg(value: Option<&Value>) -> Result<i32, VmError> {
    match value {
        Some(Value::Number(fd)) if fd.is_finite() && *fd >= 0.0 && fd.fract() == 0.0 => {
            Ok(*fd as i32)
        }
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
    if view_length == 0 {
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

fn index_arg(value: Option<&Value>, name: &str, default: usize) -> Result<usize, VmError> {
    match value {
        None | Some(Value::Undefined) => Ok(default),
        Some(Value::Number(number))
            if number.is_finite() && *number >= 0.0 && number.fract() == 0.0 =>
        {
            Ok(*number as usize)
        }
        Some(value) => Err(crate::modules::buffer_enc::out_of_range(
            name,
            "an integer",
            &crate::modules::util::inspect(value),
        )),
    }
}

fn io_length_arg(value: Option<&Value>, default: usize) -> Result<usize, VmError> {
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
        "r" => {
            options.read(true);
        }
        "r+" => {
            options.read(true).write(true);
        }
        "w" => {
            options.write(true).create(true).truncate(true);
        }
        "w+" => {
            options.read(true).write(true).create(true).truncate(true);
        }
        "wx+" => {
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
        "wx" => {
            options.write(true).create_new(true);
        }
        "ax" => {
            options.write(true).create_new(true).append(true);
        }
        "ax+" => {
            options.read(true).write(true).create_new(true).append(true);
        }
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
                "The argument 'flags' is invalid. Received {flags:?}"
            )))
        }
    }
    Ok(options)
}

pub fn open_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = path_arg(args.first())?;
    let flags = match args.get(1) {
        None | Some(Value::Undefined) => "r",
        Some(Value::String(flags)) => flags.as_str(),
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
    fs.fs.descriptors.insert(fd, FileDescriptor { file, path });
    Ok(Value::Number(fd as f64))
}

pub fn close_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = descriptor_arg(args.first())?;
    if state.borrow_mut().fs.descriptors.remove(&fd).is_none() {
        return Err(crate::modules::buffer_enc::invalid_arg_value(
            "file descriptor is not valid".into(),
        ));
    }
    Ok(Value::Undefined)
}

pub fn close(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (callback, leading) = args
        .split_last()
        .ok_or_else(|| callback_type_error(&Value::Undefined))?;
    let fd = descriptor_arg(leading.first())?;
    let callback = require_callback(Some(callback))?;
    let result = close_sync(state, None, &[Value::Number(fd as f64)]);
    match result {
        Ok(_) => defer(state, &callback, vec![Value::Null]),
        Err(error) => defer(state, &callback, vec![err_value(&Err(error))]),
    }
    Ok(Value::Undefined)
}

pub fn read_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = descriptor_arg(args.first())?;
    let offset = index_arg(args.get(2), "offset", 0)?;
    let length = io_length_arg(
        args.get(3),
        io_view(args.get(1))?.3.saturating_sub(offset),
    )?;
    let (value, buffer, target) = io_range(args.get(1), offset, length)?;
    let position = match args.get(4) {
        None | Some(Value::Null) | Some(Value::Undefined) => None,
        Some(value) => Some(index_arg(Some(value), "position", 0)? as u64),
    };
    let mut bytes = vec![0; length];
    let count = {
        let mut fs = state.borrow_mut();
        let descriptor = fs.fs.descriptors.get_mut(&fd).ok_or_else(|| {
            crate::modules::buffer_enc::invalid_arg_value("file descriptor is not valid".into())
        })?;
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
    buffer.bytes.borrow_mut()[target..target + count].copy_from_slice(&bytes[..count]);
    let _ = value;
    Ok(Value::Number(count as f64))
}

pub fn write_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = descriptor_arg(args.first())?;
    let offset = index_arg(args.get(2), "offset", 0)?;
    let length = io_length_arg(
        args.get(3),
        io_view(args.get(1))?.3.saturating_sub(offset),
    )?;
    let (_, buffer, target) = io_range(args.get(1), offset, length)?;
    let bytes = buffer.bytes.borrow()[target..target + length].to_vec();
    let position = match args.get(4) {
        None | Some(Value::Null) | Some(Value::Undefined) => None,
        Some(value) => Some(index_arg(Some(value), "position", 0)? as u64),
    };
    let count = {
        let mut fs = state.borrow_mut();
        let descriptor = fs.fs.descriptors.get_mut(&fd).ok_or_else(|| {
            crate::modules::buffer_enc::invalid_arg_value("file descriptor is not valid".into())
        })?;
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
    descriptor_arg(leading.first())?;
    let callback = require_callback(Some(callback))?;
    let count = read_sync(state, None, leading)?;
    defer(
        state,
        &callback,
        vec![
            Value::Null,
            count,
            leading.get(1).cloned().unwrap_or(Value::Undefined),
        ],
    );
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
    descriptor_arg(leading.first())?;
    let callback = require_callback(Some(callback))?;
    let count = write_sync(state, None, leading)?;
    defer(
        state,
        &callback,
        vec![
            Value::Null,
            count,
            leading.get(1).cloned().unwrap_or(Value::Undefined),
        ],
    );
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
        .ok_or_else(|| {
            crate::modules::buffer_enc::invalid_arg_value("file descriptor is not valid".into())
        })?;
    crate::modules::fs_sync::stat_sync(state, None, &[Value::String(path)])
}

pub fn ftruncate_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = descriptor_arg(args.first())?;
    let length = args
        .get(1)
        .and_then(|value| match value {
            Value::Number(value) if *value >= 0.0 => Some(*value as u64),
            _ => None,
        })
        .unwrap_or(0);
    let mut fs = state.borrow_mut();
    let descriptor = fs.fs.descriptors.get_mut(&fd).ok_or_else(|| {
        crate::modules::buffer_enc::invalid_arg_value("file descriptor is not valid".into())
    })?;
    descriptor.file.set_len(length).map_err(|error| {
        crate::modules::fs_error::fs_error("ftruncate", Some(&descriptor.path), &error)
    })?;
    Ok(Value::Undefined)
}

pub fn fsync_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = descriptor_arg(args.first())?;
    let mut fs = state.borrow_mut();
    let descriptor = fs.fs.descriptors.get_mut(&fd).ok_or_else(|| {
        crate::modules::buffer_enc::invalid_arg_value("file descriptor is not valid".into())
    })?;
    descriptor.file.sync_all().map_err(|error| {
        crate::modules::fs_error::fs_error("fsync", Some(&descriptor.path), &error)
    })?;
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
    let path = path_arg(args.first())?;
    Ok(host_api::object(vec![("path".into(), Value::String(path))]))
}

fn settle(result: Result<Value, VmError>) -> Value {
    let state = match result {
        Ok(value) => quench_runtime::value::PromiseState::Fulfilled(value),
        Err(VmError::Thrown(error)) => quench_runtime::value::PromiseState::Rejected(error),
        Err(_) => quench_runtime::value::PromiseState::Rejected(Value::String("I/O error".into())),
    };
    Value::Promise(Rc::new(quench_runtime::value::PromiseData::new(state)))
}

pub fn promises_open(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let fd = open_sync(state, None, args)?;
    let handle = host_api::object(vec![
        ("fd".into(), fd),
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
        .ok_or_else(|| {
            crate::modules::buffer_enc::invalid_arg_value("file descriptor is not valid".into())
        })?;
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
    let mut options = FsOptions::default();
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
    options.throw_if_no_entry = truthy(&get("throwIfNoEntry"));
    if let signal @ Value::Object(_) = get("signal") {
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
    let resource = crate::modules::async_hooks::new_resource(
        state,
        &[Value::String(resource_type.into())],
    )?;
    let callback = crate::modules::domain::current(state)
        .and_then(|domain| crate::modules::domain::bind(state, Some(&domain), &[cb.clone()]).ok())
        .unwrap_or_else(|| cb.clone());
    state
        .borrow()
        .event_loop
        .queue_immediate_with_resource(callback, args, Some(resource));
    Ok(())
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
        ("access", crate::host::capability(SPEC_FS_ACCESS)),
        ("mkdtemp", crate::host::capability(SPEC_FS_MKDTEMP)),
        ("realpath", crate::host::capability(SPEC_FS_REALPATH)),
        ("watch", crate::host::capability(SPEC_FS_WATCH)),
        ("opendir", crate::host::capability(SPEC_FS_OPENDIR)),
        ("readlink", crate::host::capability(SPEC_FS_READLINK)),
        ("chmod", crate::host::capability(SPEC_FS_CHMOD)),
        ("truncate", crate::host::capability(SPEC_FS_TRUNCATE)),
        ("open", crate::host::capability(SPEC_FS_OPEN)),
    ];
    props.extend([
        (
            "createReadStream",
            crate::host::capability(SPEC_FS_CREATE_READSTREAM),
        ),
        (
            "createWriteStream",
            crate::host::capability(SPEC_FS_WRITESTREAM),
        ),
        ("ReadStream", crate::host::capability(SPEC_FS_READSTREAM)),
        ("WriteStream", crate::host::capability(SPEC_FS_WRITESTREAM)),
    ]);
    props.extend(sync_props());
    props.extend([
        ("openSync", crate::host::capability(SPEC_FS_OPENSYNC)),
        ("closeSync", crate::host::capability(SPEC_FS_CLOSESYNC)),
        ("readSync", crate::host::capability(SPEC_FS_READSYNC)),
        ("writeSync", crate::host::capability(SPEC_FS_WRITESYNC)),
        ("read", crate::host::capability(SPEC_FS_READ)),
        ("write", crate::host::capability(SPEC_FS_WRITE)),
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
        ("Dir", crate::host::capability(SPEC_FS_DIR)),
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
    props.push(("constants", constants()));
    props.push(("promises", promises()));
    crate::host::namespace_object(props).unwrap_or_else(|_| Value::Undefined)
}

pub fn open(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (leading, callback) = async_args(args)?;
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
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.get(1).cloned().unwrap_or_else(|| host_api::object(Vec::new()));
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
    let stream = readable_stream(state, &options)?;
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
        if matches!(fd, Value::Number(_)) { fd } else { Value::Null },
    );
    execute::set_property_in_place(&stream, "readable", Value::Boolean(true));
    execute::set_property_in_place(&stream, "closed", Value::Boolean(false));
    execute::set_property_in_place(&stream, "destroyed", Value::Boolean(false));
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
    let length = if matches!(execute::get_property(&options, "encoding"), Value::String(_)) {
        10_000.0
    } else {
        30_000.0
    };
    execute::set_property_in_place(&stream, "length", Value::Number(length));
    let open = crate::host::capability(crate::registry::SPEC_FS_READSTREAM_OPEN);
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
    if let Some(callback) = args.first().filter(|value| quench_runtime::is_callable(value)) {
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
    let options = args.get(2).cloned().unwrap_or_else(|| host_api::object(Vec::new()));
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
            if !matches!(execute::get_property(&options, "autoClose"), Value::Boolean(false)) {
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
    emit_stream_event(state, stream, "end", Vec::new())?;
    if !matches!(execute::get_property(&options, "autoClose"), Value::Boolean(false)) {
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
        Value::Number(value)
            if value.is_finite() && value >= 0.0 && value.fract() == 0.0 =>
        {
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
            if number.is_finite() && number >= 0.0 && number.fract() == 0.0
                && number <= ((1u64 << 53) - 1) as f64 =>
        {
            Ok(Some(number as usize))
        }
        Value::Number(number) if number.is_nan() => Err(stream_range_error(key, &number.to_string())),
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
        &[Value::String(format!("The \"{key}\" option is out of range. Received {received}"))],
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

pub fn validate_watch_options(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    parse_options(args.get(1))?;
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

pub fn validate_directory_options(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    parse_options(args.get(1))?;
    Ok(host_api::object(vec![]))
}

fn sync_props() -> Vec<(&'static str, Value)> {
    use crate::registry::*;
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
        ("realpathSync", crate::host::capability(SPEC_FS_REALSYNC)),
        ("opendirSync", crate::host::capability(SPEC_FS_OPENDIRSYNC)),
        ("mkdirSync", crate::host::capability(SPEC_FS_MKDIRSYNC)),
        ("unlinkSync", crate::host::capability(SPEC_FS_UNLINKSYNC)),
        ("rmdirSync", crate::host::capability(SPEC_FS_RMDIRSYNC)),
    ];
    props.extend(sync_props_more());
    props
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
        ("copyFile", crate::host::capability(SPEC_FSP_COPYFILE)),
        ("access", crate::host::capability(SPEC_FSP_ACCESS)),
        ("mkdtemp", crate::host::capability(SPEC_FSP_MKDTEMP)),
        ("readlink", crate::host::capability(SPEC_FSP_READLINK)),
        ("chmod", crate::host::capability(SPEC_FSP_CHMOD)),
        ("truncate", crate::host::capability(SPEC_FSP_TRUNCATE)),
        ("realpath", crate::host::capability(SPEC_FSP_REALPATH)),
        ("open", crate::host::capability(SPEC_FSP_OPEN)),
    ];
    crate::host::namespace_object(props).unwrap_or_else(|_| Value::Undefined)
}

fn constants() -> Value {
    let entries: Vec<(String, Value)> = CONSTANT_ENTRIES
        .iter()
        .map(|(name, value)| (name.to_string(), Value::Number(*value)))
        .collect();
    host_api::object(entries)
}

#[cfg(target_os = "macos")]
mod flags {
    pub const O_CREAT: f64 = 0x200 as f64;
    pub const O_EXCL: f64 = 0x800 as f64;
    pub const O_TRUNC: f64 = 0x400 as f64;
    pub const O_DIRECTORY: f64 = 0x100000 as f64;
    pub const O_NOFOLLOW: f64 = 0x100 as f64;
}

#[cfg(all(unix, not(target_os = "macos")))]
mod flags {
    pub const O_CREAT: f64 = 0x40 as f64;
    pub const O_EXCL: f64 = 0x80 as f64;
    pub const O_TRUNC: f64 = 0x200 as f64;
    pub const O_DIRECTORY: f64 = 0x10000 as f64;
    pub const O_NOFOLLOW: f64 = 0x20000 as f64;
}

#[cfg(not(unix))]
mod flags {
    pub const O_CREAT: f64 = 0x100 as f64;
    pub const O_EXCL: f64 = 0x400 as f64;
    pub const O_TRUNC: f64 = 0x200 as f64;
    pub const O_DIRECTORY: f64 = 0.0;
    pub const O_NOFOLLOW: f64 = 0.0;
}

const CONSTANT_ENTRIES: &[(&str, f64)] = &[
    ("F_OK", 0.0),
    ("R_OK", 4.0),
    ("W_OK", 2.0),
    ("X_OK", 1.0),
    ("COPYFILE_EXCL", 1.0),
    ("COPYFILE_FICLONE", 2.0),
    ("COPYFILE_FICLONE_FORCE", 4.0),
    ("O_RDONLY", 0.0),
    ("O_WRONLY", 1.0),
    ("O_RDWR", 2.0),
    ("O_CREAT", flags::O_CREAT),
    ("O_EXCL", flags::O_EXCL),
    ("O_TRUNC", flags::O_TRUNC),
    ("O_APPEND", 8.0),
    ("O_DIRECTORY", flags::O_DIRECTORY),
    ("O_NOFOLLOW", flags::O_NOFOLLOW),
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
        "access" => sync::access_sync,
        "mkdtemp" => sync::mkdtemp_sync,
        "readlink" => sync::readlink_sync,
        "chmod" => sync::chmod_sync,
        "truncate" => sync::truncate_sync,
        "realpath" => sync::realpath_sync,
        _ => return None,
    })
}
