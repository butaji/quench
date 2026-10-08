use crate::host::NodeHost;
use crate::modules::{fs_error_details, fs_ops as ops, fs_shared_vm as shared_vm};
use quench_runtime::{NativeContext, RootId, RootedError, Value};

const SYNC_API: &str = r#"(mkdirSync, rmSync, writeFileSync, openSync, closeSync, fstatSync, readDescriptor, writeDescriptor) => {
  const normalizePath = (path) =>
    typeof path === 'string' ? path : Buffer.isBuffer(path) ? path.toString() : path instanceof URL ? path.pathname : path;
  const writeBytes = (data, options) => {
    const encoding = typeof options === 'string' ? options : options?.encoding;
    if (typeof data === 'string') return Buffer.from(data, encoding);
    if (!ArrayBuffer.isView(data)) {
      const error = new TypeError(
        'The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView',
      );
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  };
  const normalizeMode = (mode) => {
    if (typeof mode === "string") {
      const parsed = Number.parseInt(mode, 8);
      if (Number.isNaN(parsed)) {
        const error = new TypeError('The "mode" argument must be a valid integer');
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      return parsed;
    }
    if (mode != null && typeof mode !== "number") {
      const error = new TypeError('The "mode" argument must be of type number.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return mode;
  };
  const validateFd = (fd) => {
    if (typeof fd === "number") return;
    let received;
    if (fd === null || fd === undefined) received = ` Received ${fd}`;
    else if (typeof fd === "object") received = ` Received an instance of ${Array.isArray(fd) ? "Array" : "Object"}`;
    else received = ` Received type ${typeof fd} (${typeof fd === "string" ? `'${fd}'` : String(fd)})`;
    const error = new TypeError(`The "fd" argument must be of type number.${received}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  };
  return {
    mkdirSync(path, options) {
      return mkdirSync(normalizePath(path), options);
    },
    rmSync(path, options) {
      return rmSync(normalizePath(path), options);
    },
    writeFileSync(path, data, options) {
      return writeFileSync(normalizePath(path), writeBytes(data, options), options);
    },
    openSync(path, flags, mode) {
      return openSync(normalizePath(path), flags, normalizeMode(mode));
    },
    closeSync(fd) {
      validateFd(fd);
      return closeSync(fd);
    },
    fstatSync(fd) {
      validateFd(fd);
      return fstatSync(fd);
    },
    readSync(fd, buffer, offset = 0, length, position = null) {
      validateFd(fd);
      if (!ArrayBuffer.isView(buffer)) {
        const error = new TypeError('The "buffer" argument must be an instance of Buffer, TypedArray, or DataView');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      length ??= buffer.byteLength - offset;
      if (buffer.byteLength === 0 && length > 0) {
        const name = buffer.constructor?.name || "TypedArray";
        const error = new TypeError(`The argument 'buffer' is empty and cannot be written. Received ${name}(0) []`);
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      const bytes = readDescriptor(fd, length, position);
      const target = buffer instanceof DataView
        ? new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength)
        : buffer;
      target.set(bytes, offset);
      return bytes.byteLength;
    },
    writeSync(fd, data, offset, length, position) {
      validateFd(fd);
      if (typeof data === "string") {
        const encoding = typeof length === "string" ? length : "utf8";
        const bytes = Buffer.from(data, encoding);
        return writeDescriptor(fd, bytes, offset == null ? null : offset);
      }
      const bytes = writeBytes(data, undefined);
      offset ??= 0;
      length ??= bytes.byteLength - offset;
      return writeDescriptor(fd, bytes.subarray(offset, offset + length), position);
    },
  };
}"#;

pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
) -> Result<(), RootedError> {
    let mkdir = context.host_function(crate::host::shared_vm::operation("fsMkdirSync"))?;
    let rm = context.host_function(crate::host::shared_vm::operation("fsRmSync"))?;
    let write = context.host_function(crate::host::shared_vm::operation("fsWriteFileSync"))?;
    let open = context.host_function(crate::host::shared_vm::operation("fsOpenSync"))?;
    let close = context.host_function(crate::host::shared_vm::operation("fsCloseSync"))?;
    let fstat = context.host_function(crate::host::shared_vm::operation("fsFstatSync"))?;
    let read_descriptor = context.host_function(crate::host::shared_vm::operation("fsReadDescriptor"))?;
    let write_descriptor = context.host_function(crate::host::shared_vm::operation("fsWriteDescriptor"))?;
    let factory = context.evaluate_script_rooted(SYNC_API, "node:fs/shared-sync.js")?;
    let undefined = context.undefined();
    let api = context.call_rooted(
        factory,
        undefined,
        &[
            mkdir,
            rm,
            write,
            open,
            close,
            fstat,
            read_descriptor,
            write_descriptor,
        ],
    )?;
    for (name, method) in [
        ("mkdirSync", "mkdirSync"),
        ("rmSync", "rmSync"),
        ("writeFileSync", "writeFileSync"),
        ("openSync", "openSync"),
        ("closeSync", "closeSync"),
        ("fstatSync", "fstatSync"),
        ("readSync", "readSync"),
        ("writeSync", "writeSync"),
    ] {
        let key = context.string_rooted(method);
        let function = context.get_property_rooted(api, key)?;
        shared_vm::set(context, module, name, function)?;
    }
    Ok(())
}

pub(crate) fn open_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = path_argument(context, args.first().copied())?;
    let flags = args.get(1).copied();
    let mode = args
        .get(2)
        .copied()
        .and_then(|value| context.rooted_value(value))
        .and_then(Value::as_number)
        .map(|value| value as u32);
    let opened = match flags {
        Some(value) if context.rooted_value(value).is_some_and(Value::is_undefined) => {
            ops::open(&path, None, mode)
        }
        Some(value) if context.string_text(value)?.is_some() => {
            let flag = context.string_text(value)?.unwrap_or_default();
            ops::open(&path, Some(&flag), mode)
        }
        Some(value)
            if context
                .rooted_value(value)
                .and_then(Value::as_number)
                .is_some() =>
        {
            let flag = context
                .rooted_value(value)
                .and_then(Value::as_number)
                .unwrap_or_default() as i32;
            ops::open_numeric(&path, flag, mode)
        }
        None => ops::open(&path, None, mode),
        Some(_) => Err(std::io::Error::from(std::io::ErrorKind::InvalidInput)),
    };
    let file = match opened {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            let exception =
                context.type_error_rooted("The \"flags\" argument must be a valid string")?;
            let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
            set(context, exception, "code", code)?;
            return Err(context.throw(exception));
        }
        Err(error) => {
            return Err(super::stream_io_error(context, error, "open", &path)?);
        }
    };
    let fd = match context
        .host_mut()
        .shared_state()
        .borrow()
        .fs
        .insert_descriptor(file, path.clone())
    {
        Ok(fd) => fd,
        Err(error) => return Err(super::stream_io_error(context, error, "open", &path)?),
    };
    Ok(context.number(fd as f64))
}

pub(crate) fn close_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let result = context
        .host_mut()
        .shared_state()
        .borrow()
        .fs
        .close_stream(fd);
    match result {
        Ok(_) => Ok(context.undefined()),
        Err(error) => Err(super::stream_io_error(context, error, "close", "")?),
    }
}

pub(crate) fn read_descriptor(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let size = args
        .get(1)
        .and_then(|value| context.rooted_value(*value))
        .and_then(Value::as_number)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value as usize)
        .unwrap_or(0);
    let position = descriptor_position(context, args.get(2).copied());
    let (result, path) = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        let path = state
            .fs
            .descriptors()
            .get(&fd)
            .map(|descriptor| descriptor.path.clone())
            .unwrap_or_default();
        (state.fs.read_descriptor(fd, size, position), path)
    };
    match result {
        Ok(bytes) => super::buffer_from_bytes(context, &bytes),
        Err(error) => Err(super::stream_io_error(context, error, "read", &path)?),
    }
}

pub(crate) fn write_descriptor(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let bytes = byte_view(context, args.get(1).copied())?;
    let position = descriptor_position(context, args.get(2).copied());
    let (result, path) = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        let path = state
            .fs
            .descriptors()
            .get(&fd)
            .map(|descriptor| descriptor.path.clone())
            .unwrap_or_default();
        (state.fs.write_descriptor(fd, &bytes, position), path)
    };
    match result {
        Ok(written) => Ok(context.number(written as f64)),
        Err(error) => Err(super::stream_io_error(context, error, "write", &path)?),
    }
}

fn descriptor_position(
    context: &NativeContext<'_, NodeHost>,
    value: Option<RootId>,
) -> Option<u64> {
    value
        .and_then(|value| context.rooted_value(value))
        .and_then(Value::as_number)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value as u64)
}

pub(crate) fn mkdir_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = path_argument(context, args.first().copied())?;
    let options = mkdir_options(context, args.get(1).copied())?;
    let first_created = match ops::mkdir(&path, options) {
        Ok(path) => path,
        Err(error) => return throw_operation_error(context, &error),
    };
    match first_created {
        Some(path) => Ok(context.string_rooted(&path)),
        None => Ok(context.undefined()),
    }
}

pub(crate) fn rm_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = path_argument(context, args.first().copied())?;
    let options = rm_options(context, args.get(1).copied())?;
    if let Err(error) = ops::rm(&path, options) {
        return throw_operation_error(context, &error);
    }
    Ok(context.undefined())
}

pub(crate) fn write_file_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = path_argument(context, args.first().copied())?;
    let bytes = byte_view(context, args.get(1).copied())?;
    let options = write_options(context, args.get(2).copied())?;
    if !ops::valid_write_flag(options.flag.as_deref()) {
        return Err(invalid_value(
            context,
            "flag",
            options.flag.as_deref().unwrap_or("w"),
        )?);
    }
    if let Err(error) = ops::write_file(&path, &bytes, options) {
        return throw_operation_error(context, &error);
    }
    Ok(context.undefined())
}

fn path_argument(
    context: &mut NativeContext<'_, NodeHost>,
    path: Option<RootId>,
) -> Result<String, RootedError> {
    let Some(path) = path else {
        return Err(invalid_path(context, "undefined")?);
    };
    let path_text = context.string_text(path)?;
    let Some(path) = path_text else {
        let received = received_type(context, path)?;
        return Err(invalid_path(context, &received)?);
    };
    if path.contains('\0') {
        return Err(invalid_path_value(context, &path)?);
    }
    Ok(shared_vm::resolve_shared_path(context, path))
}

fn mkdir_options(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<ops::MkdirOptions, RootedError> {
    let Some(value) = options else {
        return Ok(ops::MkdirOptions::default());
    };
    let rooted = context.rooted_value(value).unwrap_or(Value::UNDEFINED);
    if rooted.is_undefined() || rooted.is_null() {
        return Ok(ops::MkdirOptions::default());
    }
    if let Some(mode) = rooted.as_number() {
        return Ok(ops::MkdirOptions {
            mode: Some(mode as u32),
            ..ops::MkdirOptions::default()
        });
    }
    if let Some(mode_text) = context.string_text(value)? {
        if let Ok(mode) = u32::from_str_radix(&mode_text, 8) {
            return Ok(ops::MkdirOptions {
                mode: Some(mode),
                ..ops::MkdirOptions::default()
            });
        }
        return Err(invalid_value(context, "options", &mode_text)?);
    }
    if !context.is_object_rooted(value)? {
        return Err(invalid_options_type(context, value)?);
    }

    let mode = property(context, value, "mode")?;
    let recursive = property(context, value, "recursive")?;
    let recursive_value = context.rooted_value(recursive).unwrap_or(Value::UNDEFINED);
    let recursive = match recursive_value.as_bool() {
        Some(value) => value,
        None if recursive_value.is_undefined() => false,
        None => return Err(option_type_error(context, "recursive", recursive)?),
    };
    let mode = context
        .rooted_value(mode)
        .and_then(Value::as_number)
        .map(|mode| mode as u32);
    Ok(ops::MkdirOptions { mode, recursive })
}

fn rm_options(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<ops::RmOptions, RootedError> {
    let Some(value) = options else {
        return Ok(ops::RmOptions::default());
    };
    let rooted = context.rooted_value(value).unwrap_or(Value::UNDEFINED);
    if rooted.is_undefined() || rooted.is_null() {
        return Ok(ops::RmOptions::default());
    }
    if !context.is_object_rooted(value)? {
        return Err(invalid_options_type(context, value)?);
    }

    let recursive = property(context, value, "recursive")?;
    let recursive_value = context.rooted_value(recursive).unwrap_or(Value::UNDEFINED);
    let recursive = match recursive_value.as_bool() {
        Some(value) => value,
        None if recursive_value.is_undefined() => false,
        None => return Err(option_type_error(context, "recursive", recursive)?),
    };
    let force = property(context, value, "force")?;
    let force = context.truthy_rooted(force)?;
    Ok(ops::RmOptions { recursive, force })
}

fn write_options(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<ops::WriteOptions, RootedError> {
    let Some(value) = options else {
        return Ok(ops::WriteOptions::default());
    };
    let rooted = context.rooted_value(value).unwrap_or(Value::UNDEFINED);
    if rooted.is_undefined() || rooted.is_null() {
        return Ok(ops::WriteOptions::default());
    }
    if context.string_text(value)?.is_some() {
        return Ok(ops::WriteOptions {
            flag: None,
            ..ops::WriteOptions::default()
        });
    }
    if !context.is_object_rooted(value)? {
        return Err(invalid_options_type(context, value)?);
    }

    let flag_value = property(context, value, "flag")?;
    let flag = context.string_text(flag_value)?;
    let mode = property(context, value, "mode")?;
    let mode = context
        .rooted_value(mode)
        .and_then(Value::as_number)
        .map(|mode| mode as u32);
    let flush = property(context, value, "flush")?;
    let flush_value = context.rooted_value(flush).unwrap_or(Value::UNDEFINED);
    let flush = match flush_value.as_bool() {
        Some(value) => value,
        None if flush_value.is_undefined() || flush_value.is_null() => false,
        None => return Err(option_type_error(context, "flush", flush)?),
    };
    Ok(ops::WriteOptions { flag, mode, flush })
}

pub(super) fn byte_view(
    context: &mut NativeContext<'_, NodeHost>,
    data: Option<RootId>,
) -> Result<Vec<u8>, RootedError> {
    let Some(data) = data else {
        return Err(invalid_data(context)?);
    };
    let length_root = property(context, data, "byteLength")?;
    let Some(length) = context
        .rooted_value(length_root)
        .and_then(Value::as_number)
        .filter(|length| length.is_finite() && *length >= 0.0 && *length <= usize::MAX as f64)
    else {
        return Err(invalid_data(context)?);
    };
    let length = length as usize;
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        let byte = property(context, data, &index.to_string())?;
        let value = context
            .rooted_value(byte)
            .and_then(Value::as_number)
            .filter(|value| value.is_finite() && (0.0..=255.0).contains(value))
            .ok_or_else(|| RootedError::host("shared fs byte view contained a non-byte"))?;
        bytes.push(value as u8);
    }
    Ok(bytes)
}

fn property(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn invalid_data(context: &mut NativeContext<'_, NodeHost>) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(
        "The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView",
    )?;
    set_string(context, error, "code", "ERR_INVALID_ARG_TYPE")?;
    Ok(context.throw(error))
}

fn invalid_options_type(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<RootedError, RootedError> {
    let received = received_type(context, value)?;
    let error = context.type_error_rooted(&format!(
        "The \"options\" argument must be of type object. Received {received}"
    ))?;
    set_string(context, error, "code", "ERR_INVALID_ARG_TYPE")?;
    Ok(context.throw(error))
}

fn option_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    value: RootId,
) -> Result<RootedError, RootedError> {
    let received = received_type(context, value)?;
    let error = context.type_error_rooted(&format!(
        "The \"options.{name}\" property must be of type boolean. Received {received}"
    ))?;
    set_string(context, error, "code", "ERR_INVALID_ARG_TYPE")?;
    Ok(context.throw(error))
}

fn invalid_path(
    context: &mut NativeContext<'_, NodeHost>,
    received: &str,
) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(&format!(
        "The \"path\" argument must be of type string, Buffer, or URL. Received {received}"
    ))?;
    set_string(context, error, "code", "ERR_INVALID_ARG_TYPE")?;
    Ok(context.throw(error))
}

fn invalid_path_value(
    context: &mut NativeContext<'_, NodeHost>,
    path: &str,
) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(&format!(
        "The \"path\" argument must be a string, Buffer, or URL without null bytes. Received {path:?}"
    ))?;
    set_string(context, error, "code", "ERR_INVALID_ARG_VALUE")?;
    Ok(context.throw(error))
}

fn invalid_value(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    value: &str,
) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(&format!(
        "The argument '{name}' is invalid. Received {value:?}"
    ))?;
    set_string(context, error, "code", "ERR_INVALID_ARG_VALUE")?;
    Ok(context.throw(error))
}

fn received_type(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<String, RootedError> {
    let rooted = context.rooted_value(value).unwrap_or(Value::UNDEFINED);
    if rooted.is_undefined() {
        return Ok("undefined".to_owned());
    }
    if rooted.is_null() {
        return Ok("null".to_owned());
    }
    if let Some(boolean) = rooted.as_bool() {
        return Ok(format!("type boolean ({boolean})"));
    }
    if let Some(number) = rooted.as_number() {
        return Ok(format!("type number ({number})"));
    }
    if let Some(string) = context.string_text(value)? {
        return Ok(format!("type string ({string:?})"));
    }
    if context.is_callable_rooted(value)? {
        return Ok("type function".to_owned());
    }
    Ok("an instance of Object".to_owned())
}

fn throw_operation_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: &ops::OperationError,
) -> Result<RootId, RootedError> {
    let details = fs_error_details::operation_error_details(error);
    let exception = context.error_rooted(&details.message)?;
    if let Some(name) = details.name {
        set_string(context, exception, "name", name)?;
    }
    set_string(context, exception, "code", details.code)?;
    let syscall = context.string_rooted(&details.syscall);
    set(context, exception, "syscall", syscall)?;
    let errno = context.number(details.errno as f64);
    set(context, exception, "errno", errno)?;
    if let Some(path) = details.path {
        let path = context.string_rooted(&path);
        set(context, exception, "path", path)?;
    }
    Err(context.throw(exception))
}

fn set_string(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host("cannot set shared fs error property"))
    }
}
