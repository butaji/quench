//! Polyfill: `links`

pub const JS: &str = quench_js_check::checked_js!(r#"globalThis.__nodeFs.link = (existing, link, callback) => {
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (
    (typeof existing !== "string" && !(existing instanceof Uint8Array)) ||
    (typeof link !== "string" && !(link instanceof Uint8Array))
  ) {
    throw Object.assign(new TypeError('The "path" argument must be of type string or an instance of Buffer or URL'), { code: "ERR_INVALID_ARG_TYPE" });
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
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
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
    throw Object.assign(new TypeError('The "data" argument must be of type string or an instance of Buffer'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (typeof callback !== "function") {
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  const encoding =
    typeof options === "string" ? options : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    throw Object.assign(new TypeError(`The argument 'encoding' is invalid. Received '${encoding}'`), { code: "ERR_INVALID_ARG_VALUE" });
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
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    error.toString = () => `TypeError [ERR_INVALID_ARG_TYPE]: ${error.message}`;
    throw error;
  }
  if (options?.recursive === true) {
    throw Object.assign(new TypeError("The recursive option is no longer supported for fs.rmdir"), { code: "ERR_INVALID_ARG_VALUE" });
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.rmdirSync(path, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.rm = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const path = nodeFsPath(value).replace(/^\.\/test\//, "tests/node/test/");
  queueMicrotask(() => {
    try {
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
    throw Object.assign(new TypeError('The "callback" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  const source = __nodeFsCopyPath(from, "src");
  const destination = __nodeFsCopyPath(to, "dest");
  if (typeof mode !== "number") {
    throw Object.assign(new TypeError('The "mode" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
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
    throw Object.assign(new TypeError(`The argument 'encoding' is invalid. Received '${encoding}'`), { code: "ERR_INVALID_ARG_VALUE" });
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
for (const name of ["isSocket", "isBlockDevice", "isCharacterDevice", "isFIFO"])
  globalThis.__nodeStats.prototype[name] = () => false;
globalThis.__nodeStats.prototype.isSymbolicLink = function () {
  return this._symlink === true;
};
globalThis.__nodeFs.Dir = class Dir {
  constructor(path) {
    if (path === undefined) {
      throw Object.assign(new TypeError('The "path" argument must be specified'), { code: "ERR_MISSING_ARGS" });
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
const __nodeStatsWithBigInt = (stats, options) => {
  if (options?.bigint !== true) return stats;
  for (const name of "dev mode nlink uid gid rdev blksize ino size blocks atimeMs mtimeMs ctimeMs birthtimeMs".split(
    " "
  ))
    stats[name] = BigInt(Math.trunc(Number(stats[name]) || 0));
  for (const name of ["atime", "mtime", "ctime", "birthtime"]) {
    stats[`${name}Ns`] = BigInt(
      Math.trunc(Number(stats[`${name}Ms`]) * 1_000_000)
    );
  }
  return stats;
};
globalThis.__nodeFs.lstatSync = (value, options = {}) => {
  const path = nodeFsPath(value);
  let kind;
  try {
    kind = globalThis.__quench_fs_link_kind(path);
  } catch (_) {
    if (options?.throwIfNoEntry === false) return undefined;
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
  return __nodeStatsWithBigInt(stats, options);
};
"#);
