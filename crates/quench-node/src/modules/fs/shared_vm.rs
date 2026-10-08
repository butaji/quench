use crate::host::NodeHost;
use quench_runtime_next::{NativeContext, RootId, RootedError};

#[path = "shared_vm/stat.rs"]
pub(crate) mod stat;
#[path = "shared_vm/sync.rs"]
pub(crate) mod sync;
#[path = "shared_vm/write_stream.rs"]
pub(crate) mod write_stream;

const CREATE_READ_STREAM: &str = r#"(openFile, readFileChunk, closeFile, Readable) => {
  function ReadStream(path, options) {
    if (!(this instanceof ReadStream)) return new ReadStream(path, options);

    const autoClose = options?.autoClose !== false;
    let phase = "idle";
    let fd = null;
    let bytesRead = 0;
    const stream = this;

    const closeFileOnce = () => {
      if (fd === null) return false;
      const current = fd;
      fd = null;
      stream.fd = null;
      closeFile(current);
      return true;
    };

    const fail = (error) => {
      phase = "failed";
      closeFileOnce();
      stream.destroy(error);
    };

    const scheduleRead = (size) => {
      if (phase !== "open") return;
      phase = "reading";
      setImmediate(() => {
        if (stream.destroyed) {
          phase = "destroyed";
          closeFileOnce();
          return;
        }
        try {
          const chunk = readFileChunk(fd, size);
          if (chunk === null) {
            phase = "ended";
            stream.push(null);
            if (autoClose) closeFileOnce();
            return;
          }
          bytesRead += chunk.byteLength;
          stream.bytesRead = bytesRead;
          phase = "open";
          stream.push(chunk);
        } catch (error) {
          fail(error);
        }
      });
    };

    Readable.call(stream, {
      highWaterMark: options?.highWaterMark,
      autoDestroy: autoClose,
      read(size) {
        if (phase === "open") {
          scheduleRead(size);
        } else if (phase === "idle") {
          phase = "opening";
          setImmediate(() => {
            if (stream.destroyed) {
              phase = "destroyed";
              return;
            }
            try {
              fd = openFile(path);
              stream.fd = fd;
              phase = "open";
              stream.emit("open", fd);
              scheduleRead(size);
            } catch (error) {
              fail(error);
            }
          });
        }
      },
      destroy(error, callback) {
        if (phase !== "ended" && phase !== "failed") phase = "destroyed";
        try {
          closeFileOnce();
          callback(error);
        } catch (closeError) {
          callback(error || closeError);
        }
      },
    });
    stream.fd = null;
    stream.bytesRead = 0;
    stream.close = function close(callback) {
      closeFileOnce();
      if (callback) {
        if (stream.closed) setImmediate(callback);
        else stream.once("close", callback);
      }
      stream.destroy();
      return stream;
    };
    if (options?.encoding !== undefined) stream.setEncoding(options.encoding);
    return stream;
  }

  Object.setPrototypeOf(ReadStream, Readable);
  Object.setPrototypeOf(ReadStream.prototype, Readable.prototype);
  ReadStream.prototype.constructor = ReadStream;

  return {
    ReadStream,
    createReadStream(path, options) {
      return ReadStream(path, options);
    },
  };
}"#;

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let read_file = context.host_function(crate::host::shared_vm::operation("fsReadFileSync"))?;
    set(context, module, "readFileSync", read_file)?;
    stat::install(context, module)?;
    sync::install(context, module)?;

    let streams = crate::host::shared_vm::commonjs::stream_module(context)?;
    let readable = get(context, streams, "Readable")?;
    let factory = context
        .evaluate_script_rooted(CREATE_READ_STREAM, "node:fs/shared-create-read-stream.js")?;
    let undefined = context.undefined();
    let open_file = context.host_function(crate::host::shared_vm::operation("fsReadStreamOpen"))?;
    let read_file_chunk =
        context.host_function(crate::host::shared_vm::operation("fsReadStreamRead"))?;
    let close_file =
        context.host_function(crate::host::shared_vm::operation("fsReadStreamClose"))?;
    let stream_api = context.call_rooted(
        factory,
        undefined,
        &[open_file, read_file_chunk, close_file, readable],
    )?;
    let create_read_stream = get(context, stream_api, "createReadStream")?;
    let read_stream = get(context, stream_api, "ReadStream")?;
    set(context, module, "createReadStream", create_read_stream)?;
    set(context, module, "ReadStream", read_stream)?;

    let writable = get(context, streams, "Writable")?;
    write_stream::install(context, module, writable)?;

    Ok(module)
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

pub(crate) fn resolve_shared_path(
    context: &mut NativeContext<'_, NodeHost>,
    path: String,
) -> String {
    let path = crate::modules::fs::resolve_fixture_path(path);
    let candidate = std::path::Path::new(&path);
    if candidate.is_absolute() || path.starts_with("tests/node/test/") {
        return path;
    }
    let cwd = context.host_mut().state().borrow().process.cwd.clone();
    cwd.join(candidate).to_string_lossy().into_owned()
}

pub(super) fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if !context.set_property_rooted(object, key, value, object)? {
        return Err(RootedError::host("cannot install shared fs binding"));
    }
    Ok(())
}

