use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const READ_STREAM_FACTORY: &str = quench_js_check::checked_js!(
    r#"((readFileSync, EventEmitter) => {
  class ReadStream extends EventEmitter {}

  function createReadStream(path, options = undefined) {
    const stream = new ReadStream();
    let text = readFileSync(path, 'utf8');
    let offset = 0;
    let paused = false;
    let scheduled = false;
    let ended = false;
    let encoding = typeof options === 'string' ? options : options?.encoding;
    stream.readable = true;
    stream.readableFlowing = null;
    stream.readableObjectMode = false;
    stream.destroyed = false;

    const schedule = () => {
      if (scheduled || paused || ended || stream.destroyed) return;
      scheduled = true;
      setImmediate(() => {
        scheduled = false;
        if (paused || stream.destroyed) return;
        if (offset < text.length) {
          const chunk = text.slice(offset);
          offset = text.length;
          stream.emit('data', encoding ? chunk : chunk);
          schedule();
        } else {
          ended = true;
          stream.readable = false;
          stream.readableFlowing = false;
          stream.emit('end');
          stream.emit('close');
        }
      });
    };

    const on = stream.on;
    stream.on = function(event, listener) {
      const result = Reflect.apply(on, this, [event, listener]);
      if (event === 'data') stream.resume();
      return result;
    };
    stream.setEncoding = (value) => { encoding = String(value); return stream; };
    stream.read = () => null;
    stream.pause = () => {
      paused = true;
      stream.readableFlowing = false;
      return stream;
    };
    stream.resume = () => {
      paused = false;
      stream.readableFlowing = true;
      schedule();
      return stream;
    };
    stream.destroy = (error = undefined) => {
      if (stream.destroyed) return stream;
      stream.destroyed = true;
      stream.readable = false;
      if (error !== undefined) stream.emit('error', error);
      stream.emit('close');
      return stream;
    };
    stream.pipe = (destination) => {
      const write = (chunk) => {
        if (destination.write(chunk) === false) stream.pause();
      };
      const finish = () => destination.end();
      const fail = (error) => {
        if (typeof destination.destroy === 'function') destination.destroy(error);
        else destination.emit('error', error);
      };
      stream.on('data', write);
      stream.once('end', finish);
      stream.once('error', fail);
      if (typeof destination.once === 'function') {
        destination.once('drain', () => stream.resume());
      }
      if (typeof destination.emit === 'function') destination.emit('pipe', stream);
      stream.resume();
      return destination;
    };
    return stream;
  }
  return { createReadStream, ReadStream };
})"#
);

const STAT_FACTORY: &str = r#"((fsStat) => (path, callback) => {
  fsStat(path, (errorCode, kind, size, mtimeMs) => {
    if (errorCode !== null) {
      const error = {
        name: 'Error',
        message: `${errorCode}: stat '${path}'`,
        code: errorCode,
        path: String(path),
        syscall: 'stat',
      };
      callback(error);
      return;
    }
    const modified = new Date(mtimeMs);
    const stats = {
      dev: 0,
      ino: 0,
      mode: kind === 'directory' ? 0o40755 : 0o100644,
      nlink: 1,
      uid: 0,
      gid: 0,
      rdev: 0,
      size,
      blksize: 4096,
      blocks: Math.ceil(size / 512),
      atimeMs: mtimeMs,
      mtimeMs,
      ctimeMs: mtimeMs,
      birthtimeMs: mtimeMs,
      atime: modified,
      mtime: modified,
      ctime: modified,
      birthtime: modified,
      isFile: () => kind === 'file',
      isDirectory: () => kind === 'directory',
      isSymbolicLink: () => false,
      isBlockDevice: () => false,
      isCharacterDevice: () => false,
      isFIFO: () => false,
      isSocket: () => false,
    };
    callback(null, stats);
  });
})"#;

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let stat = context.host_function(crate::host::shared_vm::operation("fsStat"))?;
    let factory = context.evaluate_script_rooted(STAT_FACTORY, "node:fs/shared-stat.js")?;
    let undefined = context.undefined();
    let stat = context.call_rooted(factory, undefined, &[stat])?;
    let key = context.string_rooted("stat");
    if !context.set_property_rooted(module, key, stat, module)? {
        return Err(RootedError::host("cannot install shared fs stat binding"));
    }
    let function = context.host_function(crate::host::shared_vm::operation("fsReadFileSync"))?;
    let key = context.string_rooted("readFileSync");
    if !context.set_property_rooted(module, key, function, module)? {
        return Err(RootedError::host("cannot install shared fs binding"));
    }
    let global = context.global_root()?;
    let emitter_key = context.string_rooted("__nodeEventEmitter");
    let emitter = context.get_property_rooted(global, emitter_key)?;
    let factory =
        context.evaluate_script_rooted(READ_STREAM_FACTORY, "node:fs/shared-stream.js")?;
    let undefined = context.undefined();
    let api = context.call_rooted(factory, undefined, &[function, emitter])?;
    let create_read_stream_key = context.string_rooted("createReadStream");
    let create_read_stream = context.get_property_rooted(api, create_read_stream_key)?;
    let key = context.string_rooted("createReadStream");
    if !context.set_property_rooted(module, key, create_read_stream, module)? {
        return Err(RootedError::host("cannot install shared fs stream binding"));
    }
    let read_stream_key = context.string_rooted("ReadStream");
    let read_stream = context.get_property_rooted(api, read_stream_key)?;
    let key = context.string_rooted("ReadStream");
    if !context.set_property_rooted(module, key, read_stream, module)? {
        return Err(RootedError::host(
            "cannot install shared fs ReadStream binding",
        ));
    }
    Ok(module)
}

pub(crate) fn stat(
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
    let callback = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("fs.stat requires a callback"))?;
    if !context.is_callable_rooted(callback)? {
        let error = context.type_error_rooted("The callback argument must be of type function")?;
        return Err(context.throw(error));
    }

    let error_code = context.null();
    let kind;
    let size;
    let modified;
    match std::fs::metadata(&path) {
        Ok(metadata) => {
            kind = context.string_rooted(if metadata.is_dir() {
                "directory"
            } else if metadata.is_file() {
                "file"
            } else {
                "other"
            });
            size = context.number(metadata.len() as f64);
            modified = context.number(
                metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0.0, |duration| duration.as_secs_f64() * 1000.0),
            );
        }
        Err(error) => {
            let code = fs_error_code(error.kind());
            let code = context.string_rooted(code);
            let missing_stats = context.undefined();
            return call_stat_callback(context, callback, &[code, missing_stats]);
        }
    }
    let undefined = context.undefined();
    let result = context.call_rooted(callback, undefined, &[error_code, kind, size, modified])?;
    context.release_root(result);
    Ok(context.undefined())
}

fn call_stat_callback(
    context: &mut NativeContext<'_, NodeHost>,
    callback: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let result = context.call_rooted(callback, undefined, args)?;
    context.release_root(result);
    Ok(context.undefined())
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

fn fs_error_code(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::IsADirectory => "EISDIR",
        std::io::ErrorKind::NotADirectory => "ENOTDIR",
        _ => "EIO",
    }
}
