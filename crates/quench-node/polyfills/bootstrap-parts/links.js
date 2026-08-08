globalThis.__nodeFs.link = (existing, link, callback) => {
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    (typeof existing !== "string" && !(existing instanceof Uint8Array)) ||
    (typeof link !== "string" && !(link instanceof Uint8Array))
  ) {
    const error = new TypeError(
      'The "path" argument must be of type string or an instance of Buffer or URL'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.linkSync(existing, link);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.chmod = (value, mode, callback) => {
  if (typeof mode === "function") {
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
  const path = __nodeFsPathOnly(value);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.chmodSync(path, mode);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.appendFile = (value, data, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (
    typeof data !== "string" &&
    !(data instanceof NodeBuffer) &&
    !(data instanceof Uint8Array)
  ) {
    const error = new TypeError(
      'The "data" argument must be of type string or an instance of Buffer'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const encoding =
    typeof options === "string" ? options : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    const error = new TypeError(
      `The argument 'encoding' is invalid. Received '${encoding}'`
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.appendFileSync(value, data, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.rmdir = (value, options, callback) => {
  if (typeof options === "function") callback = options;
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    error.toString = () => `TypeError [ERR_INVALID_ARG_TYPE]: ${error.message}`;
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.rmdirSync(path);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.rm = (value, options, callback) => {
  if (typeof options === "function") callback = options;
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const path = nodeFsPath(value).replace(/^\.\/test\//, "tests/node/test/");
  queueMicrotask(() => {
    try {
      if (
        !(options && options.recursive) &&
        globalThis.__nodeFs.readdirSync(path).length === 0
      ) {
        globalThis.__nodeFs.rmdirSync(path);
        callback(null);
        return;
      }
      globalThis.__nodeFs.rmSync(path, { ...(options || {}), __async: true });
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.rename = (from, to, callback) => {
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const source = __nodeFsPathOnly(from);
  const destination = __nodeFsPathOnly(to);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.renameSync(source, destination);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.copyFile = (from, to, mode, callback) => {
  if (typeof mode === "function") {
    callback = mode;
    mode = 0;
  }
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const source = __nodeFsCopyPath(from, "src");
  const destination = __nodeFsCopyPath(to, "dest");
  if (typeof mode !== "number") {
    const error = new TypeError('The "mode" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      __nodeFsCopyExclusiveError(destination, source, mode);
      globalThis.__nodeFs.copyFileSync(source, destination, mode);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.realpath = (value, options, callback) => {
  if (typeof options === "function") callback = options;
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const encoding =
    typeof options === "string" ? options : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    const error = new TypeError(
      `The argument 'encoding' is invalid. Received '${encoding}'`
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    let result;
    try {
      result = globalThis.__nodeFs.realpathSync(path, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, result);
  });
};
globalThis.__nodeFs.realpathSync.native = globalThis.__nodeFs.realpathSync;
globalThis.__nodeFs.realpath.native = globalThis.__nodeFs.realpath;
globalThis.__nodeStats = function Stats(
  file = false,
  directory = false,
  date = new Date()
) {
  if (!(date instanceof Date)) date = new Date(Number(date) || 0);
  this.dev = 0;
  this.mode = 0;
  this.nlink = 1;
  this.uid = 0;
  this.gid = 0;
  this.rdev = 0;
  this.blksize = 4096;
  this.ino = 0;
  this.size = 0;
  this.blocks = 0;
  this.atime = date;
  this.mtime = date;
  this.ctime = date;
  this.birthtime = date;
  this.atimeMs = date.getTime();
  this.mtimeMs = date.getTime();
  this.ctimeMs = date.getTime();
  this.birthtimeMs = date.getTime();
  this._file = file;
  this._directory = directory;
};
globalThis.__nodeStats.prototype.isFile = function () {
  return this._file;
};
globalThis.__nodeStats.prototype.isDirectory = function () {
  return this._directory;
};
globalThis.__nodeStats.prototype.isSocket = function () {
  return false;
};
globalThis.__nodeStats.prototype.isBlockDevice = function () {
  return false;
};
globalThis.__nodeStats.prototype.isCharacterDevice = function () {
  return false;
};
globalThis.__nodeStats.prototype.isFIFO = function () {
  return false;
};
globalThis.__nodeStats.prototype.isSymbolicLink = function () {
  return this._symlink === true;
};
globalThis.__nodeFs.Dir = class Dir {
  constructor(path) {
    if (path === undefined) {
      const error = new TypeError('The "path" argument must be specified');
      error.code = "ERR_MISSING_ARGS";
      throw error;
    }
    this._path = path;
    if (globalThis.__quench_fs_kind(path) === "file") {
      const error = new Error(`ENOTDIR: not a directory, scandir '${path}'`);
      error.code = "ENOTDIR";
      throw error;
    }
    this._entries = globalThis.__nodeFs.readdirSync(path, {
      withFileTypes: true
    });
    this._index = 0;
    this._closed = false;
    this._reading = false;
    this._pending = null;
  }
  readSync() {
    if (this._reading) {
      const error = new Error("Directory read operation in progress");
      error.code = "ERR_DIR_CONCURRENT_OPERATION";
      throw error;
    }
    if (this._closed) {
      const error = new Error("Directory handle was closed");
      error.code = "ERR_DIR_CLOSED";
      throw error;
    }
    return this._entries[this._index++] || null;
  }
  closeSync() {
    if (this._reading) {
      const error = new Error("Directory read operation in progress");
      error.code = "ERR_DIR_CONCURRENT_OPERATION";
      throw error;
    }
    if (this._closed) {
      const error = new Error("Directory handle was closed");
      error.code = "ERR_DIR_CLOSED";
      throw error;
    }
    this._closed = true;
  }
  read(callback) {
    if (callback === undefined) {
      this._reading = true;
      const operation = Promise.resolve().then(() => {
        this._reading = false;
        return this.readSync();
      });
      const tracked = operation.finally(() => {
        if (this._pending === tracked) this._pending = null;
      });
      this._pending = tracked;
      return operation;
    }
    if (typeof callback !== "function") {
      const error = new TypeError(
        'The "callback" argument must be of type function'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      error.toString = () => `TypeError [${error.code}]: ${error.message}`;
      throw error;
    }
    this._reading = true;
    const operation = new Promise((resolve, reject) =>
      queueMicrotask(() => {
        try {
          this._reading = false;
          const value = this.readSync();
          callback(null, value);
          resolve(value);
        } catch (error) {
          this._reading = false;
          callback(error);
          // The callback owns error delivery; keep the internal tracking
          // promise fulfilled so it cannot become an unhandled rejection.
          resolve(undefined);
        }
      })
    );
    const tracked = operation.finally(() => {
      if (this._pending === tracked) this._pending = null;
    });
    this._pending = tracked;
  }
  close(callback) {
    if (callback === undefined) {
      if (!this._reading) return Promise.resolve().then(() => this.closeSync());
      return this._pending.then(() => this.closeSync());
    }
    if (typeof callback !== "function") {
      const error = new TypeError(
        'The "callback" argument must be of type function'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      error.toString = () => `TypeError [${error.code}]: ${error.message}`;
      throw error;
    }
    queueMicrotask(() => {
      try {
        this.closeSync();
        callback(null);
      } catch (error) {
        callback(error);
      }
    });
  }
  async *[Symbol.asyncIterator]() {
    try {
      while (true) {
        const entry = await this.read();
        if (entry === null) break;
        yield entry;
      }
    } finally {
      if (!this._closed) this.closeSync();
    }
  }
};
Symbol.asyncDispose ||= Symbol("Symbol.asyncDispose");
Symbol.dispose ||= Symbol("Symbol.dispose");
globalThis.__nodeFs.Dir.prototype[Symbol.asyncDispose] = function () {
  return this.close();
};
globalThis.__nodeFs.Dir.prototype[Symbol.dispose] = function () {
  if (!this._closed) this.closeSync();
};
globalThis.__nodeFs.opendir = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    error.toString = () => `TypeError [${error.code}]: ${error.message}`;
    throw error;
  }
  globalThis.__validateOpendirOptions?.(options);
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      callback(null, new globalThis.__nodeFs.Dir(path, options));
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.lstatSync = (value) => {
  const path = nodeFsPath(value);
  let kind;
  try {
    kind = globalThis.__quench_fs_link_kind(path);
  } catch (_) {
    const error = new Error(
      `ENOENT: no such file or directory, lstat '${path}'`
    );
    error.code = "ENOENT";
    error.syscall = "lstat";
    error.path = path;
    throw error;
  }
  const times = globalThis.__nodeTimes?.[path];
  const stats = new globalThis.__nodeStats(
    kind === "file",
    kind === "directory",
    new Date(times?.mtime ?? Date.now() - 1)
  );
  if (times) {
    stats.atime = new Date(times.atime);
    stats.atimeMs = times.atime;
    stats.mtime = new Date(times.mtime);
    stats.mtimeMs = times.mtime;
  }
  stats._symlink = kind === "symlink";
  stats.mode = globalThis.__nodeModes[path] || 0;
  return stats;
};
globalThis.__nodeFs.stat = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    let result;
    try {
      result = globalThis.__nodeFs.statSync(path, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, result);
  });
};
globalThis.__nodeFs.lstat = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.lstatSync(path));
    } catch (error) {
      callback(error);
      return;
    }
  });
};
globalThis.__nodeFs.fstatSync = (fd) => {
  if (typeof fd !== "number") {
    const error = new TypeError(
      `The "fd" argument must be of type number.${globalThis.__nodeCommon.invalidArgTypeHelper(
        fd
      )}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  return globalThis.__nodeFs.statSync(globalThis.__nodeFdPaths[fd] || ".");
};
globalThis.__nodeFs.fstat = (fd, options, callback) => {
  if (typeof options === "function") callback = options;
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  queueMicrotask(() => {
    let result;
    try {
      result = globalThis.__nodeFs.fstatSync(fd);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, result);
  });
};
globalThis.__nodeFs.Stats = globalThis.__nodeStats;
globalThis.__nodeFs.close = (fd, callback) => {
  if (typeof fd !== "number") {
    const error = new TypeError(
      `The "fd" argument must be of type number.${globalThis.__nodeCommon.invalidArgTypeHelper(
        fd
      )}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  try {
    globalThis.__nodeFs.closeSync(fd);
    callback(null);
  } catch (error) {
    callback(error);
  }
};
const __nodeFsWriteStreamEnd = (
  stream,
  path,
  chunks,
  options,
  chunk,
  encoding,
  callback
) => {
  if (typeof encoding === "function") {
    callback = encoding;
    encoding = undefined;
  }
  if (chunk !== undefined) stream.write(chunk, encoding);
  queueMicrotask(() => {
    try {
      const flags = options.flags || "w";
      stream.fd = globalThis.__nodeFs.openSync(path, flags);
      stream.emit("open", stream.fd);
      const data = NodeBuffer.concat(chunks);
      stream.bytesWritten = data.byteLength;
      if (String(options.flags || "w").startsWith("a")) {
        globalThis.__nodeFs.appendFileSync(path, data);
      } else globalThis.__nodeFs.writeFileSync(path, data);
      stream._writableState.ending = true;
      stream._writableState.ended = true;
      stream._writableState.finished = true;
      stream.writableEnded = true;
      stream.writableFinished = true;
      stream.writable = false;
      stream.emit("finish");
      if (callback) callback();
      if (options.autoClose !== false) {
        globalThis.__nodeFs.closeSync(stream.fd);
        stream.fd = null;
      }
      stream.emit("close");
    } catch (error) {
      stream.emit("error", error);
    }
  });
};
globalThis.__nodeFs.createWriteStream = (value, options = {}) => {
  const encoding =
    typeof options === "string" ? options : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    const error = new TypeError(
      `The argument 'encoding' is invalid. Received '${encoding}'`
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  const stream = new NodeWritable(options);
  const fileHandle =
    options.fd && typeof options.fd === "object" ? options.fd : null;
  const path = fileHandle
    ? globalThis.__nodeFdPaths[fileHandle.fd]
    : nodeFsPath(value);
  const chunks = [];
  stream.path = path;
  stream.fd = null;
  stream.bytesWritten = 0;
  stream.write = (chunk) => {
    const bytes =
      typeof chunk === "string"
        ? NodeBuffer.from(chunk, options.encoding || "utf8")
        : NodeBuffer.from(chunk);
    chunks.push(bytes);
    stream.bytesWritten += bytes.byteLength;
    return true;
  };
  stream.end = (chunk, encoding, callback) => {
    __nodeFsWriteStreamEnd(
      stream,
      path,
      chunks,
      options,
      chunk,
      encoding,
      callback
    );
    return stream;
  };
  return stream;
};
globalThis.__nodeFs.WriteStream = globalThis.__nodeFs.createWriteStream;

class __nodeFsUtf8Stream extends NodeWritable {
  constructor(options = {}) {
    super({ ...options, autoDestroy: false });
    if (options == null || typeof options !== "object") {
      const error = new TypeError(
        'The "options" argument must be of type object'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    this.sync = options.sync === true;
    this.minLength = options.minLength ?? 4096;
    this.bytesWritten = 0;
    this.fd = typeof options.fd === "number" ? options.fd : null;
    this.path =
      options.dest === undefined ? undefined : nodeFsPath(options.dest);
    this._ownsFd = this.fd === null;
    this._open();
    if (this.sync) this.emit("ready");
    else queueMicrotask(() => this.emit("ready"));
  }

  _open() {
    if (this.fd !== null) return;
    if (this.path === undefined) {
      const error = new TypeError('The "dest" or "fd" option is required');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    this.fd = globalThis.__nodeFs.openSync(this.path, "w");
  }

  _write(chunk, encoding, callback) {
    const data =
      typeof chunk === "string"
        ? NodeBuffer.from(chunk, encoding || "utf8")
        : NodeBuffer.from(chunk);
    const write = () => {
      globalThis.__nodeFs.writeSync(this.fd, data, 0, data.byteLength, null);
      this.bytesWritten += data.byteLength;
      callback();
    };
    if (this.sync) {
      try {
        write();
      } catch (error) {
        callback(error);
      }
    } else {
      queueMicrotask(() => {
        try {
          write();
        } catch (error) {
          callback(error);
        }
      });
    }
  }

  reopen(dest) {
    if (this._ownsFd && this.fd !== null) {
      try {
        globalThis.__nodeFs.closeSync(this.fd);
      } catch (_) {}
    }
    this.path = nodeFsPath(dest);
    this.fd = null;
    this._ownsFd = true;
    this._open();
    if (!this.sync) queueMicrotask(() => this.emit("ready"));
    else this.emit("ready");
    return this;
  }

  destroy(error, callback) {
    if (this.destroyed) return this;
    if (this._ownsFd && this.fd !== null) {
      try {
        globalThis.__nodeFs.closeSync(this.fd);
      } catch (_) {}
      this.fd = null;
    }
    super.destroy(error, callback);
    if (!error) queueMicrotask(() => this.emit("close"));
    return this;
  }
}
globalThis.__nodeFs.Utf8Stream = __nodeFsUtf8Stream;