pub(crate) fn read_file_sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args
        .first()
        .copied()
        .map(|path| context.to_string(path))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let path = resolve_shared_path(context, path);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let encoding = if let Some(options) = args.get(1).copied() {
                if let Some(encoding) = context.string_text(options)? {
                    Some(encoding)
                } else {
                    let key = context.string_rooted("encoding");
                    let encoding = context.get_property_rooted(options, key)?;
                    context.string_text(encoding)?
                }
            } else {
                None
            };
            match encoding.as_deref() {
                None | Some("buffer") => buffer_from_bytes(context, &bytes),
                Some("utf8" | "utf-8") => match String::from_utf8(bytes) {
                    Ok(text) => Ok(context.string_rooted(&text)),
                    Err(_) => {
                        let error = context.error_rooted("The input is not valid UTF-8")?;
                        Err(context.throw(error))
                    }
                },
                _ => {
                    let error = context.type_error_rooted(
                        "shared fs.readFileSync currently requires a UTF-8 encoding",
                    )?;
                    let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
                    let property = context.string_rooted("code");
                    if !context.set_property_rooted(error, property, code, error)? {
                        return Err(RootedError::host("cannot set fs error code"));
                    }
                    Err(context.throw(error))
                }
            }
        }
        Err(error) => {
            let code = fs_error_code(error.kind());
            let error = context.error_rooted(&format!(
                "{code}: {}, open '{path}'",
                std::io::Error::from_raw_os_error(error.raw_os_error().unwrap_or(5))
            ))?;
            let code_value = context.string_rooted(code);
            let property = context.string_rooted("code");
            if !context.set_property_rooted(error, property, code_value, error)? {
                return Err(RootedError::host("cannot set fs error code"));
            }
            Err(context.throw(error))
        }
    }
}

pub(crate) fn read_stream_open(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args
        .first()
        .copied()
        .map(|path| context.to_string(path))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let path = resolve_shared_path(context, path);
    let result = {
        let state = context.host_mut().state();
        let result = state.borrow_mut().fs.open_read_stream(path.clone());
        result
    };
    match result {
        Ok(fd) => Ok(context.number(fd as f64)),
        Err(error) => Err(stream_io_error(context, error, "open", &path)?),
    }
}

pub(crate) fn read_stream_read(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = integer_arg(context, args.first().copied(), "file descriptor")?;
    let requested = args
        .get(1)
        .copied()
        .and_then(|arg| context.rooted_value(arg))
        .and_then(|value| value.as_number())
        .filter(|size| size.is_finite() && *size > 0.0)
        .unwrap_or(READ_STREAM_CHUNK_LIMIT as f64) as usize;
    let size = requested.min(READ_STREAM_CHUNK_LIMIT);
    let (result, path) = {
        let state = context.host_mut().state();
        let mut state = state.borrow_mut();
        let path = state
            .fs
            .descriptors
            .get(&fd)
            .map(|descriptor| descriptor.path.clone())
            .unwrap_or_default();
        let result = state.fs.read_stream_chunk(fd, size);
        (result, path)
    };
    match result {
        Ok(Some(bytes)) => buffer_from_bytes(context, &bytes),
        Ok(None) => Ok(context.null()),
        Err(error) => Err(stream_io_error(context, error, "read", &path)?),
    }
}

pub(crate) fn read_stream_close(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = integer_arg(context, args.first().copied(), "file descriptor")?;
    let state = context.host_mut().state();
    state.borrow_mut().fs.close_read_stream(fd);
    Ok(context.undefined())
}

fn buffer_from_bytes(
    context: &mut NativeContext<'_, NodeHost>,
    bytes: &[u8],
) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let buffer = get(context, global, "Buffer")?;
    let from = get(context, buffer, "from")?;
    let values = bytes
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let values = context.array_rooted(&values)?;
    context.call_rooted(from, buffer, &[values])
}

const READ_STREAM_CHUNK_LIMIT: usize = 64 * 1024;

pub(super) fn integer_arg(
    context: &mut NativeContext<'_, NodeHost>,
    argument: Option<RootId>,
    name: &str,
) -> Result<i32, RootedError> {
    let number = argument
        .and_then(|argument| context.rooted_value(argument))
        .and_then(|value| value.as_number())
        .filter(|number| number.is_finite() && number.fract() == 0.0);
    match number.and_then(|number| i32::try_from(number as i64).ok()) {
        Some(number) => Ok(number),
        None => {
            let error = context.type_error_rooted(&format!("invalid internal fs {name}"))?;
            Err(context.throw(error))
        }
    }
}

pub(super) fn stream_io_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: std::io::Error,
    syscall: &str,
    path: &str,
) -> Result<RootedError, RootedError> {
    let code = if error.raw_os_error() == Some(libc::EBADF) {
        "EBADF"
    } else {
        fs_error_code(error.kind())
    };
    let description = match error.kind() {
        std::io::ErrorKind::NotFound => "no such file or directory",
        std::io::ErrorKind::PermissionDenied => "permission denied",
        std::io::ErrorKind::IsADirectory => "illegal operation on a directory",
        std::io::ErrorKind::NotADirectory => "not a directory",
        _ if error.raw_os_error() == Some(libc::EBADF) => "bad file descriptor",
        _ => "input/output error",
    };
    let message = format!("{code}: {description}, {syscall} '{path}'");
    let exception = context.error_rooted(&message)?;
    let code_value = context.string_rooted(code);
    set(context, exception, "code", code_value)?;
    let syscall_value = context.string_rooted(syscall);
    set(context, exception, "syscall", syscall_value)?;
    let path_value = context.string_rooted(path);
    set(context, exception, "path", path_value)?;
    if let Some(errno) = error.raw_os_error() {
        let errno_value = context.number(-f64::from(errno));
        set(context, exception, "errno", errno_value)?;
    }
    Ok(context.throw(exception))
}

fn fs_error_code(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::IsADirectory => "EISDIR",
        std::io::ErrorKind::NotADirectory => "ENOTDIR",
        _ => "EIO",
    }
}
