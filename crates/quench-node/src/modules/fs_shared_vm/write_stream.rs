use super::{resolve_shared_path, set, stream_io_error};
use crate::host::NodeHost;
use crate::modules::fs_ops as ops;
use quench_runtime::{NativeContext, RootId, RootedError};

const CREATE_WRITE_STREAM: &str = r#"(Writable, fsModule) => {
  const flushFile = (fd, fsOps) => new Promise((resolve, reject) => {
    fsOps.fsync(fd, (error) => error ? reject(error) : resolve());
  });
  function WriteStream(path, options) {
    if (!(this instanceof WriteStream)) return new WriteStream(path, options);
    if (options !== undefined && options !== null && typeof options !== "string" && typeof options !== "object") {
      const received = typeof options === "number" || typeof options === "boolean"
        ? `type ${typeof options} (${String(options)})`
        : `type ${typeof options}`;
      const error = new TypeError(`The "options" argument must be of type object. Received ${received}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }

    const settings = typeof options === "string" ? { encoding: options } : (options || {});
    const invalidArg = (message) => Object.assign(new TypeError(message), { code: "ERR_INVALID_ARG_TYPE" });
    const received = (value) => value === null
      ? "null"
      : typeof value === "string"
        ? `type string ('${value}')`
        : typeof value === "object"
          ? `an instance of ${Array.isArray(value) ? "Array" : "Object"}`
          : `type ${typeof value} (${String(value)})`;
    const rangeReceived = (value) => {
      const text = String(value);
      if (typeof value !== "number" || !Number.isInteger(value) || Math.abs(value) <= 0x1_0000_0000) {
        return text;
      }
      const sign = text.startsWith("-") ? "-" : "";
      const digits = sign ? text.slice(1) : text;
      return /^\d+$/.test(digits) ? sign + digits.replace(/\B(?=(\d{3})+(?!\d))/g, "_") : text;
    };
    if (settings.fd !== undefined && settings.fd !== null &&
        typeof settings.fd !== "number" &&
        !(typeof settings.fd === "object" && typeof settings.fd.write === "function")) {
      throw invalidArg(`The "fd" argument must be of type number or FileHandle. Received ${received(settings.fd)}`);
    }
    if (settings.windowsHandle !== undefined) {
      if (settings.fd !== undefined && settings.fd !== null) {
        const error = new Error('The "fd" and "windowsHandle" options cannot be used together.');
        error.code = "ERR_INCOMPATIBLE_OPTION_PAIR";
        throw error;
      }
      if (process.platform !== "win32") {
        const error = new Error('The "windowsHandle" option is only supported on Windows.');
        error.code = "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM";
        throw error;
      }
    }
    if (path !== null && path !== undefined && typeof path !== "string" &&
        !Buffer.isBuffer(path) && !(path instanceof URL)) {
      throw invalidArg(`The "path" argument must be of type string, Buffer, or URL. Received ${received(path)}`);
    }
    if ((path === null || path === undefined) && (settings.fd === undefined || settings.fd === null)) {
      throw invalidArg(`The "path" argument must be of type string, Buffer, or URL. Received ${path}`);
    }
    if (settings.signal !== undefined && !(settings.signal instanceof AbortSignal)) {
      throw invalidArg('The "options.signal" property must be an instance of AbortSignal.');
    }
    if (settings.start !== undefined) {
      if (typeof settings.start !== "number") {
        throw invalidArg('The "start" option must be of type number.');
      }
      if (!Number.isSafeInteger(settings.start) || settings.start < 0) {
        const requirement = Number.isInteger(settings.start)
          ? `>= 0 && <= ${Number.MAX_SAFE_INTEGER}`
          : 'an integer';
        const error = new RangeError(`The value of "start" is out of range. It must be ${requirement}. Received ${rangeReceived(settings.start)}`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
    }
    const fsOps = settings.fs || fsModule;
    if (settings.fs !== undefined) {
      for (const name of ["open", "close", "write"]) {
        if (typeof settings.fs?.[name] !== "function") {
          throw invalidArg(`The "options.fs.${name}" property must be of type function. Received ${received(settings.fs?.[name])}`);
        }
      }
      if (settings.fs.writev !== undefined && typeof settings.fs.writev !== "function") {
        throw invalidArg(`The "options.fs.writev" property must be of type function. Received ${received(settings.fs.writev)}`);
      }
    }
    if (settings.flush !== undefined && settings.flush !== null && typeof settings.flush !== "boolean") {
      const error = new TypeError('The "flush" option must be of type boolean');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const flags = settings.flags || "w";
    const autoClose = settings.autoClose !== false;
    const fileHandle = settings.fd && typeof settings.fd === "object" &&
      typeof settings.fd.write === "function" ? settings.fd : null;
    const suppliedFd = settings.fd !== undefined && settings.fd !== null;
    let phase = "opening";
    let fd = null;
    let pendingWrite = null;
    let pendingFinal = null;
    let pendingDestroy = null;
    let explicitClose = false;
    let closeError = null;
    const stream = this;

    const writePending = (pending) => {
      if (!pending) return;
      try {
        const position = settings.start === undefined ? null : settings.start + stream.bytesWritten;
        if (fileHandle) {
          const result = typeof pending.chunk === "string"
            ? (position === null
              ? fileHandle.write(pending.chunk)
              : fileHandle.write(pending.chunk, position))
            : fileHandle.write(pending.chunk, 0, pending.chunk.byteLength, position);
          Promise.resolve(result).then(({ bytesWritten }) => {
            stream.bytesWritten += bytesWritten;
            pending.callback();
          }, pending.callback);
          return;
        }
        const buffer = Buffer.isBuffer(pending.chunk)
          ? pending.chunk
          : Buffer.from(pending.chunk, pending.encoding || stream.writableEncoding || "utf8");
        let offset = 0;
        let retries = 0;
        const writeNext = () => {
          fsOps.write(fd, buffer, offset, buffer.byteLength - offset,
            position === null ? null : position + offset,
            (error, bytesWritten) => {
              if (error && error.code === "EAGAIN") {
                if (retries++ < 5) {
                  setImmediate(writeNext);
                  return;
                }
                pending.callback(Object.assign(new Error("write failed"), { code: "ERR_SYSTEM_ERROR" }));
                return;
              }
              if (error) {
                pending.callback(error);
                return;
              }
              const written = Number(bytesWritten) || 0;
              stream.bytesWritten += written;
              offset += written;
              if (offset < buffer.byteLength) {
                if (written > 0) retries = 0;
                else retries++;
                if (retries <= 5) {
                  setImmediate(writeNext);
                  return;
                }
              }
              if (offset < buffer.byteLength) {
                pending.callback(Object.assign(new Error("write failed"), { code: "ERR_SYSTEM_ERROR" }));
                return;
              }
              pending.callback();
            });
        };
        writeNext();
      } catch (error) {
        pending.callback(error);
      }
    };

    const writeVector = (chunks, callback) => {
      const buffers = chunks.map(({ chunk, encoding }) => Buffer.isBuffer(chunk)
        ? chunk
        : Buffer.from(chunk, encoding || stream.writableEncoding || "utf8"));
      const position = settings.start === undefined ? null : settings.start + stream.bytesWritten;
      if (typeof fsOps.writev !== "function") {
        let index = 0;
        const writeNext = (error) => {
          if (error || index >= buffers.length) {
            callback(error);
            return;
          }
          const buffer = buffers[index++];
          fsOps.write(fd, buffer, 0, buffer.byteLength,
            position === null ? null : position + stream.bytesWritten,
            (writeError, bytesWritten) => {
              if (!writeError) stream.bytesWritten += Number(bytesWritten) || 0;
              writeNext(writeError);
            });
        };
        writeNext();
        return;
      }
      fsOps.writev(fd, buffers, position, (error, bytesWritten) => {
        if (!error) stream.bytesWritten += Number(bytesWritten) || 0;
        callback(error);
      });
    };

    const closeDescriptor = () => {
      if (fd === null) return;
      const current = fd;
      fd = null;
      stream.fd = null;
      if (fileHandle) return fileHandle.close();
      return new Promise((resolve, reject) => {
        fsOps.close(current, (error) => error ? reject(error) : resolve());
      });
    };

    const closeWithFlush = (error, callback) => {
      const closeAfterFlush = (flushError) => {
        try {
          const closing = closeDescriptor();
          const complete = (closeError) => callback(closeError || error || flushError);
          if (closing && typeof closing.then === "function") {
            closing.then(() => complete(), complete);
          } else {
            complete();
          }
        } catch (closeError) {
          callback(closeError || error || flushError);
        }
      };
      if (settings.flush !== true || fd === null) {
        closeAfterFlush();
        return;
      }
      try {
        const flushing = fileHandle ? fileHandle.sync() : flushFile(fd, fsOps);
        Promise.resolve(flushing).then(() => closeAfterFlush(), closeAfterFlush);
      } catch (flushError) {
        closeAfterFlush(flushError);
      }
    };

    const writableOptions = {
      ...settings,
      defaultEncoding: settings.encoding || settings.defaultEncoding || "utf8",
      autoDestroy: settings.autoDestroy === false ? false : autoClose,
      write(chunk, encoding, callback) {
        if (phase === "opening") {
          pendingWrite = { chunk, encoding, callback };
          return;
        }
        if (phase !== "open") {
          const error = Object.assign(
            new Error("Cannot call write after a stream was destroyed"),
            { code: "ERR_STREAM_DESTROYED" },
          );
          callback(error);
          return;
        }
        writePending({ chunk, encoding, callback });
      },
      writev(chunks, callback) {
        if (phase === "opening") {
          pendingWrite = { chunks, callback, writev: true };
          return;
        }
        if (phase !== "open") {
          callback(Object.assign(
            new Error("Cannot call write after a stream was destroyed"),
            { code: "ERR_STREAM_DESTROYED" },
          ));
          return;
        }
        writeVector(chunks, callback);
      },
      final(callback) {
        if (phase === "opening") {
          pendingFinal = callback;
          return;
        }
        callback();
      },
      destroy(error, callback) {
        if (phase === "opening") {
          pendingDestroy = { error, callback };
          return;
        }
        const complete = (failure) => {
          closeError = error || failure;
          phase = "closed";
          stream.closed = true;
          callback(closeError);
        };
        try {
          if (autoClose || explicitClose) {
            closeWithFlush(error, complete);
            return;
          }
          complete();
        } catch (failure) {
          complete(failure);
        }
      },
    };

    Writable.call(stream, writableOptions);
    stream.path = path;
    stream.flags = flags;
    stream.mode = settings.mode;
    stream._autoClose = autoClose;
    stream.fd = null;
    stream.bytesWritten = 0;
    stream.closed = false;
    stream.pending = true;
    stream._open = () => {
      try {
        fsOps.open(path, flags, settings.mode, (error, openedFd) => {
          if (error) failOpen(error);
          else finishOpen(openedFd);
        });
      } catch (error) {
        failOpen(error);
      }
    };
    stream.close = function close(callback) {
      if (typeof callback === "function") {
        if (stream.closed) setImmediate(() => callback(closeError));
        else stream.once("close", () => callback(closeError));
      }
      explicitClose = true;
      if (!autoClose) stream.once("finish", () => stream.destroy());
      stream.end();
      return stream;
    };

    const failOpen = (error) => {
      phase = "failed";
      stream.pending = false;
      if (pendingWrite) {
        const pending = pendingWrite;
        pendingWrite = null;
        pending.callback(error);
      }
      if (pendingFinal) {
        const callback = pendingFinal;
        pendingFinal = null;
        callback(error);
      }
      if (pendingDestroy) {
        const pending = pendingDestroy;
        pendingDestroy = null;
        closeError = pending.error || error;
        pending.callback(closeError);
      } else {
        stream.destroy(error);
      }
    };

    const finishOpen = (openedFd) => {
      try {
        fd = openedFd;
        stream.fd = fd;
        stream.pending = false;
        phase = "open";
        if (!suppliedFd) stream.emit("open", fd);
        stream.emit("ready");

        if (pendingDestroy) {
          const pending = pendingDestroy;
          pendingDestroy = null;
          if (pendingWrite) {
            const write = pendingWrite;
            pendingWrite = null;
            write.callback(Object.assign(
              new Error("Cannot call write after a stream was destroyed"),
              { code: "ERR_STREAM_DESTROYED" },
            ));
          }
          const complete = (failure) => {
            closeError = pending.error || failure;
            phase = "closed";
            stream.closed = true;
            pending.callback(closeError);
          };
          closeWithFlush(pending.error, complete);
          return;
        }

        if (pendingWrite) {
          const pending = pendingWrite;
          pendingWrite = null;
          if (pending.writev) writeVector(pending.chunks, pending.callback);
          else writePending(pending);
        }
        if (pendingFinal) {
          const callback = pendingFinal;
          pendingFinal = null;
          callback();
        }
      } catch (error) {
        failOpen(error);
      }
    };

    setImmediate(() => {
      if (suppliedFd) {
        finishOpen(fileHandle ? fileHandle.fd : settings.fd);
        return;
      }
      stream.open();
    });
  }

  Object.setPrototypeOf(WriteStream, Writable);
  Object.setPrototypeOf(WriteStream.prototype, Writable.prototype);
  WriteStream.prototype.constructor = WriteStream;
  WriteStream.prototype.open = function open() {
    if (typeof this._open === "function") this._open();
  };
  Object.defineProperty(WriteStream.prototype, "autoClose", {
    configurable: true,
    get() {
      if (this === WriteStream.prototype || !(this instanceof WriteStream)) {
        const error = new TypeError('Cannot read properties of undefined (reading \'autoClose\')');
        error.code = "ERR_INVALID_THIS";
        throw error;
      }
      return this._autoClose;
    },
  });
  return WriteStream;
}"#;

pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
    writable: RootId,
) -> Result<(), RootedError> {
    let factory = context
        .evaluate_script_rooted(CREATE_WRITE_STREAM, "node:fs/shared-create-write-stream.js")?;
    let undefined = context.undefined();
    let constructor = context.call_rooted(factory, undefined, &[writable, module])?;
    set(context, module, "createWriteStream", constructor)?;
    set(context, module, "WriteStream", constructor)
}

pub(crate) fn open(
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
    let flags = args
        .get(1)
        .copied()
        .and_then(|flags| context.string_text(flags).ok().flatten());
    if !ops::valid_write_flag(flags.as_deref()) {
        let error = context.type_error_rooted("The \"flags\" option is invalid")?;
        let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    let result = ops::open_write(&path, flags.as_deref()).and_then(|file| {
        context
            .host_mut()
            .shared_state()
            .borrow()
            .fs
            .insert_descriptor(file, path.clone())
    });
    match result {
        Ok(fd) => Ok(context.number(fd as f64)),
        Err(error) => Err(stream_io_error(context, error, "open", &path)?),
    }
}

pub(crate) fn write(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let bytes = super::sync::byte_view(context, args.get(1).copied())?;
    let position = args
        .get(2)
        .copied()
        .and_then(|value| context.rooted_value(value))
        .and_then(quench_runtime::Value::as_number)
        .filter(|value| value.is_finite() && *value >= 0.0 && value.fract() == 0.0)
        .map(|value| value as u64);
    let (result, path) = {
        let shared_state = context.host_mut().shared_state();
        let state = shared_state.borrow();
        let path = state
            .fs
            .descriptors()
            .get(&fd)
            .map(|descriptor| descriptor.path.clone())
            .unwrap_or_default();
        let result = state.fs.write_all_descriptor(fd, &bytes, position);
        (result, path)
    };
    match result {
        Ok(written) => Ok(context.number(written as f64)),
        Err(error) => Err(stream_io_error(context, error, "write", &path)?),
    }
}

pub(crate) fn close(
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
        Err(error) => Err(stream_io_error(context, error, "close", "")?),
    }
}
