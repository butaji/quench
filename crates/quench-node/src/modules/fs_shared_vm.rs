use crate::host::NodeHost;
use crate::modules::fs_ops as fs_ops;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::io::Read;

#[path = "fs_shared_vm/stat.rs"]
pub(crate) mod stat;
#[path = "fs_shared_vm/sync.rs"]
pub(crate) mod sync;
#[path = "fs_shared_vm/write_stream.rs"]
pub(crate) mod write_stream;

const PROMISES_FACTORY: &str = quench_js_check::checked_js!(r#"(readFile, stat, lstat, readdir, readlink, realpath, openSync, closeSync, readSync, writeFileSync, appendFileSync) => ({
  readFile: (...args) => Promise.resolve().then(() => readFile(...args)),
  writeFile: (...args) => Promise.resolve().then(() => writeFileSync(...args)),
  appendFile: (...args) => Promise.resolve().then(() => appendFileSync(...args)),
  stat: (...args) => Promise.resolve().then(() => stat(...args)),
  lstat: (...args) => Promise.resolve().then(() => lstat(...args)),
  readdir: (...args) => Promise.resolve().then(() => readdir(...args)),
  readlink: (...args) => Promise.resolve().then(() => readlink(...args)),
  realpath: (...args) => Promise.resolve().then(() => realpath(...args)),
  open: (...args) => Promise.resolve().then(() => {
    if (Buffer.isBuffer(args[0])) args[0] = args[0].toString();
    else if (args[0] instanceof URL) args[0] = args[0].pathname;
    if (typeof args[2] === "string") {
      args[2] = Number.parseInt(args[2], 8);
      if (Number.isNaN(args[2])) {
        const error = new TypeError('The "mode" argument must be a valid integer');
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
    } else if (args[2] != null && typeof args[2] !== "number") {
      const error = new TypeError('The "mode" argument must be of type number.');
      error.code = typeof args[2] === "string" ? "ERR_INVALID_ARG_VALUE" : "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const fd = openSync(...args);
    const handle = {
      fd,
      close: () => Promise.resolve().then(() => closeSync(fd)),
      read: (...readArgs) => Promise.resolve().then(() => {
        let [buffer, offset = 0, length, position = null] = readArgs;
        if (!ArrayBuffer.isView(buffer) && buffer && typeof buffer === "object") {
          const options = buffer;
          buffer = options.buffer;
          offset = options.offset ?? 0;
          length = options.length;
          position = options.position ?? null;
        } else if (offset && typeof offset === "object") {
          const options = offset;
          offset = options.offset ?? 0;
          length = options.length;
          position = options.position ?? null;
        }
        const bytesRead = readSync(fd, buffer, offset, length, position);
        return { bytesRead, buffer };
      }),
    };
    handle[Symbol.asyncDispose] = handle.close;
    handle[Symbol.dispose] = handle.close;
    return handle;
  }),
})"#);

const OPEN_CLOSE_FACTORY: &str = quench_js_check::checked_js!(r#"(openSync, closeSync, readSync) => {
  const assertBuffer = (buffer) => {
    if (ArrayBuffer.isView(buffer)) return;
    const received = buffer === null || buffer === undefined
      ? ` Received ${buffer}`
      : typeof buffer === "number" || typeof buffer === "boolean" || typeof buffer === "string"
        ? ` Received type ${typeof buffer} (${typeof buffer === "string" ? `'${buffer}'` : String(buffer)})`
        : ` Received an instance of ${Array.isArray(buffer) ? "Array" : "Object"}`;
    const error = new TypeError(`The "buffer" argument must be an instance of Buffer, TypedArray, or DataView.${received}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  };
  const validateReadRange = (buffer, offset, length, position) => {
    for (const [name, value] of [["offset", offset], ["length", length]]) {
      if (value != null && typeof value !== "number") {
        const error = new TypeError(`The "${name}" argument must be of type number.`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
    }
    offset ??= 0;
    length ??= buffer.byteLength - offset;
    if (!Number.isInteger(offset) || offset < 0 || offset > buffer.byteLength) {
      const message = offset > buffer.byteLength
        ? `The value of "offset" is out of range. It must be <= ${buffer.byteLength}. Received ${offset}`
        : `The value of "offset" is out of range. It must be an integer. Received ${String(offset)}`;
      const error = new RangeError(message);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (!Number.isInteger(length) || length < 0 || length > buffer.byteLength - offset) {
      const message = length < 0
        ? `The value of "length" is out of range. It must be >= 0. Received ${length}`
        : length > buffer.byteLength - offset
          ? `The value of "length" is out of range. It must be <= ${buffer.byteLength - offset}. Received ${length}`
        : `The value of "length" is out of range. Received ${String(length)}`;
      const error = new RangeError(message);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (position != null && typeof position !== "number" && typeof position !== "bigint") {
      const error = new TypeError('The "position" argument must be of type number or bigint.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (typeof position === "bigint" && position > BigInt(Number.MAX_SAFE_INTEGER)) {
      const error = new RangeError(`The value of "position" is out of range. Received ${String(position)}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (typeof position === "number" && (!Number.isInteger(position) || position < 0 || position > Number.MAX_SAFE_INTEGER)) {
      const error = new RangeError(`The value of "position" is out of range. Received ${String(position)}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return [offset, length];
  };
  const validatePath = (path) => {
    if (typeof path !== "string" && !Buffer.isBuffer(path) && !(path instanceof URL)) {
      const error = new TypeError('The "path" argument must be of type string, Buffer, or URL.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
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
  const readInto = (fd, buffer, offset, length, position) => {
    assertBuffer(buffer);
    length ??= buffer.byteLength - (offset ?? 0);
    if (buffer.byteLength === 0 && length > 0) {
      const name = buffer.constructor?.name || "TypedArray";
      const error = new TypeError(`The argument 'buffer' is empty and cannot be written. Received ${name}(0) []`);
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    [offset, length] = validateReadRange(buffer, offset, length, position);
    const bytesRead = readSync(fd, buffer, offset ?? 0, length, position ?? null);
    return { bytesRead, buffer };
  };
  const api = {
  open(path, flags, mode, callback) {
    if (typeof flags === "function") {
      callback = flags;
      flags = undefined;
      mode = undefined;
    } else if (typeof mode === "function") {
      callback = mode;
      mode = undefined;
    }
    if (typeof callback !== "function") {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    validatePath(path);
    mode = normalizeMode(mode);
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, openSync(path, flags, mode)]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  },
  close(fd, callback) {
    validateFd(fd);
    if (typeof callback !== "function") {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    queueMicrotask(() => {
      try {
        closeSync(fd);
        Reflect.apply(callback, undefined, [null]);
      } catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  },
  read(fd, bufferOrOptions, offsetOrOptions, length, position, callback) {
    validateFd(fd);
    let buffer = bufferOrOptions;
    let offset = 0;
    let readLength;
    let readPosition = null;
    if (!ArrayBuffer.isView(bufferOrOptions)) {
      if (bufferOrOptions && typeof bufferOrOptions === "object" && ArrayBuffer.isView(bufferOrOptions.buffer)) {
        buffer = bufferOrOptions.buffer;
        offset = bufferOrOptions.offset ?? 0;
        readLength = bufferOrOptions.length;
        readPosition = bufferOrOptions.position ?? null;
        callback = offsetOrOptions;
      } else {
        if (bufferOrOptions === undefined || bufferOrOptions === null ||
            (typeof bufferOrOptions === "object" && !Array.isArray(bufferOrOptions))) {
          const options = bufferOrOptions || {};
          buffer = Buffer.alloc(16384);
          offset = options.offset ?? 0;
          readLength = options.length;
          readPosition = options.position ?? null;
          callback = offsetOrOptions;
        } else if (typeof bufferOrOptions === "function") {
          buffer = Buffer.alloc(16384);
          callback = bufferOrOptions;
        } else {
          buffer = bufferOrOptions;
          callback = offsetOrOptions;
        }
      }
    } else if (offsetOrOptions && typeof offsetOrOptions === "object") {
      offset = offsetOrOptions.offset ?? 0;
      readLength = offsetOrOptions.length;
      readPosition = offsetOrOptions.position ?? null;
      callback = length;
    } else {
      if (typeof callback === "function") {
        offset = offsetOrOptions ?? 0;
        readLength = length;
        readPosition = position ?? null;
      } else if (typeof offsetOrOptions === "function") {
        callback = offsetOrOptions;
      } else if (typeof length === "function") {
        offset = offsetOrOptions ?? 0;
        callback = length;
      } else if (typeof position === "function") {
        offset = offsetOrOptions ?? 0;
        readLength = length;
        callback = position;
      } else {
        offset = offsetOrOptions ?? 0;
        readLength = length;
        readPosition = position ?? null;
      }
    }
    assertBuffer(buffer);
    if (typeof callback !== "function") {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    readLength ??= buffer.byteLength - offset;
    const empty = readLength !== 0 && buffer?.byteLength === 0;
    if (empty) {
      const name = buffer.constructor?.name || "TypedArray";
      const error = new TypeError(`The argument 'buffer' is empty and cannot be written. Received ${name}(0) []`);
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    [offset, readLength] = validateReadRange(buffer, offset, readLength, readPosition);
    queueMicrotask(() => {
      try {
        const result = readInto(fd, buffer, offset, readLength, readPosition);
        Reflect.apply(callback, undefined, [null, result.bytesRead, result.buffer]);
      } catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  },
  };
  api.read[Symbol.for("nodejs.util.promisify.custom")] = (fd, ...args) => new Promise((resolve, reject) => {
    api.read(fd, ...args, (error, bytesRead, buffer) => {
      if (error) reject(error);
      else resolve({ bytesRead, buffer });
    });
  });
  return api;
}"#);

const READDIR_FACTORY: &str = quench_js_check::checked_js!(r#"(readDir) => (path, options) => {
  if (typeof path !== "string" && !Buffer.isBuffer(path) && !(path instanceof URL)) {
    const received = path === null ? "null" : path === undefined ? "undefined" : `type ${typeof path}`;
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${received}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = path.pathname;
  const entries = readDir(path);
  if (!options || options.withFileTypes !== true) return entries.map((entry) => entry.name);
  return entries.map((entry) => {
    const dirent = { name: entry.name };
    for (const name of ["isFile", "isDirectory", "isSymbolicLink", "isBlockDevice", "isCharacterDevice", "isFIFO", "isSocket"]) {
      const result = entry[name];
      Object.defineProperty(dirent, name, { value: () => result });
    }
    return dirent;
  });
}"#);

const ASYNC_READDIR_FACTORY: &str = quench_js_check::checked_js!(r#"(readdirSync) => function readdir(path, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof path !== "string" && !Buffer.isBuffer(path) && !(path instanceof URL)) {
    const received = path === null ? "null" : path === undefined ? "undefined" : `type ${typeof path}`;
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${received}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = path.pathname;
  queueMicrotask(() => {
    let entries;
    try {
      entries = readdirSync(path, options);
    } catch (error) {
      Reflect.apply(callback, undefined, [error]);
      return;
    }
    Reflect.apply(callback, undefined, [null, entries]);
  });
}"#);

const ASYNC_READ_FILE_FACTORY: &str = quench_js_check::checked_js!(r#"(readFileSync) => function readFile(path, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const encoding = typeof options === "string" ? options : options?.encoding;
  if (encoding != null && !Buffer.isEncoding(encoding)) {
    const error = new TypeError(`The "${typeof options === "string" ? "encoding" : "options.encoding"}" argument is invalid. Received ${String(encoding)}`);
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = path.pathname;
  queueMicrotask(() => {
    try { Reflect.apply(callback, undefined, [null, readFileSync(path, options)]); }
    catch (error) { Reflect.apply(callback, undefined, [error]); }
  });
}"#);

const ASYNC_WRITE_FILE_FACTORY: &str = quench_js_check::checked_js!(r#"(writeFileSync) => function writeFile(path, data, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = path.pathname;
  queueMicrotask(() => {
    if (options?.signal?.aborted) {
      const error = new Error('The operation was aborted');
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      Reflect.apply(callback, undefined, [error]);
      return;
    }
    try {
      writeFileSync(path, data, options);
      Reflect.apply(callback, undefined, [null]);
    } catch (error) { Reflect.apply(callback, undefined, [error]); }
  });
}"#);

const ASYNC_APPEND_FILE_FACTORY: &str = quench_js_check::checked_js!(r#"(appendFileSync) => function appendFile(path, data, options, callback) {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof data !== "string" && !ArrayBuffer.isView(data)) {
    const error = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = path.pathname;
  queueMicrotask(() => {
    try {
      appendFileSync(path, data, options);
      Reflect.apply(callback, undefined, [null]);
    } catch (error) { Reflect.apply(callback, undefined, [error]); }
  });
}"#);

const REALPATH_FACTORY: &str = quench_js_check::checked_js!(r#"(realpathSync) => {
  function realpath(path, options, callback) {
    if (typeof options === "function") callback = options;
    if (typeof callback !== "function") {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, realpathSync(path)]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  }
  realpath.native = realpath;
  return realpath;
}"#);

const CREATE_READ_STREAM: &str = r#"(openFile, readFileChunk, closeFile, Readable) => {
  function ReadStream(path, options) {
    if (!(this instanceof ReadStream)) return new ReadStream(path, options);

    if (typeof options !== "string" && options != null && typeof options !== "object") {
      const error = new TypeError('The "options" argument must be of type object or string.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (typeof path !== "string" && typeof path !== "number" &&
        !Buffer.isBuffer(path) && !(path instanceof URL)) {
      const error = new TypeError('The "path" argument must be of type string, Buffer, or URL.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const settings = typeof options === "string" ? { encoding: options } : options || {};
    for (const name of ["start", "end"]) {
      const value = settings[name];
      if (value === undefined || (name === "end" && value === Infinity)) continue;
      if (typeof value !== "number") {
        const error = new TypeError(`The "${name}" option must be of type number.`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (!Number.isSafeInteger(value) || value < 0) {
        const error = new RangeError(`The value of "${name}" is out of range.`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
    }
    if (settings.start !== undefined && settings.end !== undefined && settings.start > settings.end) {
      const error = new RangeError('The value of "start" is out of range.');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const autoClose = settings.autoClose !== false;
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
      highWaterMark: settings.highWaterMark,
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
    if (settings.encoding !== undefined) stream.setEncoding(settings.encoding);
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

const EXISTS_API: &str = r#"(statSync, accessHost, chmodHost, fchmodHost) => {
  const validatePath = (path) => {
    if (typeof path === 'string' || Buffer.isBuffer(path) || path instanceof URL) return;
    const received = path === null || path === undefined
      ? ` Received ${path}`
      : typeof path === 'object'
        ? ` Received an instance of ${Array.isArray(path) ? 'Array' : 'Object'}`
        : ` Received type ${typeof path} (${typeof path === 'string' ? `'${path}'` : String(path)})`;
    const error = new TypeError(`The "path" argument must be of type string or an instance of Buffer or URL.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  };
  const normalizePath = (path) =>
    typeof path === 'string' ? path : Buffer.isBuffer(path) ? path.toString() : path instanceof URL ? decodeURIComponent(path.pathname) : path;
  const normalizeMode = (mode) => {
    if (typeof mode === 'string') {
      const parsed = Number.parseInt(mode, 8);
      if (Number.isNaN(parsed) || !/^[0-7]+$/.test(mode)) {
        const error = new TypeError('The "mode" argument must be a valid integer');
        error.code = 'ERR_INVALID_ARG_VALUE';
        throw error;
      }
      mode = parsed;
    }
    if (typeof mode !== 'number') {
      const error = new TypeError('The "mode" argument must be of type number.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(mode) || !Number.isFinite(mode)) {
      const error = new RangeError(`The value of "mode" is out of range. It must be an integer. Received ${String(mode)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    if (mode < 0 || mode > 0xFFFFFFFF) {
      const error = new RangeError(`The value of "mode" is out of range. It must be >= 0 && <= 4294967295. Received ${mode}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    return mode;
  };
  const validateFd = (fd) => {
    if (typeof fd !== 'number') {
      const received = fd === null || fd === undefined
        ? ` Received ${fd}`
        : typeof fd === 'object'
          ? ` Received an instance of ${Array.isArray(fd) ? 'Array' : 'Object'}`
          : ` Received type ${typeof fd} (${typeof fd === 'string' ? `'${fd}'` : String(fd)})`;
      const error = new TypeError(`The "fd" argument must be of type number.${received}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(fd) || !Number.isFinite(fd)) {
      const error = new RangeError(`The value of "fd" is out of range. It must be an integer. Received ${String(fd)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    if (fd < 0 || fd > 0x7FFFFFFF) {
      const error = new RangeError(`The value of "fd" is out of range. It must be >= 0 && <= 2147483647. Received ${fd}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
  };
  function chmodSync(path, mode) {
    validatePath(path);
    return chmodHost(normalizePath(path), normalizeMode(mode));
  }
  function chmod(path, mode, callback) {
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    validatePath(path);
    const normalizedMode = normalizeMode(mode);
    queueMicrotask(() => {
      try {
        chmodHost(normalizePath(path), normalizedMode);
        Reflect.apply(callback, undefined, [null]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  }
  function fchmodSync(fd, mode) {
    validateFd(fd);
    return fchmodHost(fd, normalizeMode(mode));
  }
  function fchmod(fd, mode, callback) {
    validateFd(fd);
    const normalizedMode = normalizeMode(mode);
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try {
        fchmodHost(fd, normalizedMode);
        Reflect.apply(callback, undefined, [null]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  }
  function accessSync(path, mode = 0) {
    validatePath(path);
    if (typeof mode !== 'number') {
      const error = new TypeError('The "mode" argument must be of type number.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isFinite(mode) || mode < 0 || mode > 7) {
      const error = new RangeError('mode is out of range');
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    return accessHost(normalizePath(path), mode);
  }
  function existsSync(path) {
    try {
      statSync(path);
      return true;
    } catch {
      return false;
    }
  }
  function exists(path, callback) {
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => callback(existsSync(path)));
  }
  function access(path, mode, callback) {
    if (typeof mode === 'function') {
      callback = mode;
      mode = 0;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    validatePath(path);
    queueMicrotask(() => {
      try {
        accessSync(path, mode);
        Reflect.apply(callback, undefined, [null]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  }
  return { access, accessSync, chmod, chmodSync, exists, existsSync, fchmod, fchmodSync };
}"#;

const ASYNC_MKDIR_API: &str = r#"(mkdirSync) => {
  const normalizePath = (path) =>
    typeof path === 'string' ? path : Buffer.isBuffer(path) ? path.toString() : path instanceof URL ? path.pathname : path;
  const validatePath = (path) => {
    if (typeof path === 'string' || Buffer.isBuffer(path) || path instanceof URL) return;
    const received = path === null || path === undefined
      ? ` Received ${path}`
      : typeof path === 'object'
        ? ` Received an instance of ${Array.isArray(path) ? 'Array' : 'Object'}`
        : ` Received type ${typeof path} (${String(path)})`;
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  };
  const validateOptions = (options) => {
    if (options == null || typeof options === 'number' || typeof options === 'string') return;
    if (typeof options !== 'object') {
      const error = new TypeError('The "options" argument must be of type object or number.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (options.recursive !== undefined && typeof options.recursive !== 'boolean') {
      const value = options.recursive;
      const received = value === null ? ' Received null' : typeof value === 'object'
        ? ` Received an instance of ${Array.isArray(value) ? 'Array' : 'Object'}`
        : ` Received type ${typeof value} (${String(value)})`;
      const error = new TypeError(`The "options.recursive" property must be of type boolean.${received}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
  };
  return function mkdir(path, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    validatePath(path);
    validateOptions(options);
    queueMicrotask(() => {
      try {
        const createdPath = mkdirSync(normalizePath(path), options);
        Reflect.apply(callback, undefined, [null, createdPath]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  };
}"#;

const ASYNC_RMDIR_API: &str = r#"(rmdirSync) => {
  return function rmdir(path, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (options != null && typeof options !== 'object') {
      const error = new TypeError('The "options" argument must be of type object.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (options?.recursive === true) {
      const error = new TypeError('The recursive option is no longer supported.');
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
    if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
      const error = new TypeError('The "path" argument must be of type string, Buffer, or URL.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try {
        rmdirSync(path);
        Reflect.apply(callback, undefined, [null]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  };
}"#;

const ASYNC_MKDTEMP_API: &str = r#"(mkdtempSync) => {
  return function mkdtemp(prefix, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (typeof prefix !== 'string' && !Buffer.isBuffer(prefix) && !(prefix instanceof URL) &&
        !(ArrayBuffer.isView(prefix) && !(prefix instanceof DataView))) {
      const error = new TypeError('The "prefix" argument must be of type string, Buffer, URL, or Uint8Array.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try {
        Reflect.apply(callback, undefined, [null, mkdtempSync(prefix)]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  };
}"#;

const ASYNC_COPYFILE_API: &str = r#"(copyFileSync) => {
  return function copyFile(source, destination, mode, callback) {
    if (typeof mode === 'function') {
      callback = mode;
      mode = 0;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try {
        copyFileSync(source, destination, mode);
        Reflect.apply(callback, undefined, [null]);
      } catch (error) {
        Reflect.apply(callback, undefined, [error]);
      }
    });
  };
}"#;

const CP_API: &str = r#"(existsSync, statSync, readdirSync, mkdirSync, copyFileSync) => {
  const exists = (path) => {
    try { statSync(path); return true; } catch { return false; }
  };
  const fail = (code, message, path) => {
    const error = new Error(message);
    error.code = code;
    if (path !== undefined) error.path = path;
    throw error;
  };
  function cpSync(source, destination, options = {}) {
    if (options == null || typeof options !== 'object') {
      const error = new TypeError('The "options" argument must be of type object.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (options.recursive !== undefined && typeof options.recursive !== 'boolean') {
      const error = new TypeError('The "options.recursive" property must be of type boolean.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    const sourceStats = statSync(source);
    if (sourceStats.isDirectory()) {
      if (options.recursive !== true) {
        fail('ERR_FS_EISDIR', `Recursive option not enabled, cannot copy a directory: ${source}`);
      }
      if (exists(destination) && !statSync(destination).isDirectory()) {
        fail('ERR_FS_CP_EINVAL', `Cannot overwrite non-directory with directory: ${destination}`);
      }
      mkdirSync(destination, { recursive: true, mode: options.mode });
      for (const name of readdirSync(source)) {
        const src = `${source.replace(/[\\/]$/, '')}/${name}`;
        const dest = `${destination.replace(/[\\/]$/, '')}/${name}`;
        if (typeof options.filter === 'function' && options.filter(src, dest) === false) continue;
        cpSync(src, dest, options);
      }
      return undefined;
    }
    let dest = destination;
    if (exists(destination) && statSync(destination).isDirectory()) {
      const sourceParts = String(source).split(/[\\/]/);
      dest = `${destination.replace(/[\\/]$/, '')}/${sourceParts[sourceParts.length - 1]}`;
    }
    if (exists(dest)) {
      if (options.errorOnExist && options.force === false) {
        fail('ERR_FS_CP_EEXIST', `Target already exists: ${dest}`, dest);
      }
      if (options.force === false) return undefined;
    }
    copyFileSync(source, dest, options.mode || 0);
    return undefined;
  }
  function cp(source, destination, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = {};
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "callback" argument must be of type function.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try { cpSync(source, destination, options || {}); Reflect.apply(callback, undefined, [null]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  }
  const cpPromise = (source, destination, options) =>
    Promise.resolve().then(() => cpSync(source, destination, options || {}));
  return { cp, cpSync, cpPromise };
}"#;

const ASYNC_SYMLINK_API: &str = r#"(symlinkSync) => {
  const validatePath = (value, name) => {
    if (typeof value === 'string' || Buffer.isBuffer(value) || value instanceof URL) return;
    const received = value === null || value === undefined
      ? ` Received ${value}`
      : typeof value === 'object'
        ? ` Received an instance of ${Array.isArray(value) ? 'Array' : 'Object'}`
        : ` Received type ${typeof value} (${String(value)})`;
    const error = new TypeError(`The "${name}" argument must be of type string, Buffer, or URL.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  };
  return function symlink(target, path, type, callback) {
    if (typeof type === 'function') {
      callback = type;
      type = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    validatePath(target, 'target');
    validatePath(path, 'path');
    if (type !== undefined && type !== 'file' && type !== 'dir' && type !== 'junction') {
      const error = new TypeError('The "type" argument must be one of: "dir", "file", "junction".');
      error.code = 'ERR_INVALID_ARG_VALUE';
      throw error;
    }
    queueMicrotask(() => {
      try { symlinkSync(target, path, type); Reflect.apply(callback, undefined, [null]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const ASYNC_LSTAT_API: &str = r#"(lstatSync) => {
  return function lstat(path, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, lstatSync(path)]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const ASYNC_STAT_API: &str = r#"(statSync) => {
  return function stat(path, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, statSync(path)]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const ASYNC_READLINK_API: &str = r#"(readlinkSync) => {
  return function readlink(path, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, readlinkSync(path)]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const UV_FS_SYMLINK_DIR: i32 = 1;
const UV_FS_SYMLINK_JUNCTION: i32 = 2;
const UV_DIRENT_UNKNOWN: i32 = 0;
const UV_DIRENT_FILE: i32 = 1;
const UV_DIRENT_DIR: i32 = 2;
const UV_DIRENT_LINK: i32 = 3;
const UV_DIRENT_FIFO: i32 = 4;
const UV_DIRENT_SOCKET: i32 = 5;
const UV_DIRENT_CHAR: i32 = 6;
const UV_DIRENT_BLOCK: i32 = 7;
const UV_FS_O_FILEMAP: i32 = 0;
const UV_FS_COPYFILE_EXCL: i32 = 1;
const UV_FS_COPYFILE_FICLONE: i32 = 2;
const UV_FS_COPYFILE_FICLONE_FORCE: i32 = 4;
#[cfg(not(target_os = "linux"))]
const S_IRUSR: i32 = 0o400;
#[cfg(not(target_os = "linux"))]
const S_IWUSR: i32 = 0o200;

#[cfg(target_os = "linux")]
const FS_CONSTANT_VALUES: &[(&str, i32)] = &[
    ("UV_FS_SYMLINK_DIR", UV_FS_SYMLINK_DIR),
    ("UV_FS_SYMLINK_JUNCTION", UV_FS_SYMLINK_JUNCTION),
    ("O_RDONLY", libc::O_RDONLY),
    ("O_WRONLY", libc::O_WRONLY),
    ("O_RDWR", libc::O_RDWR),
    ("UV_DIRENT_UNKNOWN", UV_DIRENT_UNKNOWN),
    ("UV_DIRENT_FILE", UV_DIRENT_FILE),
    ("UV_DIRENT_DIR", UV_DIRENT_DIR),
    ("UV_DIRENT_LINK", UV_DIRENT_LINK),
    ("UV_DIRENT_FIFO", UV_DIRENT_FIFO),
    ("UV_DIRENT_SOCKET", UV_DIRENT_SOCKET),
    ("UV_DIRENT_CHAR", UV_DIRENT_CHAR),
    ("UV_DIRENT_BLOCK", UV_DIRENT_BLOCK),
    ("S_IFMT", libc::S_IFMT as i32),
    ("S_IFREG", libc::S_IFREG as i32),
    ("S_IFDIR", libc::S_IFDIR as i32),
    ("S_IFCHR", libc::S_IFCHR as i32),
    ("S_IFBLK", libc::S_IFBLK as i32),
    ("S_IFIFO", libc::S_IFIFO as i32),
    ("S_IFLNK", libc::S_IFLNK as i32),
    ("S_IFSOCK", libc::S_IFSOCK as i32),
    ("O_CREAT", libc::O_CREAT),
    ("O_EXCL", libc::O_EXCL),
    ("UV_FS_O_FILEMAP", UV_FS_O_FILEMAP),
    ("O_NOCTTY", libc::O_NOCTTY),
    ("O_TRUNC", libc::O_TRUNC),
    ("O_APPEND", libc::O_APPEND),
    ("O_DIRECTORY", libc::O_DIRECTORY),
    ("O_NOATIME", libc::O_NOATIME),
    ("O_NOFOLLOW", libc::O_NOFOLLOW),
    ("O_SYNC", libc::O_SYNC),
    ("O_DSYNC", libc::O_DSYNC),
    ("O_DIRECT", libc::O_DIRECT),
    ("O_NONBLOCK", libc::O_NONBLOCK),
    ("S_IRWXU", libc::S_IRWXU as i32),
    ("S_IRUSR", libc::S_IRUSR as i32),
    ("S_IWUSR", libc::S_IWUSR as i32),
    ("S_IXUSR", libc::S_IXUSR as i32),
    ("S_IRWXG", libc::S_IRWXG as i32),
    ("S_IRGRP", libc::S_IRGRP as i32),
    ("S_IWGRP", libc::S_IWGRP as i32),
    ("S_IXGRP", libc::S_IXGRP as i32),
    ("S_IRWXO", libc::S_IRWXO as i32),
    ("S_IROTH", libc::S_IROTH as i32),
    ("S_IWOTH", libc::S_IWOTH as i32),
    ("S_IXOTH", libc::S_IXOTH as i32),
    ("F_OK", libc::F_OK),
    ("R_OK", libc::R_OK),
    ("W_OK", libc::W_OK),
    ("X_OK", libc::X_OK),
    ("UV_FS_COPYFILE_EXCL", UV_FS_COPYFILE_EXCL),
    ("COPYFILE_EXCL", UV_FS_COPYFILE_EXCL),
    ("UV_FS_COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE),
    ("COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE),
    ("UV_FS_COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE),
    ("COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE),
];

#[cfg(not(target_os = "linux"))]
const FS_CONSTANT_VALUES: &[(&str, i32)] = &[
    ("UV_FS_SYMLINK_DIR", UV_FS_SYMLINK_DIR),
    ("UV_FS_SYMLINK_JUNCTION", UV_FS_SYMLINK_JUNCTION),
    ("UV_DIRENT_UNKNOWN", UV_DIRENT_UNKNOWN),
    ("UV_DIRENT_FILE", UV_DIRENT_FILE),
    ("UV_DIRENT_DIR", UV_DIRENT_DIR),
    ("UV_DIRENT_LINK", UV_DIRENT_LINK),
    ("UV_DIRENT_FIFO", UV_DIRENT_FIFO),
    ("UV_DIRENT_SOCKET", UV_DIRENT_SOCKET),
    ("UV_DIRENT_CHAR", UV_DIRENT_CHAR),
    ("UV_DIRENT_BLOCK", UV_DIRENT_BLOCK),
    ("S_IRUSR", S_IRUSR),
    ("S_IWUSR", S_IWUSR),
    ("UV_FS_O_FILEMAP", UV_FS_O_FILEMAP),
    ("UV_FS_COPYFILE_EXCL", UV_FS_COPYFILE_EXCL),
    ("COPYFILE_EXCL", UV_FS_COPYFILE_EXCL),
    ("UV_FS_COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE),
    ("COPYFILE_FICLONE", UV_FS_COPYFILE_FICLONE),
    ("UV_FS_COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE),
    ("COPYFILE_FICLONE_FORCE", UV_FS_COPYFILE_FICLONE_FORCE),
];

fn fs_constants(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let object = get(context, global, "Object")?;
    let create = get(context, object, "create")?;
    let null = context.null();
    let constants = context.call_rooted(create, object, &[null])?;
    for &(name, value) in FS_CONSTANT_VALUES {
        let key = context.string_rooted(name);
        let value = context.number(value as f64);
        if !context.set_property_rooted(constants, key, value, constants)? {
            return Err(RootedError::host("cannot set fs.constants property"));
        }
    }
    Ok(constants)
}

fn make_readdir_sync(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let read_directory =
        context.host_function(crate::host::shared_vm::operation("fsReaddirSync"))?;
    let factory =
        context.evaluate_script_rooted(READDIR_FACTORY, "node:fs/shared-readdir.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[read_directory])
}

fn make_readdir_async(
    context: &mut NativeContext<'_, NodeHost>,
    readdir_sync: RootId,
) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(
        ASYNC_READDIR_FACTORY,
        "node:fs/shared-async-readdir.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[readdir_sync])
}

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let read_file = context.host_function(crate::host::shared_vm::operation("fsReadFileSync"))?;
    set(context, module, "readFileSync", read_file)?;
    let read_file_factory = context.evaluate_script_rooted(
        ASYNC_READ_FILE_FACTORY,
        "node:fs/shared-async-read-file.js",
    )?;
    let undefined = context.undefined();
    let read_file_async = context.call_rooted(read_file_factory, undefined, &[read_file])?;
    set(context, module, "readFile", read_file_async)?;
    let stat_sync = context.host_function(crate::host::shared_vm::operation("fsStatSync"))?;
    set(context, module, "statSync", stat_sync)?;
    let exists_factory = context.evaluate_script_rooted(EXISTS_API, "node:fs/shared-exists.js")?;
    let undefined = context.undefined();
    let access_host = context.host_function(crate::host::shared_vm::operation("fsAccessSync"))?;
    let chmod_host = context.host_function(crate::host::shared_vm::operation("fsChmodSync"))?;
    let fchmod_host = context.host_function(crate::host::shared_vm::operation("fsFchmodSync"))?;
    let exists_api = context.call_rooted(
        exists_factory,
        undefined,
        &[stat_sync, access_host, chmod_host, fchmod_host],
    )?;
    let exists = get(context, exists_api, "exists")?;
    let exists_sync = get(context, exists_api, "existsSync")?;
    let access = get(context, exists_api, "access")?;
    let access_sync = get(context, exists_api, "accessSync")?;
    let chmod = get(context, exists_api, "chmod")?;
    let chmod_sync = get(context, exists_api, "chmodSync")?;
    let fchmod = get(context, exists_api, "fchmod")?;
    let fchmod_sync = get(context, exists_api, "fchmodSync")?;
    set(context, module, "exists", exists)?;
    set(context, module, "existsSync", exists_sync)?;
    set(context, module, "access", access)?;
    set(context, module, "accessSync", access_sync)?;
    set(context, module, "chmod", chmod)?;
    set(context, module, "chmodSync", chmod_sync)?;
    set(context, module, "fchmod", fchmod)?;
    set(context, module, "fchmodSync", fchmod_sync)?;
    let lstat_sync = context.host_function(crate::host::shared_vm::operation("fsLstatSync"))?;
    set(context, module, "lstatSync", lstat_sync)?;
    let lstat_factory = context.evaluate_script_rooted(
        ASYNC_LSTAT_API,
        "node:fs/shared-async-lstat.js",
    )?;
    let lstat = context.call_rooted(lstat_factory, undefined, &[lstat_sync])?;
    set(context, module, "lstat", lstat)?;
    let readdir_sync = make_readdir_sync(context)?;
    set(context, module, "readdirSync", readdir_sync)?;
    let readdir = make_readdir_async(context, readdir_sync)?;
    set(context, module, "readdir", readdir)?;
    let readlink_sync = context.host_function(crate::host::shared_vm::operation("fsReadlinkSync"))?;
    set(context, module, "readlinkSync", readlink_sync)?;
    let readlink_factory = context.evaluate_script_rooted(
        ASYNC_READLINK_API,
        "node:fs/shared-async-readlink.js",
    )?;
    let readlink = context.call_rooted(readlink_factory, undefined, &[readlink_sync])?;
    set(context, module, "readlink", readlink)?;
    let realpath_sync = context.host_function(crate::host::shared_vm::operation("fsRealpathSync"))?;
    set(context, module, "realpathSync", realpath_sync)?;
    let native = context.string_rooted("native");
    if !context.set_property_rooted(realpath_sync, native, realpath_sync, realpath_sync)? {
        return Err(RootedError::host("cannot install fs.realpathSync.native"));
    }
    let realpath_factory = context.evaluate_script_rooted(REALPATH_FACTORY, "node:fs/shared-realpath.js")?;
    let undefined = context.undefined();
    let realpath = context.call_rooted(realpath_factory, undefined, &[realpath_sync])?;
    set(context, module, "realpath", realpath)?;
    let constants = fs_constants(context)?;
    set(context, module, "constants", constants)?;
    stat::install(context, module)?;
    sync::install(context, module)?;
    let stat_factory = context.evaluate_script_rooted(
        ASYNC_STAT_API,
        "node:fs/shared-async-stat.js",
    )?;
    let undefined = context.undefined();
    let stat = context.call_rooted(stat_factory, undefined, &[stat_sync])?;
    set(context, module, "stat", stat)?;
    let mkdir_sync = get(context, module, "mkdirSync")?;
    let mkdir_factory = context.evaluate_script_rooted(
        ASYNC_MKDIR_API,
        "node:fs/shared-async-mkdir.js",
    )?;
    let undefined = context.undefined();
    let mkdir = context.call_rooted(mkdir_factory, undefined, &[mkdir_sync])?;
    set(context, module, "mkdir", mkdir)?;
    let rmdir_sync = get(context, module, "rmdirSync")?;
    let rmdir_factory = context.evaluate_script_rooted(
        ASYNC_RMDIR_API,
        "node:fs/shared-async-rmdir.js",
    )?;
    let rmdir = context.call_rooted(rmdir_factory, undefined, &[rmdir_sync])?;
    set(context, module, "rmdir", rmdir)?;
    let mkdtemp_sync = get(context, module, "mkdtempSync")?;
    let mkdtemp_factory = context.evaluate_script_rooted(
        ASYNC_MKDTEMP_API,
        "node:fs/shared-async-mkdtemp.js",
    )?;
    let mkdtemp = context.call_rooted(mkdtemp_factory, undefined, &[mkdtemp_sync])?;
    set(context, module, "mkdtemp", mkdtemp)?;
    let copy_file_sync = get(context, module, "copyFileSync")?;
    let copy_file_factory = context.evaluate_script_rooted(
        ASYNC_COPYFILE_API,
        "node:fs/shared-async-copyfile.js",
    )?;
    let copy_file = context.call_rooted(copy_file_factory, undefined, &[copy_file_sync])?;
    set(context, module, "copyFile", copy_file)?;
    let cp_factory = context.evaluate_script_rooted(CP_API, "node:fs/shared-cp.js")?;
    let cp_api = context.call_rooted(
        cp_factory,
        undefined,
        &[exists_sync, stat_sync, readdir_sync, mkdir_sync, copy_file_sync],
    )?;
    let cp_sync = get(context, cp_api, "cpSync")?;
    let cp = get(context, cp_api, "cp")?;
    let cp_promise = get(context, cp_api, "cpPromise")?;
    set(context, module, "cpSync", cp_sync)?;
    set(context, module, "cp", cp)?;
    let symlink_sync = get(context, module, "symlinkSync")?;
    let symlink_factory = context.evaluate_script_rooted(
        ASYNC_SYMLINK_API,
        "node:fs/shared-async-symlink.js",
    )?;
    let symlink = context.call_rooted(symlink_factory, undefined, &[symlink_sync])?;
    set(context, module, "symlink", symlink)?;
    let open_sync = get(context, module, "openSync")?;
    let close_sync = get(context, module, "closeSync")?;
    let factory = context.evaluate_script_rooted(OPEN_CLOSE_FACTORY, "node:fs/shared-open-close.js")?;
    let undefined = context.undefined();
    let read_sync = get(context, module, "readSync")?;
    let open_close = context.call_rooted(factory, undefined, &[open_sync, close_sync, read_sync])?;
    let open = get(context, open_close, "open")?;
    let close = get(context, open_close, "close")?;
    let read = get(context, open_close, "read")?;
    set(context, module, "open", open)?;
    set(context, module, "close", close)?;
    set(context, module, "read", read)?;
    let write_file_sync = get(context, module, "writeFileSync")?;
    let write_file_factory = context.evaluate_script_rooted(
        ASYNC_WRITE_FILE_FACTORY,
        "node:fs/shared-async-write-file.js",
    )?;
    let write_file = context.call_rooted(write_file_factory, undefined, &[write_file_sync])?;
    set(context, module, "writeFile", write_file)?;
    let append_file_sync = get(context, module, "appendFileSync")?;
    let append_file_factory = context.evaluate_script_rooted(
        ASYNC_APPEND_FILE_FACTORY,
        "node:fs/shared-async-append-file.js",
    )?;
    let append_file = context.call_rooted(append_file_factory, undefined, &[append_file_sync])?;
    set(context, module, "appendFile", append_file)?;
    let promises = promises_module(
        context,
        constants,
        open_sync,
        close_sync,
        read_sync,
        write_file_sync,
        append_file_sync,
        mkdir_sync,
        rmdir_sync,
        mkdtemp_sync,
        copy_file_sync,
    )?;
    set(context, promises, "cp", cp_promise)?;
    let symlink_promise_factory = context.evaluate_script_rooted(
        "(symlinkSync) => (...args) => Promise.resolve().then(() => symlinkSync(...args))",
        "node:fs/promises/shared-symlink.js",
    )?;
    let symlink_promise =
        context.call_rooted(symlink_promise_factory, undefined, &[symlink_sync])?;
    set(context, promises, "symlink", symlink_promise)?;
    let access_promise_factory = context.evaluate_script_rooted(
        "(accessSync) => (...args) => Promise.resolve().then(() => accessSync(...args))",
        "node:fs/shared-access-promise.js",
    )?;
    let access_promise = context.call_rooted(access_promise_factory, undefined, &[access_sync])?;
    set(context, promises, "access", access_promise)?;
    let chmod_promise_factory = context.evaluate_script_rooted(
        "(chmodSync) => (...args) => Promise.resolve().then(() => chmodSync(...args))",
        "node:fs/shared-chmod-promise.js",
    )?;
    let chmod_promise = context.call_rooted(chmod_promise_factory, undefined, &[chmod_sync])?;
    set(context, promises, "chmod", chmod_promise)?;
    set(context, module, "promises", promises)?;

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

pub(crate) fn promises_module(
    context: &mut NativeContext<'_, NodeHost>,
    constants: RootId,
    open_sync: RootId,
    close_sync: RootId,
    read_sync: RootId,
    write_file_sync: RootId,
    append_file_sync: RootId,
    mkdir_sync: RootId,
    rmdir_sync: RootId,
    mkdtemp_sync: RootId,
    copy_file_sync: RootId,
) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(PROMISES_FACTORY, "node:fs/promises/shared.js")?;
    let read_file = context.host_function(crate::host::shared_vm::operation("fsReadFileSync"))?;
    let stat = context.host_function(crate::host::shared_vm::operation("fsStatSync"))?;
    let lstat = context.host_function(crate::host::shared_vm::operation("fsLstatSync"))?;
    let readdir = make_readdir_sync(context)?;
    let readlink = context.host_function(crate::host::shared_vm::operation("fsReadlinkSync"))?;
    let realpath = context.host_function(crate::host::shared_vm::operation("fsRealpathSync"))?;
    let undefined = context.undefined();
    let promises = context.call_rooted(
        factory,
        undefined,
        &[
            read_file,
            stat,
            lstat,
            readdir,
            readlink,
            realpath,
            open_sync,
            close_sync,
            read_sync,
            write_file_sync,
            append_file_sync,
        ],
    )?;
    set(context, promises, "constants", constants)?;
    let mkdir_factory = context.evaluate_script_rooted(
        "(mkdirSync) => (...args) => Promise.resolve().then(() => mkdirSync(...args))",
        "node:fs/promises/shared-mkdir.js",
    )?;
    let mkdir = context.call_rooted(mkdir_factory, undefined, &[mkdir_sync])?;
    set(context, promises, "mkdir", mkdir)?;
    let rmdir_factory = context.evaluate_script_rooted(
        "(rmdirSync) => (...args) => Promise.resolve().then(() => rmdirSync(...args))",
        "node:fs/promises/shared-rmdir.js",
    )?;
    let rmdir = context.call_rooted(rmdir_factory, undefined, &[rmdir_sync])?;
    set(context, promises, "rmdir", rmdir)?;
    let mkdtemp_factory = context.evaluate_script_rooted(
        "(mkdtempSync) => (...args) => Promise.resolve().then(() => mkdtempSync(...args))",
        "node:fs/promises/shared-mkdtemp.js",
    )?;
    let mkdtemp = context.call_rooted(mkdtemp_factory, undefined, &[mkdtemp_sync])?;
    set(context, promises, "mkdtemp", mkdtemp)?;
    let copy_file_factory = context.evaluate_script_rooted(
        "(copyFileSync) => (...args) => Promise.resolve().then(() => copyFileSync(...args))",
        "node:fs/promises/shared-copyfile.js",
    )?;
    let copy_file = context.call_rooted(copy_file_factory, undefined, &[copy_file_sync])?;
    set(context, promises, "copyFile", copy_file)?;
    Ok(promises)
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
    let path = crate::modules::fs_path::resolve_fixture_path(path);
    let candidate = std::path::Path::new(&path);
    if candidate.is_absolute() || path.starts_with("tests/node/test/") {
        return path;
    }
    let cwd = context.host_mut().shared_state().borrow().cwd.clone();
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
    let (encoding, flag) = if let Some(options) = args.get(1).copied().filter(|options| {
        context
            .rooted_value(*options)
            .is_none_or(|value| !value.is_undefined() && !value.is_null())
    }) {
        if let Some(encoding) = context.string_text(options)? {
            (Some(encoding), None)
        } else {
            let encoding_key = context.string_rooted("encoding");
            let encoding = context.get_property_rooted(options, encoding_key)?;
            let encoding = context.string_text(encoding)?;
            let flag_key = context.string_rooted("flag");
            let flag = context.get_property_rooted(options, flag_key)?;
            let flag = context.string_text(flag)?;
            (encoding, flag)
        }
    } else {
        (None, None)
    };
    let bytes = fs_ops::open(&path, flag.as_deref(), None).and_then(|mut file| {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map(|_| bytes)
    });
    match bytes {
        Ok(bytes) => {
            match encoding.as_deref() {
                None | Some("buffer") => buffer_from_bytes(context, &bytes),
                Some(encoding) => {
                    let buffer = buffer_from_bytes(context, &bytes)?;
                    let to_string = get(context, buffer, "toString")?;
                    let encoding = context.string_rooted(encoding);
                    context.call_rooted(to_string, buffer, &[encoding])
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
    let result = context
        .host_mut()
        .shared_state()
        .borrow()
        .fs
        .open_read_stream(path.clone());
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
        let shared_state = context.host_mut().shared_state();
        let state = shared_state.borrow();
        let path = state
            .fs
            .descriptors()
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
    context
        .host_mut()
        .shared_state()
        .borrow()
        .fs
        .close_read_stream(fd);
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
    } else if error.raw_os_error() == Some(libc::EFBIG) {
        "EFBIG"
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
