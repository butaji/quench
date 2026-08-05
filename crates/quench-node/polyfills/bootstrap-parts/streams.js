const __nodeFsReadStreamOptions = (options) => {
  const start = options.start === undefined ? 0 : Number(options.start);
  const end = options.end === undefined ? undefined : Number(options.end);
  if (!Number.isInteger(start) || start < 0) {
    const error = new RangeError('The "start" option is out of range');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  if (end !== undefined && (!Number.isInteger(end) || end < 0 || start > end)) {
    const error = new RangeError('The "end" option is out of range');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return { start, end };
};
const __nodeFsReadStreamClose = (stream) => {
  if (stream.fd !== null) {
    try {
      globalThis.__nodeFs.closeSync(stream.fd);
    } catch (error) {
      if (error.code !== "EBADF") throw error;
    }
    stream.fd = null;
  }
  stream.emit("close");
};
globalThis.__nodeFs.createReadStream = (value, options = {}) => {
  const { start, end } = __nodeFsReadStreamOptions(options);
  const stream = new NodeReadable(options);
  const path = nodePathValue(value);
  stream.path = path;
  stream.fd = null;
  stream.bytesRead = 0;
  if (options.encoding !== undefined) stream.setEncoding(options.encoding);
  queueMicrotask(() => {
    try {
      stream.fd = globalThis.__nodeFs.openSync(path, "r");
      stream.emit("open", stream.fd);
      const bytes = globalThis.__nodeFs.readFileSync(path);
      const end = options.end === undefined ? bytes.length : options.end + 1;
      const chunk = bytes.subarray(start, Math.min(end, bytes.length));
      if (chunk.length) {
        stream.push(chunk);
        stream.bytesRead += chunk.byteLength;
      }
      stream.push(null);
      if (options.autoClose !== false)
        stream.once("end", () => __nodeFsReadStreamClose(stream));
    } catch (error) {
      stream.emit("error", error);
    }
  });
  return stream;
};
class NodeAbortSignal {
  constructor() {
    this.aborted = false;
    this._listeners = [];
  }
  addEventListener(event, listener) {
    if (event === "abort") this._listeners.push(listener);
  }
  removeEventListener(event, listener) {
    this._listeners = this._listeners.filter((item) => item !== listener);
  }
  static abort() {
    const signal = new NodeAbortSignal();
    signal.aborted = true;
    return signal;
  }
}
class NodeAbortController {
  constructor() {
    this.signal = new NodeAbortSignal();
  }
  abort() {
    this.signal.aborted = true;
    this.signal._listeners.slice().forEach((listener) => listener());
  }
}
globalThis.AbortSignal = NodeAbortSignal;
globalThis.AbortController = NodeAbortController;
globalThis.__nodeFs.open = (value, flags, mode, callback) => {
  if (typeof flags === "function") {
    callback = flags;
    flags = "r";
    mode = undefined;
  } else if (typeof mode === "function") {
    callback = mode;
    mode = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  __nodeFsValidateMode(mode);
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    let fd;
    try {
      fd = globalThis.__nodeFs.openSync(path, flags, mode);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, fd);
  });
};
globalThis.__nodeFs.readdir = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    let result;
    try {
      result = globalThis.__nodeFs.readdirSync(path, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, result);
  });
};
globalThis.__nodeFs.mkdir = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (
    options &&
    Object.prototype.hasOwnProperty.call(options, "recursive") &&
    typeof options.recursive !== "boolean"
  ) {
    const error = new TypeError(
      `The "options.recursive" property must be of type boolean.${__nodeInvalidArgSuffix(options.recursive)}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      const created = globalThis.__nodeFs.mkdirSync(path, options);
      callback(null, created);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.readFile = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof value === "function") __nodeFsReadPath(value);
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    options &&
    options.signal !== undefined &&
    !(options.signal instanceof NodeAbortSignal)
  ) {
    const error = new TypeError('The "signal" option must be an AbortSignal');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    if (options && options.signal && options.signal.aborted) {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      callback(error);
      return;
    }
    let data;
    try {
      data = globalThis.__nodeFs.readFileSync(value, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, data);
  });
};
globalThis.__nodeFs.watch = (value) => {
  __nodeFsReadPath(typeof value === "number" ? false : value);
  const watcher = new globalThis.__nodeEventEmitter();
  watcher.close = () => watcher;
  return watcher;
};
globalThis.__nodeFs.watchFile = () => undefined;
globalThis.__nodeFs.unwatchFile = () => undefined;
const __nodeFsValidateMkdtemp = (prefix, options, callback) => {
  if (
    options !== undefined &&
    typeof options !== "function" &&
    typeof options !== "string" &&
    (typeof options !== "object" || options === null)
  )
    throw new TypeError('The "options" argument must be a string or an object');
  if (
    typeof prefix !== "string" &&
    !(prefix instanceof Uint8Array) &&
    !(prefix instanceof globalThis.__nodeURL)
  )
    throw new TypeError(
      'The "prefix" argument must be of type string or an instance of Buffer or URL'
    );
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
};
globalThis.__nodeFs.mkdtemp = (prefix, options, callback) => {
  if (typeof options === "function") callback = options;
  __nodeFsValidateMkdtemp(prefix, options, callback);
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.mkdtempSync(prefix));
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.writeFile = (value, data, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (options && options.signal && options.signal.aborted) {
    queueMicrotask(() => {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      callback(error);
    });
    return;
  }
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.writeFileSync(value, data);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
const __nodeFsPromisedReadOptions = (buffer, offset, length, position) => {
  if (!offset || typeof offset !== "object")
    return { target: buffer, start: offset, size: length, at: position };
  const options = offset;
  const target = options.buffer || NodeBuffer.alloc(16384);
  const start = options.offset == null ? 0 : options.offset;
  const size =
    options.length === undefined ? target.length - start : options.length;
  const at = options.position;
  return { target, start, size, at };
};
const __nodeFsPromisedRead = (fd, buffer, offset, length, position) => {
  const { target, start, size, at } = __nodeFsPromisedReadOptions(
    buffer,
    offset,
    length,
    position
  );
  if (target.length === 0 && Number(size) > 0) {
    const error = new TypeError("The buffer is empty");
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  const bytesRead = globalThis.__nodeFs.readSync(
    fd,
    target,
    start || 0,
    size === undefined ? target.length : size,
    at === undefined ? null : at
  );
  return { bytesRead, buffer: target };
};
globalThis.__nodeFs.promises = {
  open: (value, flags = "r", mode) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.open(value, flags, mode, (error, fd) =>
        error
          ? reject(error)
          : resolve({
              fd,
              close: () => Promise.resolve(),
              read: (buffer, offset, length, position) =>
                Promise.resolve().then(() =>
                  __nodeFsPromisedRead(fd, buffer, offset, length, position)
                )
            })
      )
    ),
  readFile: (value, options) =>
    value && typeof value === "object" && typeof value.fd === "number"
      ? value.readFile(options)
      : new Promise((resolve, reject) =>
          globalThis.__nodeFs.readFile(value, options, (error, data) =>
            error ? reject(error) : resolve(data)
          )
        ),
  writeFile: (value, data, options) =>
    new Promise((resolve, reject) =>
      queueMicrotask(() =>
        globalThis.__nodeFs.writeFile(value, data, options, (error) =>
          error ? reject(error) : resolve()
        )
      )
    ),
  appendFile: (value, data, options) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.appendFile(value, data, options, (error) =>
        error ? reject(error) : resolve()
      )
    ),
  access: (value, mode) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.access(value, mode, (error) =>
        error ? reject(error) : resolve()
      )
    ),
  truncate: (value, length = 0) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.truncateSync(value, length)
    ),
  ftruncate: (fd, length = 0) =>
    Promise.resolve().then(() => globalThis.__nodeFs.ftruncateSync(fd, length)),
  fsync: (fd) =>
    Promise.resolve().then(() => globalThis.__nodeFs.fsyncSync(fd)),
  fdatasync: (fd) =>
    Promise.resolve().then(() => globalThis.__nodeFs.fdatasyncSync(fd)),
  rm: (value, options) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.rm(value, options, (error) =>
        error ? reject(error) : resolve()
      )
    ),
  opendir: (value, options) =>
    Promise.resolve().then(() => globalThis.__nodeFs.opendirSync(value)),
  symlink: (target, link, type) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.symlink(target, link, type, (error) =>
        error ? reject(error) : resolve()
      )
    ),
  readlink: (value, options) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.readlink(value, options, (error, result) =>
        error ? reject(error) : resolve(result)
      )
    ),
  realpath: (value, options) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.realpathSync(value, options)
    ),
  fstat: (fd) =>
    Promise.resolve().then(() => globalThis.__nodeFs.fstatSync(fd)),
  fchmod: (fd, mode) =>
    Promise.resolve().then(() => globalThis.__nodeFs.fchmodSync(fd, mode)),
  chmod: (value, mode) =>
    Promise.resolve().then(() => globalThis.__nodeFs.chmodSync(value, mode)),
  rename: (from, to) =>
    Promise.resolve().then(() =>
      globalThis.__quench_fs_rename(nodeFsPath(from), nodeFsPath(to))
    ),
  unlink: (value) =>
    Promise.resolve().then(() =>
      globalThis.__quench_fs_unlink(nodeFsPath(value))
    ),
  copyFile: (from, to, mode = 0) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.copyFileSync(from, to, mode)
    ),
  rmdir: (value, options) =>
    Promise.resolve().then(() => globalThis.__nodeFs.rmdirSync(value, options)),
  mkdtemp: (prefix) =>
    Promise.resolve().then(() => globalThis.__nodeFs.mkdtempSync(prefix)),
  readv: (fd, buffers, position) =>
    Promise.resolve().then(() => {
      const bytesRead = globalThis.__nodeFs.readvSync(fd, buffers, position);
      return { bytesRead, buffers };
    }),
  writev: (fd, buffers, position) =>
    Promise.resolve().then(() => {
      const bytesWritten = globalThis.__nodeFs.writevSync(
        fd,
        buffers,
        position
      );
      return { bytesWritten, buffers };
    }),
  mkdir: (value) =>
    Promise.resolve().then(() => globalThis.__nodeFs.mkdirSync(value)),
  readdir: (value, options) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.readdirSync(value, options)
    ),
  stat: (value) =>
    Promise.resolve().then(() => globalThis.__nodeFs.statSync(value)),
  lstat: (value) =>
    Promise.resolve().then(() => globalThis.__nodeFs.lstatSync(value)),
  link: (existing, link) =>
    Promise.resolve().then(() => globalThis.__nodeFs.linkSync(existing, link))
};
const __nodePromiseOpen = globalThis.__nodeFs.promises.open;
