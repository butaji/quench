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

const PROMISES_FACTORY: &str = quench_js_check::checked_js!(r#"(readFile, stat, lstat, readdir, readlink, realpath, openSync, closeSync, readSync, writeSync, fstatSync, fchmodSync, fsyncSync, fdatasyncSync, writeFileSync, appendFileSync, readvSync, writevSync, createReadStream) => ({
  readFile: (...args) => Promise.resolve().then(() => {
    const handle = args[0];
    if (handle && typeof handle.readFile === 'function') return handle.readFile(args[1]);
    return readFile(...args);
  }),
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
    // VFS replaces these functions after the promises module is initialized.
    // Keep FileHandles routed through the live fs object.
    const fs = globalThis.__nodeFs;
    const fd = fs.openSync(...args);
    let closePromise;
    let closed = false;
    const ensureOpen = (syscall) => {
      if (closed) {
        throw Object.assign(new Error(`EBADF: bad file descriptor, ${syscall}`), {
          code: "EBADF",
          syscall,
        });
      }
    };
    const listeners = new Map();
    const addListener = (event, listener, once) => {
      if (typeof listener !== "function") throw new TypeError('The "listener" argument must be of type function.');
      const records = listeners.get(event) || [];
      records.push({ listener, once });
      listeners.set(event, records);
    };
    const handle = {
      fd,
      on(event, listener) { addListener(event, listener, false); return this; },
      addListener(event, listener) { addListener(event, listener, false); return this; },
      once(event, listener) { addListener(event, listener, true); return this; },
      removeListener(event, listener) {
        const records = listeners.get(event) || [];
        const index = records.findIndex((record) => record.listener === listener);
        if (index !== -1) records.splice(index, 1);
        if (records.length) listeners.set(event, records); else listeners.delete(event);
        return this;
      },
      off(event, listener) { return this.removeListener(event, listener); },
      emit(event, ...args) {
        const records = listeners.get(event) || [];
        for (const record of [...records]) {
          if (record.once) this.removeListener(event, record.listener);
          Reflect.apply(record.listener, this, args);
        }
        return records.length !== 0;
      },
      close: () => {
        closePromise ||= Promise.resolve().then(() => {
          fs.closeSync(fd);
          closed = true;
          handle.fd = -1;
          handle.emit("close");
        });
        return closePromise;
      },
      read: (...readArgs) => Promise.resolve().then(() => {
        ensureOpen("read");
        let [buffer, offset = 0, length, position = null] = readArgs;
        if (buffer === undefined && readArgs.length === 0) {
          buffer = Buffer.alloc(16 * 1024);
          length = buffer.length;
        }
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
        const bytesRead = fs.readSync(fd, buffer, offset, length, position);
        return { bytesRead, buffer };
      }),
      readFile: async (options) => {
        ensureOpen("read");
        const settings = typeof options === 'string' ? { encoding: options } : (options || {});
        const signal = settings.signal;
        const checkAbort = () => {
          if (!signal?.aborted) return;
          const error = new Error('The operation was aborted');
          error.name = 'AbortError';
          error.code = 'ABORT_ERR';
          throw error;
        };
        checkAbort();
        await new Promise((resolve) => setImmediate(resolve));
        checkAbort();
        const size = fs.fstatSync(fd).size;
        if (size > 0x7FFFFFFF) {
          const error = new RangeError('File size is greater than 2 GiB');
          error.code = 'ERR_FS_FILE_TOO_LARGE';
          throw error;
        }
        const chunks = [];
        let total = 0;
        while (true) {
          checkAbort();
          const remaining = 0x7FFFFFFF - total;
          const buffer = Buffer.alloc(Math.min(64 * 1024, remaining || 1));
          const bytesRead = fs.readSync(fd, buffer, 0, buffer.length, null);
          if (bytesRead === 0) break;
          if (bytesRead > remaining) {
            const error = new RangeError('File size is greater than 2 GiB');
            error.code = 'ERR_FS_FILE_TOO_LARGE';
            throw error;
          }
          chunks.push(buffer.subarray(0, bytesRead));
          total += bytesRead;
        }
        const result = Buffer.concat(chunks, total);
        return settings.encoding ? result.toString(settings.encoding) : result;
      },
      createReadStream: (options) => {
        ensureOpen("read");
        const settings = typeof options === "string" ? { encoding: options } : { ...(options || {}) };
        settings.fd = fd;
        settings.autoClose = false;
        return fs.createReadStream(null, settings);
      },
      write: (...writeArgs) => Promise.resolve().then(() => {
        ensureOpen("write");
        let [data, offset = 0, length, position = null] = writeArgs;
        let buffer = data;
        if (typeof data === 'string') {
          position = offset ?? null;
          buffer = Buffer.from(data, typeof length === 'string' ? length : 'utf8');
          offset = 0;
          length = buffer.length;
        } else if (offset && typeof offset === 'object') {
          const options = offset;
          offset = options.offset ?? 0;
          length = options.length;
          position = options.position ?? null;
        }
        if (!ArrayBuffer.isView(buffer)) {
          const error = new TypeError('The "buffer" argument must be an instance of Buffer, TypedArray, or DataView.');
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
        length ??= buffer.byteLength - offset;
        const bytesWritten = fs.writeSync(fd, buffer, offset, length, position);
        return { bytesWritten, buffer: data };
      }),
      writeFile: async (data, options) => {
        ensureOpen("write");
        const settings = typeof options === 'string' ? { encoding: options } : (options || {});
        const encoding = settings.encoding || 'utf8';
        const signal = settings.signal;
        const checkAbort = () => {
          if (!signal?.aborted) return;
          const error = new Error('The operation was aborted');
          error.name = 'AbortError';
          error.code = 'ABORT_ERR';
          throw error;
        };
        const chunks = [];
        if (typeof data === 'string' || ArrayBuffer.isView(data)) {
          chunks.push(data);
        } else if (data && (typeof data[Symbol.asyncIterator] === 'function' || typeof data[Symbol.iterator] === 'function')) {
          for await (const chunk of data) chunks.push(chunk);
        } else {
          const error = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, DataView, or Iterable.');
          error.code = 'ERR_INVALID_ARG_TYPE';
          throw error;
        }
        await new Promise((resolve) => setImmediate(resolve));
        checkAbort();
        for (const chunk of chunks) {
          checkAbort();
          if (typeof chunk !== 'string' && !ArrayBuffer.isView(chunk)) {
            const error = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView.');
            error.code = 'ERR_INVALID_ARG_TYPE';
            throw error;
          }
          const buffer = typeof chunk === 'string'
            ? Buffer.from(chunk, encoding)
            : Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength);
          let offset = 0;
          while (offset < buffer.length) {
            checkAbort();
            const written = fs.writeSync(fd, buffer, offset, buffer.length - offset, null);
            if (written === 0) break;
            offset += written;
          }
        }
      },
      appendFile: (...args) => handle.writeFile(...args),
      writev: (buffers, position) => Promise.resolve().then(() => ({
        bytesWritten: (ensureOpen("writev"), fs.writevSync(fd, buffers, position)),
        buffers,
      })),
      readv: (buffers, position) => Promise.resolve().then(() => ({
        bytesRead: (ensureOpen("readv"), fs.readvSync(fd, buffers, position)),
        buffers,
      })),
      stat: (...statArgs) => Promise.resolve().then(() => (ensureOpen("fstat"), fs.fstatSync(fd, ...statArgs))),
      chmod: (...chmodArgs) => Promise.resolve().then(() => (ensureOpen("fchmod"), fs.fchmodSync(fd, ...chmodArgs))),
      sync: () => Promise.resolve().then(() => (ensureOpen("fsync"), fs.fsyncSync(fd))),
      datasync: () => Promise.resolve().then(() => (ensureOpen("fdatasync"), fs.fdatasyncSync(fd))),
    };
    handle[Symbol.asyncDispose] = handle.close;
    handle[Symbol.dispose] = handle.close;
    return handle;
  }),
})"#);

