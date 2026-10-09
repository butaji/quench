use super::{resolve_shared_path, set, stream_io_error};
use crate::host::NodeHost;
use crate::modules::fs_ops as ops;
use quench_runtime::{NativeContext, RootId, RootedError};

const CREATE_WRITE_STREAM: &str = r#"(openFile, writeFile, closeFile, Writable) => {
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
    if (settings.flush !== undefined && settings.flush !== null && typeof settings.flush !== "boolean") {
      const error = new TypeError('The "flush" option must be of type boolean');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const flags = settings.flags || "w";
    const autoClose = settings.autoClose !== false;
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
        stream.bytesWritten += writeFile(fd, pending.chunk, position);
        pending.callback();
      } catch (error) {
        pending.callback(error);
      }
    };

    const closeDescriptor = () => {
      if (fd === null) return;
      const current = fd;
      fd = null;
      stream.fd = null;
      closeFile(current);
    };

    const writableOptions = {
      ...settings,
      defaultEncoding: settings.encoding || settings.defaultEncoding || "utf8",
      autoDestroy: settings.autoDestroy === false ? false : autoClose,
      write(chunk, encoding, callback) {
        if (phase === "opening") {
          pendingWrite = { chunk, callback };
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
        writePending({ chunk, callback });
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
        try {
          if (autoClose || explicitClose) closeDescriptor();
          phase = "closed";
          stream.closed = true;
          callback(error);
        } catch (failure) {
          closeError = error || failure;
          phase = "closed";
          stream.closed = true;
          callback(closeError);
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

    setImmediate(() => {
      try {
        fd = suppliedFd ? settings.fd : openFile(path, flags);
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
          try {
            closeDescriptor();
            phase = "closed";
            stream.closed = true;
            pending.callback(pending.error);
          } catch (error) {
            closeError = pending.error || error;
            phase = "closed";
            stream.closed = true;
            pending.callback(closeError);
          }
          return;
        }

        if (pendingWrite) {
          const pending = pendingWrite;
          pendingWrite = null;
          writePending(pending);
        }
        if (pendingFinal) {
          const callback = pendingFinal;
          pendingFinal = null;
          callback();
        }
      } catch (error) {
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
      }
    });
  }

  Object.setPrototypeOf(WriteStream, Writable);
  Object.setPrototypeOf(WriteStream.prototype, Writable.prototype);
  WriteStream.prototype.constructor = WriteStream;
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
    let open = context.host_function(crate::host::shared_vm::operation("fsWriteStreamOpen"))?;
    let write = context.host_function(crate::host::shared_vm::operation("fsWriteStreamWrite"))?;
    let close = context.host_function(crate::host::shared_vm::operation("fsWriteStreamClose"))?;
    let factory = context
        .evaluate_script_rooted(CREATE_WRITE_STREAM, "node:fs/shared-create-write-stream.js")?;
    let undefined = context.undefined();
    let constructor = context.call_rooted(factory, undefined, &[open, write, close, writable])?;
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
        let result = state.fs.write_descriptor(fd, &bytes, position);
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