const ASYNC_SYNC_API: &str = r#"(fsyncSync, fdatasyncSync) => {
  const create = (sync, syscall) => function(fd, callback) {
    if (typeof fd !== 'number') {
      const received = fd === null || fd === undefined ? ` Received ${fd}` : ` Received type ${typeof fd} (${String(fd)})`;
      const error = new TypeError(`The "fd" argument must be of type number.${received}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(fd) || fd < 0 || fd > 0x7FFFFFFF) {
      const error = new RangeError(`The value of "fd" is out of range. Received ${String(fd)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "callback" argument must be of type function.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try { sync(fd); callback(null); }
      catch (error) { callback(error); }
    });
  };
  return { fsync: create(fsyncSync, 'fsync'), fdatasync: create(fdatasyncSync, 'fdatasync') };
}"#;

const OPEN_CLOSE_FACTORY: &str = quench_js_check::checked_js!(r#"(openSync, closeSync, readSync, fstatSync) => {
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
  fstat(fd, options, callback) {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    validateFd(fd);
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, fstatSync(fd, options)]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
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

const WRITEV_API: &str = quench_js_check::checked_js!(r#"(writeSync) => {
  const validateBuffers = (buffers) => {
    if (!Array.isArray(buffers) || buffers.some((buffer) => !Buffer.isBuffer(buffer) && !(buffer instanceof Uint8Array))) {
      const error = new TypeError('The "buffers" argument must be an Array of Buffer or Uint8Array instances');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
  };
  function writevSync(fd, buffers, position = null) {
    validateBuffers(buffers);
    const bytes = Buffer.concat(buffers);
    if (bytes.length === 0) {
      writeSync(fd, bytes, 0, 0, position);
      return 0;
    }
    return writeSync(fd, bytes, 0, bytes.length, position);
  }
  function writev(fd, buffers, position, callback) {
    if (typeof position === 'function') { callback = position; position = null; }
    if (callback === undefined && typeof position !== 'function') callback = undefined;
    if (typeof callback !== 'function') {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      error.toString = () => `TypeError [ERR_INVALID_ARG_TYPE]: ${error.message}`;
      throw error;
    }
    validateBuffers(buffers);
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, writevSync(fd, buffers, position), buffers]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  }
  return { writev, writevSync };
}"#);

const READV_API: &str = quench_js_check::checked_js!(r#"(readSync) => {
  const validateBuffers = (buffers) => {
    if (!Array.isArray(buffers) || buffers.some((buffer) => !Buffer.isBuffer(buffer) && !(buffer instanceof Uint8Array))) {
      const error = new TypeError('The "buffers" argument must be an Array of Buffer or Uint8Array instances');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
  };
  function readvSync(fd, buffers, position = null) {
    validateBuffers(buffers);
    let bytesRead = 0;
    for (const buffer of buffers) {
      const count = readSync(fd, buffer, 0, buffer.byteLength, position == null ? null : position + bytesRead);
      bytesRead += count;
      if (count < buffer.byteLength) break;
    }
    return bytesRead;
  }
  function readv(fd, buffers, position, callback) {
    if (typeof position === 'function') { callback = position; position = null; }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      error.toString = () => `TypeError [ERR_INVALID_ARG_TYPE]: ${error.message}`;
      throw error;
    }
    validateBuffers(buffers);
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, readvSync(fd, buffers, position), buffers]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  }
  readv[Symbol.for('nodejs.util.promisify.custom')] = (fd, buffers, position = null) =>
    new Promise((resolve, reject) => readv(fd, buffers, position, (error, bytesRead, readBuffers) =>
      error ? reject(error) : resolve({ bytesRead, buffers: readBuffers })));
  return { readv, readvSync };
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

const OPENDIR_API: &str = quench_js_check::checked_js!(r#"(readdirSync) => {
  const states = new WeakMap();
  class Dirent {
    constructor(entry, parentPath) {
      this.name = entry.name;
      this.parentPath = parentPath;
      for (const name of ['isFile', 'isDirectory', 'isSymbolicLink', 'isBlockDevice', 'isCharacterDevice', 'isFIFO', 'isSocket']) {
        Object.defineProperty(this, name, { value: () => entry[name]() });
      }
    }
    isFile() { return false; }
    isDirectory() { return false; }
    isSymbolicLink() { return false; }
    isBlockDevice() { return false; }
    isCharacterDevice() { return false; }
    isFIFO() { return false; }
    isSocket() { return false; }
  }
  const invalidThis = () => {
    const error = new TypeError('Receiver must be an instance of class Dir');
    error.code = 'ERR_INVALID_THIS';
    throw error;
  };
  const dirError = (code, message) => Object.assign(new Error(message), { code });
  const invalidCallback = () => {
    const error = new TypeError('The "callback" argument must be of type function');
    error.code = 'ERR_INVALID_ARG_TYPE';
    error.toString = () => `TypeError [ERR_INVALID_ARG_TYPE]: ${error.message}`;
    return error;
  };
  class Dir {
    constructor(path, entries) {
      states.set(this, { path, entries, index: 0, closed: false, busy: false, pending: 0 });
    }
    get path() {
      const state = states.get(this);
      if (!state) return invalidThis();
      return state.path;
    }
    readSync() {
      const state = states.get(this);
      if (!state) return invalidThis();
      if (state.busy) throw dirError('ERR_DIR_CONCURRENT_OPERATION', 'Directory is already processing a request');
      if (state.closed) throw dirError('ERR_DIR_CLOSED', 'Directory handle was closed');
      const entry = state.entries[state.index++];
      return entry === undefined ? null : entry;
    }
    closeSync() {
      const state = states.get(this);
      if (!state) return invalidThis();
      if (state.busy) throw dirError('ERR_DIR_CONCURRENT_OPERATION', 'Directory is already processing a request');
      if (state.closed) throw dirError('ERR_DIR_CLOSED', 'Directory handle was closed');
      state.closed = true;
    }
    read(callback) {
      if (callback === undefined) return new Promise((resolve, reject) => this.read((error, entry) => error ? reject(error) : resolve(entry)));
      if (typeof callback !== 'function') throw invalidCallback();
      const state = states.get(this);
      if (!state) return invalidThis();
      state.pending++;
      state.busy = true;
      queueMicrotask(() => {
        state.pending--;
        state.busy = state.pending > 0;
        try {
          if (state.closed) throw dirError('ERR_DIR_CLOSED', 'Directory handle was closed');
          const entry = state.entries[state.index++];
          callback(null, entry === undefined ? null : entry);
        } catch (error) { callback(error); }
      });
    }
    close(callback) {
      if (callback === undefined) return new Promise((resolve, reject) => this.close((error) => error ? reject(error) : resolve()));
      if (callback !== undefined && typeof callback !== 'function') throw invalidCallback();
      const state = states.get(this);
      if (!state) return invalidThis();
      const finish = () => {
        if (state.pending > 0) { queueMicrotask(finish); return; }
        try { this.closeSync(); callback(null); } catch (error) { callback(error); }
      };
      queueMicrotask(finish);
    }
    async next() {
      const value = await new Promise((resolve, reject) => this.read((error, entry) => error ? reject(error) : resolve(entry)));
      if (value === null) { await this.close(); return { done: true, value: undefined }; }
      return { done: false, value };
    }
    async return() {
      const state = states.get(this);
      if (!state.closed) await this.close();
      return { done: true, value: undefined };
    }
    [Symbol.asyncIterator]() { return this; }
  }
  const makeDir = (path, options) => {
    if (options != null && typeof options !== 'object') throw Object.assign(new TypeError('The "options" argument must be of type object'), { code: 'ERR_INVALID_ARG_TYPE' });
    const bufferSize = options && Object.prototype.hasOwnProperty.call(options, 'bufferSize') ? options.bufferSize : 32;
    if (typeof bufferSize !== 'number') throw Object.assign(new TypeError('The "bufferSize" argument must be of type number.'), { code: 'ERR_INVALID_ARG_TYPE' });
    if (!Number.isInteger(bufferSize) || bufferSize < 1) throw Object.assign(new RangeError('The value of "bufferSize" is out of range. It must be >= 1'), { code: 'ERR_OUT_OF_RANGE' });
    const entries = readdirSync(path, { withFileTypes: true }).map((entry) => new Dirent(entry, path));
    return new Dir(path, entries);
  };
  function opendirSync(path, options) { return makeDir(path, options); }
  function opendir(path, options, callback) {
    if (typeof options === 'function') { callback = options; options = undefined; }
    if (typeof callback !== 'function') {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      error.toString = () => `TypeError [ERR_INVALID_ARG_TYPE]: ${error.message}`;
      throw error;
    }
    if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) throw Object.assign(new TypeError('The "path" argument must be of type string, Buffer, or URL.'), { code: 'ERR_INVALID_ARG_TYPE' });
    if (Buffer.isBuffer(path)) path = path.toString();
    else if (path instanceof URL) path = decodeURIComponent(path.pathname);
    queueMicrotask(() => { try { callback(null, makeDir(path, options)); } catch (error) { callback(error); } });
  }
  const opendirPromise = (path, options) => Promise.resolve().then(() => makeDir(path, options));
  return { Dir, Dirent, opendir, opendirSync, opendirPromise };
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

const REALPATH_SYNC_API: &str = r#"(hostRealpathSync) => function realpathSync(path, options) {
  if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${typeof path === 'string' ? `'${path}'` : String(path)})`}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = decodeURIComponent(path.pathname);
  const resolved = hostRealpathSync(path);
  const encoding = typeof options === 'string' ? options : options?.encoding;
  if (encoding === 'buffer') return Buffer.from(resolved);
  return encoding === undefined ? resolved : Buffer.from(resolved).toString(encoding);
}"#;

const REALPATH_FACTORY: &str = quench_js_check::checked_js!(r#"(realpathSync) => {
  function realpath(path, options, callback) {
    if (typeof options === "function") callback = options;
    if (typeof callback !== "function") {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, realpathSync(path, options)]); }
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
    const settings = typeof options === "string" ? { encoding: options } : options || {};
    const fileHandleFd = settings.fd && typeof settings.fd === "object" &&
      typeof settings.fd.fd === "number" ? settings.fd.fd : null;
    const suppliedFd = typeof settings.fd === "number" || fileHandleFd !== null;
    const descriptor = typeof settings.fd === "number" ? settings.fd : fileHandleFd;
    if (typeof path !== "string" && typeof path !== "number" &&
        !Buffer.isBuffer(path) && !(path instanceof URL)) {
      if (!(path == null && suppliedFd)) {
      const error = new TypeError('The "path" argument must be of type string, Buffer, or URL.');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
      }
    }
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
    const autoClose = settings.autoClose !== false && fileHandleFd === null;
    let explicitClose = false;
    let phase = suppliedFd ? "open" : "idle";
    let fd = suppliedFd ? descriptor : null;
    let bytesRead = 0;
    const stream = this;

    const closeFileOnce = () => {
      if (!autoClose && !explicitClose) return false;
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
              stream.emit("ready");
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
    stream.fd = fd;
    stream.bytesRead = 0;
    stream.close = function close(callback) {
      explicitClose = true;
      closeFileOnce();
      if (callback) {
        if (stream.closed) setImmediate(callback);
        else stream.once("close", callback);
      }
      stream.destroy();
      return stream;
    };
    if (suppliedFd) setImmediate(() => {
      if (!stream.destroyed) stream.emit("ready");
    });
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
        : typeof value === 'function'
          ? ` Received function ${value.name}`
        : ` Received type ${typeof value} (${typeof value === 'string' ? `'${value}'` : String(value)})`;
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

const CP_API: &str = r#"(existsSync, statSync, lstatSync, readdirSync, mkdirSync, unlinkSync, copyFileSync, symlinkSync, readlinkSync, realpathSync, utimesSync) => {
  const exists = (path) => {
    try { statSync(path); return true; } catch { return false; }
  };
  const fail = (code, message, path) => {
    const error = new Error(message);
    error.code = code;
    if (path !== undefined) error.path = path;
    throw error;
  };
  const normalizePath = (path) => path instanceof URL ? decodeURIComponent(path.pathname) : Buffer.isBuffer(path) ? path.toString() : path;
  const validateOptions = (options) => {
    if (options == null || typeof options !== 'object') {
      const error = new TypeError('The "options" argument must be of type object.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (options.mode !== undefined && (!Number.isInteger(options.mode) || options.mode < 0 || options.mode > 0o777)) {
      const error = new RangeError('The value of "mode" is out of range.');
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    for (const name of ['recursive', 'force', 'errorOnExist', 'dereference', 'preserveTimestamps', 'verbatimSymlinks']) {
      if (Object.prototype.hasOwnProperty.call(options, name) && typeof options[name] !== 'boolean') {
        const error = new TypeError(`The "options.${name}" property must be of type boolean.`);
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
    }
    if (options.filter !== undefined && typeof options.filter !== 'function') {
      const error = new TypeError('The "options.filter" property must be of type function.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
  };
  const samePath = (left, right) => String(left).replace(/[\\/]$/, '') === String(right).replace(/[\\/]$/, '');
  const isWithin = (parent, child) => {
    const prefix = `${String(parent).replace(/[\\/]$/, '')}/`;
    return String(child).startsWith(prefix);
  };
  const resolvedPath = (path) => {
    try { return realpathSync(path); } catch {
      const value = String(path);
      const separator = Math.max(value.lastIndexOf('/'), value.lastIndexOf('\\'));
      if (separator < 0) return path;
      const parent = value.slice(0, separator) || value.slice(0, 1);
      try { return `${realpathSync(parent)}${value.slice(separator)}`; } catch { return path; }
    }
  };
  const invalidCopyTarget = (source, destination, dereference) => {
    if (!dereference && lstatSync(source).isSymbolicLink()) {
      if (!exists(destination) && !lstatExists(destination)) return undefined;
      if (!statSync(source).isDirectory()) return undefined;
      const resolvedSource = resolvedPath(source);
      const resolvedDestination = resolvedPath(destination);
      if (samePath(resolvedSource, resolvedDestination)) return 'ERR_FS_CP_EINVAL';
      if (isWithin(resolvedDestination, resolvedSource)) return 'ERR_FS_CP_SYMLINK_TO_SUBDIRECTORY';
      return undefined;
    }
    const resolvedSource = resolvedPath(source);
    const resolvedDestination = resolvedPath(destination);
    return samePath(resolvedSource, resolvedDestination) || isWithin(resolvedSource, resolvedDestination)
      ? 'ERR_FS_CP_EINVAL'
      : undefined;
  };
  function cpSync(source, destination, options = {}) {
    source = normalizePath(source);
    destination = normalizePath(destination);
    validateOptions(options);
    if (typeof options.filter === 'function') {
      const accepted = options.filter(source, destination);
      if (accepted && typeof accepted.then === 'function') {
        const error = new TypeError('The "filter" function must return a boolean.');
        error.code = 'ERR_INVALID_RETURN_VALUE';
        throw error;
      }
      if (accepted === false) return undefined;
    }
    if (options.dereference === true && options.verbatimSymlinks === true) {
      fail('ERR_INCOMPATIBLE_OPTION_PAIR', "Option 'dereference' and 'verbatimSymlinks' cannot be used together");
    }
    const invalidTarget = invalidCopyTarget(source, destination, options.dereference === true);
    if (invalidTarget) fail(invalidTarget, `Cannot copy '${source}' to a subdirectory of self '${destination}'`, destination);
    const sourceLstat = lstatSync(source);
    if (sourceLstat.isSymbolicLink() && options.dereference !== true) {
      let dest = destination;
      if (exists(destination) && !lstatSync(destination).isSymbolicLink() && statSync(destination).isDirectory()) {
        const sourceParts = String(source).split(/[\\/]/);
        dest = `${destination.replace(/[\\/]$/, '')}/${sourceParts[sourceParts.length - 1]}`;
      }
      if (exists(dest) || lstatExists(dest)) {
        if (!lstatSync(dest).isSymbolicLink()) fail('EEXIST', `EEXIST: file already exists, symlink '${dest}'`, dest);
        if (options.errorOnExist && options.force === false) {
          fail('ERR_FS_CP_EEXIST', `Target already exists: ${dest}`, dest);
        }
        if (options.force === false) return undefined;
        unlinkSync(dest);
      }
      let target = readlinkSync(source);
      if (options.verbatimSymlinks !== true && !/^(?:[A-Za-z]:[\\/]|[\\/]{1,2})/.test(target)) {
        target = realpathSync(source);
      }
      ensureParent(dest);
      symlinkSync(target, dest);
      return undefined;
    }
    const sourceStats = statSync(source);
    if (sourceStats.isDirectory()) {
      if (exists(destination) && !statSync(destination).isDirectory()) {
        fail('ERR_FS_CP_DIR_TO_NON_DIR', `Cannot overwrite non-directory '${destination}' with directory '${source}'`);
      }
      if (exists(destination) && options.errorOnExist === true && options.force === false) {
        fail('ERR_FS_CP_EEXIST', `Target already exists: ${destination}`, destination);
      }
      if (options.recursive !== true) {
        fail('ERR_FS_EISDIR', `Recursive option not enabled, cannot copy a directory: ${source}`);
      }
      mkdirSync(destination, { recursive: true });
      for (const name of readdirSync(source)) {
        const src = `${source.replace(/[\\/]$/, '')}/${name}`;
        const dest = `${destination.replace(/[\\/]$/, '')}/${name}`;
        cpSync(src, dest, options);
      }
      if (options.preserveTimestamps) {
        utimesSync(destination, sourceStats.atime.getTime() / 1000, sourceStats.mtime.getTime() / 1000);
      }
      return undefined;
    }
    let dest = destination;
    if (exists(destination) && lstatSync(destination).isSymbolicLink() && options.dereference !== true) {
      unlinkSync(destination);
    }
    if (exists(destination) && statSync(destination).isDirectory()) {
      fail('ERR_FS_CP_NON_DIR_TO_DIR', `Cannot overwrite directory with non-directory: ${destination}`);
    }
    if (exists(dest)) {
      if (options.errorOnExist && options.force === false) {
        fail('ERR_FS_CP_EEXIST', `Target already exists: ${dest}`, dest);
      }
      if (options.force === false) return undefined;
    }
    if ((options.mode & 4) !== 0) {
      const error = new Error(`ENOTSUP: operation not supported, copyfile '${source}' -> '${dest}'`);
      error.errno = -95;
      error.code = 'ENOTSUP';
      error.syscall = 'copyfile';
      error.path = source;
      error.dest = dest;
      throw error;
    }
    ensureParent(dest);
    copyFileSync(source, dest, options.mode || 0);
    if (options.preserveTimestamps) {
      utimesSync(dest, sourceStats.atime.getTime() / 1000, sourceStats.mtime.getTime() / 1000);
    }
    return undefined;
  }
  function lstatExists(path) {
    try { lstatSync(path); return true; } catch { return false; }
  }
  function ensureParent(path) {
    const parent = String(path).replace(/[\\/][^\\/]*$/, '');
    if (parent && !exists(parent)) mkdirSync(parent, { recursive: true });
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
    source = normalizePath(source);
    destination = normalizePath(destination);
    validateOptions(options === undefined ? {} : options);
    if (options && options.dereference === true && options.verbatimSymlinks === true) {
      const error = new Error("Option 'dereference' and 'verbatimSymlinks' cannot be used together");
      error.code = 'ERR_INCOMPATIBLE_OPTION_PAIR';
      throw error;
    }
    const copy = async (src, dest, opts) => {
      validateOptions(opts);
      const invalidTarget = invalidCopyTarget(src, dest, opts.dereference === true);
      if (invalidTarget) fail(invalidTarget, `Cannot copy '${src}' to a subdirectory of self '${dest}'`, dest);
      if (typeof opts.filter === 'function' && await opts.filter(src, dest) === false) return;
      const stats = opts.dereference === true ? statSync(src) : lstatSync(src);
      if (stats.isSymbolicLink() && opts.dereference !== true) {
        cpSync(src, dest, { ...opts, filter: undefined });
        return;
      }
      if (stats.isDirectory()) {
        if (exists(dest) && !statSync(dest).isDirectory()) fail('ERR_FS_CP_DIR_TO_NON_DIR', `Cannot overwrite non-directory '${dest}' with directory '${src}'`);
        if (exists(dest) && opts.errorOnExist === true && opts.force === false) fail('ERR_FS_CP_EEXIST', `Target already exists: ${dest}`, dest);
        if (opts.recursive !== true) fail('ERR_FS_EISDIR', `Recursive option not enabled, cannot copy a directory: ${src}`);
        mkdirSync(dest, { recursive: true });
        for (const name of readdirSync(src)) {
          await copy(`${String(src).replace(/[\\/]$/, '')}/${name}`, `${String(dest).replace(/[\\/]$/, '')}/${name}`, opts);
        }
        if (opts.preserveTimestamps) {
          utimesSync(dest, stats.atime.getTime() / 1000, stats.mtime.getTime() / 1000);
        }
        return;
      }
      cpSync(src, dest, { ...opts, filter: undefined });
    };
    queueMicrotask(async () => {
      let error;
      try { await copy(source, destination, options || {}); }
      catch (caught) { error = caught; }
      Reflect.apply(callback, undefined, [error || null]);
    });
  }
  const cpPromise = (source, destination, options) => new Promise((resolve, reject) => {
    if (options === undefined) options = {};
    validateOptions(options);
    if (options.dereference === true && options.verbatimSymlinks === true) {
      const error = new Error("Option 'dereference' and 'verbatimSymlinks' cannot be used together");
      error.code = 'ERR_INCOMPATIBLE_OPTION_PAIR';
      throw error;
    }
    cp(source, destination, options, (error) => error ? reject(error) : resolve());
  });
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

const ASYNC_LINK_API: &str = r#"(linkSync) => {
  const validatePath = (path, name) => {
    if (typeof path === 'string' || Buffer.isBuffer(path) || path instanceof URL) return;
    const received = path === null || path === undefined
      ? ` Received ${path}`
      : typeof path === 'object'
        ? ` Received an instance of ${Array.isArray(path) ? 'Array' : 'Object'}`
        : ` Received type ${typeof path} (${String(path)})`;
    const error = new TypeError(`The "${name}" argument must be of type string, Buffer, or URL.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  };
  return function link(existingPath, newPath, callback) {
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    validatePath(existingPath, 'existingPath');
    validatePath(newPath, 'newPath');
    queueMicrotask(() => {
      try { linkSync(existingPath, newPath); Reflect.apply(callback, undefined, [null]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const ASYNC_RENAME_API: &str = r#"(renameSync) => {
  const validatePath = (path, name) => {
    if (typeof path === 'string' || Buffer.isBuffer(path) || path instanceof URL) return;
    const received = path === null || path === undefined
      ? ` Received ${path}`
      : typeof path === 'object'
        ? ` Received an instance of ${Array.isArray(path) ? 'Array' : 'Object'}`
        : ` Received type ${typeof path} (${String(path)})`;
    const error = new TypeError(`The "${name}" argument must be of type string or an instance of Buffer or URL.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  };
  return function rename(oldPath, newPath, callback) {
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    validatePath(oldPath, 'oldPath');
    validatePath(newPath, 'newPath');
    queueMicrotask(() => {
      try { renameSync(oldPath, newPath); Reflect.apply(callback, undefined, [null]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const ASYNC_UNLINK_API: &str = r#"(unlinkSync) => {
  return function unlink(path, callback) {
    if (typeof callback !== 'function') {
      const error = new TypeError('The "cb" argument must be of type function');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
      const error = new TypeError('The "path" argument must be of type string, Buffer, or URL.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    queueMicrotask(() => {
      try { unlinkSync(path); Reflect.apply(callback, undefined, [null]); }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

const ASYNC_OWNER_API: &str = r#"(ownerSync, descriptor) => {
  const validateOwner = (name, value) => {
    if (typeof value !== 'number') {
      const error = new TypeError(`The "${name}" argument must be of type number.`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(value) || !Number.isFinite(value) || value < -1 || value > 0xFFFFFFFF) {
      const range = Number.isInteger(value) ? '>= -1 && <= 4294967295' : 'an integer';
      const error = new RangeError(`The value of "${name}" is out of range. It must be ${range}. Received ${String(value)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
  };
  const validateDescriptor = (fd) => {
    if (typeof fd !== 'number') {
      const error = new TypeError('The "fd" argument must be of type number');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (!Number.isInteger(fd)) {
      const error = new RangeError(`The value of "fd" is out of range. It must be an integer. Received ${String(fd)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
    if (fd < 0 || fd > 0x7FFFFFFF) {
      const error = new RangeError(`The value of "fd" is out of range. It must be >= 0 && <= 2147483647. Received ${String(fd)}`);
      error.code = 'ERR_OUT_OF_RANGE';
      throw error;
    }
  };
  return function owner(path, uid, gid, callback) {
    if (descriptor) {
      validateDescriptor(path);
      validateOwner('uid', uid);
      validateOwner('gid', gid);
      if (typeof callback !== 'function') {
        const error = new TypeError('The "cb" argument must be of type function');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
    } else {
      if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
        const error = new TypeError(`The "path" argument must be of type string or an instance of Buffer or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      validateOwner('uid', uid);
      validateOwner('gid', gid);
      if (typeof callback !== 'function') {
        const error = new TypeError('The "cb" argument must be of type function');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
    }
    queueMicrotask(() => {
      try { ownerSync(path, uid, gid); Reflect.apply(callback, undefined, [null]); }
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
    if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
      const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (Buffer.isBuffer(path)) path = path.toString();
    else if (path instanceof URL) path = decodeURIComponent(path.pathname);
    queueMicrotask(() => {
      try { Reflect.apply(callback, undefined, [null, lstatSync(path, options)]); }
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
    if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
      const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (Buffer.isBuffer(path)) path = path.toString();
    else if (path instanceof URL) path = decodeURIComponent(path.pathname);
    queueMicrotask(() => {
      try {
        const stats = statSync(path, options);
        Reflect.apply(callback, undefined, [null, stats]);
      }
      catch (error) { Reflect.apply(callback, undefined, [error]); }
    });
  };
}"#;

pub(crate) const BIGINT_STATS_API: &str = r#"(stats, options) => {
  if (stats === undefined || options?.bigint !== true) return stats;
  for (const name of ['dev', 'ino', 'mode', 'nlink', 'uid', 'gid', 'rdev', 'size', 'blksize', 'blocks', 'atimeMs', 'mtimeMs', 'ctimeMs', 'birthtimeMs']) {
    stats[name] = BigInt(Math.trunc(stats[name]));
  }
  for (const name of ['atime', 'mtime', 'ctime', 'birthtime']) {
    stats[`${name}Ns`] = BigInt(Math.trunc(Number(stats[`${name}Ms`]) * 1000000));
  }
  return stats;
}"#;

const STAT_SYNC_API: &str = r#"(hostStatSync, decorateStats) => function wrappedStatSync(path, options) {
  if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = decodeURIComponent(path.pathname);
  try { return decorateStats(hostStatSync(path), options); }
  catch (error) {
    if (options?.throwIfNoEntry === false && error?.code === 'ENOENT') return undefined;
    throw error;
  }
}"#;

const STATFS_SYNC_API: &str = r#"(hostStatfsSync) => function statfsSync(path, options) {
  if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = decodeURIComponent(path.pathname);
  const stats = hostStatfsSync(path);
  if (options?.bigint === true) {
    for (const name of ['type', 'bsize', 'frsize', 'blocks', 'bfree', 'bavail', 'files', 'ffree']) {
      stats[name] = BigInt(stats[name]);
    }
  }
  return stats;
}"#;

const ASYNC_STATFS_API: &str = r#"(statfsSync) => function statfs(path, options, callback) {
  if (typeof options === 'function') {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== 'function') {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = decodeURIComponent(path.pathname);
  queueMicrotask(() => {
    try { Reflect.apply(callback, undefined, [null, statfsSync(path, options)]); }
    catch (error) { Reflect.apply(callback, undefined, [error]); }
  });
}"#;

const ASYNC_TRUNCATE_API: &str = r#"(truncateSync) => function truncate(path, length, callback) {
  if (typeof length === 'function') {
    callback = length;
    length = undefined;
  }
  if (length !== undefined && typeof length !== 'number') {
    const received = length === null ? ' Received null' : typeof length === 'object'
      ? ` Received an instance of ${Array.isArray(length) ? 'Array' : 'Object'}`
      : ` Received type ${typeof length} (${typeof length === 'string' ? `'${length}'` : String(length)})`;
    const error = new TypeError(`The "len" argument must be of type number.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (typeof length === 'number' && !Number.isSafeInteger(length)) {
    const error = new RangeError(`The value of "len" is out of range. It must be an integer. Received ${String(length)}`);
    error.code = 'ERR_OUT_OF_RANGE';
    throw error;
  }
  if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (typeof callback !== 'function') {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  queueMicrotask(() => {
    try { truncateSync(path, length); Reflect.apply(callback, undefined, [null]); }
    catch (error) { Reflect.apply(callback, undefined, [error]); }
  });
}"#;

const ASYNC_FTRUNCATE_API: &str = r#"(ftruncateSync) => function ftruncate(fd, length, callback) {
  if (typeof length === 'function') {
    callback = length;
    length = undefined;
  }
  if (typeof fd !== 'number') {
    const received = fd === null || fd === undefined ? ` Received ${fd}` : typeof fd === 'object'
      ? ` Received an instance of ${Array.isArray(fd) ? 'Array' : 'Object'}`
      : ` Received type ${typeof fd} (${typeof fd === 'string' ? `'${fd}'` : String(fd)})`;
    const error = new TypeError(`The "fd" argument must be of type number.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (!Number.isInteger(fd)) {
    const error = new RangeError(`The value of "fd" is out of range. It must be an integer. Received ${String(fd)}`);
    error.code = 'ERR_OUT_OF_RANGE';
    throw error;
  }
  if (fd < 0 || fd > 0x7FFFFFFF) {
    const error = new RangeError(`The value of "fd" is out of range. It must be >= 0 && <= 2147483647. Received ${String(fd)}`);
    error.code = 'ERR_OUT_OF_RANGE';
    throw error;
  }
  if (length !== undefined && typeof length !== 'number') {
    const received = length === null ? ' Received null' : typeof length === 'object'
      ? ` Received an instance of ${Array.isArray(length) ? 'Array' : 'Object'}`
      : ` Received type ${typeof length} (${typeof length === 'string' ? `'${length}'` : String(length)})`;
    const error = new TypeError(`The "len" argument must be of type number.${received}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (typeof length === 'number' && !Number.isSafeInteger(length)) {
    const error = new RangeError(`The value of "len" is out of range. It must be an integer. Received ${String(length)}`);
    error.code = 'ERR_OUT_OF_RANGE';
    throw error;
  }
  if (typeof callback !== 'function') {
    const error = new TypeError('The "cb" argument must be of type function');
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  queueMicrotask(() => {
    try { ftruncateSync(fd, length); Reflect.apply(callback, undefined, [null]); }
    catch (error) { Reflect.apply(callback, undefined, [error]); }
  });
}"#;

const READLINK_SYNC_API: &str = r#"(hostReadlinkSync) => function readlinkSync(path, options) {
  if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
    const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
    error.code = 'ERR_INVALID_ARG_TYPE';
    throw error;
  }
  if (Buffer.isBuffer(path)) path = path.toString();
  else if (path instanceof URL) path = decodeURIComponent(path.pathname);
  return hostReadlinkSync(path);
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
    if (typeof path !== 'string' && !Buffer.isBuffer(path) && !(path instanceof URL)) {
      const error = new TypeError(`The "path" argument must be of type string, Buffer, or URL. Received ${path === null || path === undefined ? path : typeof path === 'object' ? `an instance of ${Array.isArray(path) ? 'Array' : 'Object'}` : `type ${typeof path} (${String(path)})`}`);
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (Buffer.isBuffer(path)) path = path.toString();
    else if (path instanceof URL) path = decodeURIComponent(path.pathname);
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
    let opendir_promise;
    let read_file = context.host_function(crate::host::shared_vm::operation("fsReadFileSync"))?;
    set(context, module, "readFileSync", read_file)?;
    let read_file_factory = context.evaluate_script_rooted(
        ASYNC_READ_FILE_FACTORY,
        "node:fs/shared-async-read-file.js",
    )?;
    let undefined = context.undefined();
    let read_file_async = context.call_rooted(read_file_factory, undefined, &[read_file])?;
    set(context, module, "readFile", read_file_async)?;
    let stat_host = context.host_function(crate::host::shared_vm::operation("fsStatSync"))?;
    let decorate_bigint_stats =
        context.evaluate_script_rooted(BIGINT_STATS_API, "node:fs/shared-bigint-stats.js")?;
    let stat_factory = context.evaluate_script_rooted(STAT_SYNC_API, "node:fs/shared-stat-sync.js")?;
    let undefined = context.undefined();
    let stat_sync = context.call_rooted(
        stat_factory,
        undefined,
        &[stat_host, decorate_bigint_stats],
    )?;
    set(context, module, "statSync", stat_sync)?;
    let statfs_host = context.host_function(crate::host::shared_vm::operation("fsStatfsSync"))?;
    let statfs_factory =
        context.evaluate_script_rooted(STATFS_SYNC_API, "node:fs/shared-statfs-sync.js")?;
    let statfs_sync = context.call_rooted(statfs_factory, undefined, &[statfs_host])?;
    set(context, module, "statfsSync", statfs_sync)?;
    let statfs_factory =
        context.evaluate_script_rooted(ASYNC_STATFS_API, "node:fs/shared-async-statfs.js")?;
    let statfs = context.call_rooted(statfs_factory, undefined, &[statfs_sync])?;
    set(context, module, "statfs", statfs)?;
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
    let lstat_host = context.host_function(crate::host::shared_vm::operation("fsLstatSync"))?;
    let lstat_factory = context.evaluate_script_rooted(STAT_SYNC_API, "node:fs/shared-lstat-sync.js")?;
    let undefined = context.undefined();
    let lstat_sync = context.call_rooted(
        lstat_factory,
        undefined,
        &[lstat_host, decorate_bigint_stats],
    )?;
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
    let opendir_factory = context.evaluate_script_rooted(OPENDIR_API, "node:fs/shared-opendir.js")?;
    let opendir_api = context.call_rooted(opendir_factory, undefined, &[readdir_sync])?;
    let dir_constructor = get(context, opendir_api, "Dir")?;
    let dirent_constructor = get(context, opendir_api, "Dirent")?;
    let opendir = get(context, opendir_api, "opendir")?;
    let opendir_sync = get(context, opendir_api, "opendirSync")?;
    opendir_promise = get(context, opendir_api, "opendirPromise")?;
    set(context, module, "Dir", dir_constructor)?;
    set(context, module, "Dirent", dirent_constructor)?;
    set(context, module, "opendir", opendir)?;
    set(context, module, "opendirSync", opendir_sync)?;
    let readlink_host = context.host_function(crate::host::shared_vm::operation("fsReadlinkSync"))?;
    let readlink_factory =
        context.evaluate_script_rooted(READLINK_SYNC_API, "node:fs/shared-readlink-sync.js")?;
    let undefined = context.undefined();
    let readlink_sync = context.call_rooted(readlink_factory, undefined, &[readlink_host])?;
    set(context, module, "readlinkSync", readlink_sync)?;
    let readlink_factory = context.evaluate_script_rooted(
        ASYNC_READLINK_API,
        "node:fs/shared-async-readlink.js",
    )?;
    let readlink = context.call_rooted(readlink_factory, undefined, &[readlink_sync])?;
    set(context, module, "readlink", readlink)?;
    let realpath_host = context.host_function(crate::host::shared_vm::operation("fsRealpathSync"))?;
    let realpath_sync_factory =
        context.evaluate_script_rooted(REALPATH_SYNC_API, "node:fs/shared-realpath-sync.js")?;
    let undefined = context.undefined();
    let realpath_sync =
        context.call_rooted(realpath_sync_factory, undefined, &[realpath_host])?;
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
    let fsync_sync = get(context, module, "fsyncSync")?;
    let fdatasync_sync = get(context, module, "fdatasyncSync")?;
    let async_sync_factory = context.evaluate_script_rooted(
        ASYNC_SYNC_API,
        "node:fs/shared-async-sync.js",
    )?;
    let async_sync = context.call_rooted(
        async_sync_factory,
        undefined,
        &[fsync_sync, fdatasync_sync],
    )?;
    let fsync_async = get(context, async_sync, "fsync")?;
    let fdatasync_async = get(context, async_sync, "fdatasync")?;
    set(context, module, "fsync", fsync_async)?;
    set(context, module, "fdatasync", fdatasync_async)?;
    let write_sync = get(context, module, "writeSync")?;
    let writev_factory = context.evaluate_script_rooted(WRITEV_API, "node:fs/shared-writev.js")?;
    let writev_api = context.call_rooted(writev_factory, undefined, &[write_sync])?;
    let writev = get(context, writev_api, "writev")?;
    let writev_sync = get(context, writev_api, "writevSync")?;
    set(context, module, "writev", writev)?;
    set(context, module, "writevSync", writev_sync)?;
    let readv_read_sync = get(context, module, "readSync")?;
    let readv_factory = context.evaluate_script_rooted(READV_API, "node:fs/shared-readv.js")?;
    let readv_api = context.call_rooted(readv_factory, undefined, &[readv_read_sync])?;
    let readv = get(context, readv_api, "readv")?;
    let readv_sync = get(context, readv_api, "readvSync")?;
    set(context, module, "readv", readv)?;
    set(context, module, "readvSync", readv_sync)?;
    let truncate_sync = get(context, module, "truncateSync")?;
    let truncate_factory =
        context.evaluate_script_rooted(ASYNC_TRUNCATE_API, "node:fs/shared-async-truncate.js")?;
    let truncate = context.call_rooted(truncate_factory, undefined, &[truncate_sync])?;
    set(context, module, "truncate", truncate)?;
    let ftruncate_sync = get(context, module, "ftruncateSync")?;
    let ftruncate_factory = context
        .evaluate_script_rooted(ASYNC_FTRUNCATE_API, "node:fs/shared-async-ftruncate.js")?;
    let ftruncate = context.call_rooted(ftruncate_factory, undefined, &[ftruncate_sync])?;
    set(context, module, "ftruncate", ftruncate)?;
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
    let lstat_sync = get(context, module, "lstatSync")?;
    let unlink_sync = get(context, module, "unlinkSync")?;
    let symlink_sync_for_cp = get(context, module, "symlinkSync")?;
    let readlink_sync_for_cp = get(context, module, "readlinkSync")?;
    let realpath_sync_for_cp = get(context, module, "realpathSync")?;
    let utimes_sync_for_cp = get(context, module, "utimesSync")?;
    let cp_api = context.call_rooted(
        cp_factory,
        undefined,
        &[
            exists_sync,
            stat_sync,
            lstat_sync,
            readdir_sync,
            mkdir_sync,
            unlink_sync,
            copy_file_sync,
            symlink_sync_for_cp,
            readlink_sync_for_cp,
            realpath_sync_for_cp,
            utimes_sync_for_cp,
        ],
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
    let link_sync = get(context, module, "linkSync")?;
    let link_factory = context.evaluate_script_rooted(ASYNC_LINK_API, "node:fs/shared-async-link.js")?;
    let link = context.call_rooted(link_factory, undefined, &[link_sync])?;
    set(context, module, "link", link)?;
    let rename_sync = get(context, module, "renameSync")?;
    let rename_factory = context.evaluate_script_rooted(
        ASYNC_RENAME_API,
        "node:fs/shared-async-rename.js",
    )?;
    let rename = context.call_rooted(rename_factory, undefined, &[rename_sync])?;
    set(context, module, "rename", rename)?;
    let unlink_sync = get(context, module, "unlinkSync")?;
    let unlink_factory = context.evaluate_script_rooted(
        ASYNC_UNLINK_API,
        "node:fs/shared-async-unlink.js",
    )?;
    let unlink = context.call_rooted(unlink_factory, undefined, &[unlink_sync])?;
    set(context, module, "unlink", unlink)?;
    for (name, sync_name, descriptor) in [
        ("chown", "chownSync", false),
        ("lchown", "lchownSync", false),
        ("fchown", "fchownSync", true),
    ] {
        let owner_sync = get(context, module, sync_name)?;
        let owner_factory = context.evaluate_script_rooted(
            ASYNC_OWNER_API,
            "node:fs/shared-async-owner.js",
        )?;
        let descriptor_value = context.boolean(descriptor);
        let owner = context.call_rooted(
            owner_factory,
            undefined,
            &[owner_sync, descriptor_value],
        )?;
        set(context, module, name, owner)?;
    }
    let open_sync = get(context, module, "openSync")?;
    let close_sync = get(context, module, "closeSync")?;
    let fstat_sync = get(context, module, "fstatSync")?;
    let factory = context.evaluate_script_rooted(OPEN_CLOSE_FACTORY, "node:fs/shared-open-close.js")?;
    let undefined = context.undefined();
    let read_sync = get(context, module, "readSync")?;
    let open_close =
        context.call_rooted(factory, undefined, &[open_sync, close_sync, read_sync, fstat_sync])?;
    let open = get(context, open_close, "open")?;
    let close = get(context, open_close, "close")?;
    let read = get(context, open_close, "read")?;
    let fstat = get(context, open_close, "fstat")?;
    set(context, module, "open", open)?;
    set(context, module, "close", close)?;
    set(context, module, "read", read)?;
    set(context, module, "fstat", fstat)?;
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
    let streams = crate::host::shared_vm::commonjs::stream_module(context)?;
    let readable = get(context, streams, "Readable")?;
    let read_stream_factory = context
        .evaluate_script_rooted(CREATE_READ_STREAM, "node:fs/shared-create-read-stream.js")?;
    let read_stream_open = context.host_function(crate::host::shared_vm::operation("fsReadStreamOpen"))?;
    let read_stream_read = context.host_function(crate::host::shared_vm::operation("fsReadStreamRead"))?;
    let read_stream_close = context.host_function(crate::host::shared_vm::operation("fsReadStreamClose"))?;
    let read_stream_api = context.call_rooted(
        read_stream_factory,
        undefined,
        &[read_stream_open, read_stream_read, read_stream_close, readable],
    )?;
    let create_read_stream = get(context, read_stream_api, "createReadStream")?;
    let read_stream = get(context, read_stream_api, "ReadStream")?;
    set(context, module, "createReadStream", create_read_stream)?;
    set(context, module, "ReadStream", read_stream)?;
    let rm_sync = get(context, module, "rmSync")?;
    let promises = promises_module(
        context,
        constants,
        open_sync,
        close_sync,
        read_sync,
        write_sync,
        fstat_sync,
        fchmod_sync,
        fsync_sync,
        fdatasync_sync,
        write_file_sync,
        append_file_sync,
        mkdir_sync,
        rmdir_sync,
        rm_sync,
        mkdtemp_sync,
        copy_file_sync,
        readv_sync,
        writev_sync,
        create_read_stream,
    )?;
    set(context, promises, "opendir", opendir_promise)?;
    for name in ["stat", "lstat"] {
        let sync = get(context, module, &format!("{name}Sync"))?;
        let promise_factory = context.evaluate_script_rooted(
            "(sync) => (...args) => Promise.resolve().then(() => sync(...args))",
            "node:fs/promises/shared-stat.js",
        )?;
        let promise = context.call_rooted(promise_factory, undefined, &[sync])?;
        set(context, promises, name, promise)?;
    }
    let realpath_sync = get(context, module, "realpathSync")?;
    let realpath_promise_factory = context.evaluate_script_rooted(
        "(realpathSync) => (...args) => Promise.resolve().then(() => realpathSync(...args))",
        "node:fs/promises/shared-realpath.js",
    )?;
    let realpath_promise =
        context.call_rooted(realpath_promise_factory, undefined, &[realpath_sync])?;
    set(context, promises, "realpath", realpath_promise)?;
    let statfs_sync = get(context, module, "statfsSync")?;
    let statfs_promise_factory = context.evaluate_script_rooted(
        "(statfsSync) => (...args) => Promise.resolve().then(() => statfsSync(...args))",
        "node:fs/promises/shared-statfs.js",
    )?;
    let statfs_promise =
        context.call_rooted(statfs_promise_factory, undefined, &[statfs_sync])?;
    set(context, promises, "statfs", statfs_promise)?;
    let mkdtemp_disposable_factory = context.evaluate_script_rooted(
        "(mkdtempSync, rmSync) => async (prefix) => { const absolutePath = await mkdtempSync(prefix); const normalizedPrefix = Buffer.isBuffer(prefix) ? prefix.toString() : prefix instanceof URL ? decodeURIComponent(prefix.pathname) : prefix; const path = normalizedPrefix.startsWith('/') ? absolutePath : `${normalizedPrefix}${absolutePath.slice(-6)}`; const remove = () => Promise.resolve().then(() => rmSync(absolutePath, { recursive: true, force: true })); const disposable = { path, remove }; disposable[Symbol.asyncDispose] = remove; return disposable; }",
        "node:fs/promises/shared-mkdtemp-disposable.js",
    )?;
    let mkdtemp_sync = get(context, module, "mkdtempSync")?;
    let rm_sync = get(context, module, "rmSync")?;
    let mkdtemp_disposable = context.call_rooted(
        mkdtemp_disposable_factory,
        undefined,
        &[mkdtemp_sync, rm_sync],
    )?;
    set(context, promises, "mkdtempDisposable", mkdtemp_disposable)?;
    let truncate_promise_factory = context.evaluate_script_rooted(
        "(truncateSync) => (...args) => Promise.resolve().then(() => truncateSync(...args))",
        "node:fs/promises/shared-truncate.js",
    )?;
    let truncate_promise =
        context.call_rooted(truncate_promise_factory, undefined, &[truncate_sync])?;
    set(context, promises, "truncate", truncate_promise)?;
    set(context, promises, "cp", cp_promise)?;
    let symlink_promise_factory = context.evaluate_script_rooted(
        "(symlinkSync) => (...args) => Promise.resolve().then(() => symlinkSync(...args))",
        "node:fs/promises/shared-symlink.js",
    )?;
    let symlink_promise =
        context.call_rooted(symlink_promise_factory, undefined, &[symlink_sync])?;
    set(context, promises, "symlink", symlink_promise)?;
    let link_sync = get(context, module, "linkSync")?;
    let link_promise_factory = context.evaluate_script_rooted(
        "(linkSync) => (...args) => Promise.resolve().then(() => linkSync(...args))",
        "node:fs/promises/shared-link.js",
    )?;
    let link_promise =
        context.call_rooted(link_promise_factory, undefined, &[link_sync])?;
    set(context, promises, "link", link_promise)?;
    let rename_promise_factory = context.evaluate_script_rooted(
        "(renameSync) => (...args) => Promise.resolve().then(() => renameSync(...args))",
        "node:fs/promises/shared-rename.js",
    )?;
    let rename_promise =
        context.call_rooted(rename_promise_factory, undefined, &[rename_sync])?;
    set(context, promises, "rename", rename_promise)?;
    let unlink_promise_factory = context.evaluate_script_rooted(
        "(unlinkSync) => (...args) => Promise.resolve().then(() => unlinkSync(...args))",
        "node:fs/promises/shared-unlink.js",
    )?;
    let unlink_promise =
        context.call_rooted(unlink_promise_factory, undefined, &[unlink_sync])?;
    set(context, promises, "unlink", unlink_promise)?;
    for (name, sync_name) in [("chown", "chownSync"), ("lchown", "lchownSync")] {
        let owner_sync = get(context, module, sync_name)?;
        let owner_promise_factory = context.evaluate_script_rooted(
            "(ownerSync) => (...args) => Promise.resolve().then(() => ownerSync(...args))",
            "node:fs/promises/shared-owner.js",
        )?;
        let owner_promise =
            context.call_rooted(owner_promise_factory, undefined, &[owner_sync])?;
        set(context, promises, name, owner_promise)?;
    }
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
    let metadata_promise_factory = context.evaluate_script_rooted(
        "(name) => (...args) => Promise.resolve().then(() => { const sync = globalThis.__nodeFs[`${name}Sync`] || (name === 'lchmod' ? globalThis.__nodeFs.chmodSync : undefined); return sync(...args); })",
        "node:fs/promises/shared-metadata-mutations.js",
    )?;
    for name in ["lchmod", "utimes", "lutimes"] {
        let name_root = context.string_rooted(name);
        let promise = context.call_rooted(
            metadata_promise_factory,
            undefined,
            &[name_root],
        )?;
        set(context, promises, name, promise)?;
        context.release_root(name_root);
        context.release_root(promise);
    }
    set(context, module, "promises", promises)?;

    let streams = crate::host::shared_vm::commonjs::stream_module(context)?;
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
    write_sync: RootId,
    fstat_sync: RootId,
    fchmod_sync: RootId,
    fsync_sync: RootId,
    fdatasync_sync: RootId,
    write_file_sync: RootId,
    append_file_sync: RootId,
    mkdir_sync: RootId,
    rmdir_sync: RootId,
    rm_sync: RootId,
    mkdtemp_sync: RootId,
    copy_file_sync: RootId,
    readv_sync: RootId,
    writev_sync: RootId,
    create_read_stream: RootId,
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
            write_sync,
            fstat_sync,
            fchmod_sync,
            fsync_sync,
            fdatasync_sync,
            write_file_sync,
            append_file_sync,
            readv_sync,
            writev_sync,
            create_read_stream,
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
    let rm_factory = context.evaluate_script_rooted(
        "(rmSync) => (...args) => Promise.resolve().then(() => rmSync(...args))",
        "node:fs/promises/shared-rm.js",
    )?;
    let rm = context.call_rooted(rm_factory, undefined, &[rm_sync])?;
    set(context, promises, "rm", rm)?;
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
