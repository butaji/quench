globalThis.__nodeFs.link = (existing, link, callback) => {
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
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
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
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
  if (typeof options === "function") callback = options;
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
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
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
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
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
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.rmSync(path, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.rename = (from, to, callback) => {
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const source = nodeFsPath(from);
  const destination = nodeFsPath(to);
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
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const source = nodeFsPath(from);
  const destination = nodeFsPath(to);
  if (typeof mode !== "number") {
    const error = new TypeError('The "mode" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
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
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
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
globalThis.__nodeFs.Dirent = class Dirent {
  constructor(name, type = 1) {
    this.name = name;
    this._type = type === true ? 2 : type === false ? 1 : type;
  }
  isFile() {
    return this._type === 1;
  }
  isDirectory() {
    return this._type === 2;
  }
  isSymbolicLink() {
    return this._type === 3;
  }
  isFIFO() {
    return this._type === 4;
  }
  isSocket() {
    return this._type === 5;
  }
  isCharacterDevice() {
    return this._type === 6;
  }
  isBlockDevice() {
    return this._type === 7;
  }
};
globalThis.__nodeFs.Dir = class Dir {
  constructor(path) {
    this.path = path;
    this._entries = globalThis.__nodeFs.readdirSync(path, {
      withFileTypes: true
    });
    this._index = 0;
    this._closed = false;
  }
  readSync() {
    if (this._closed) {
      const error = new Error("Directory handle was closed");
      error.code = "ERR_DIR_CLOSED";
      throw error;
    }
    return this._entries[this._index++] || null;
  }
  closeSync() {
    if (this._closed) {
      const error = new Error("Directory handle was closed");
      error.code = "ERR_DIR_CLOSED";
      throw error;
    }
    this._closed = true;
  }
  read(callback) {
    if (typeof callback !== "function")
      throw new TypeError('The "callback" argument must be of type function');
    queueMicrotask(() => {
      try {
        callback(null, this.readSync());
      } catch (error) {
        callback(error);
      }
    });
  }
  close(callback) {
    if (typeof callback !== "function")
      throw new TypeError('The "callback" argument must be of type function');
    queueMicrotask(() => {
      try {
        this.closeSync();
        callback(null);
      } catch (error) {
        callback(error);
      }
    });
  }
};
globalThis.__nodeFs.opendirSync = (value) =>
  new globalThis.__nodeFs.Dir(nodeFsPath(value));
globalThis.__nodeFs.opendir = (value, options, callback) => {
  if (typeof options === "function") callback = options;
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      callback(null, new globalThis.__nodeFs.Dir(path));
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.lstatSync = (value) => {
  const path = nodeFsPath(value);
  const kind = globalThis.__quench_fs_link_kind(path);
  const stats = new globalThis.__nodeStats(
    kind === "file",
    kind === "directory",
    new Date()
  );
  stats._symlink = kind === "symlink";
  stats.mode = globalThis.__nodeModes[path] || 0;
  return stats;
};
globalThis.__nodeFs.stat = (value, options, callback) => {
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
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
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
    const error = new TypeError('The "fd" argument must be of type number');
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
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
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
    const error = new TypeError('The "fd" argument must be of type number');
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
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.closeSync(fd);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
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
      stream.fd = globalThis.__nodeFs.openSync(path, "w");
      stream.emit("open", stream.fd);
      const data = NodeBuffer.concat(chunks);
      if (String(options.flags || "w").startsWith("a"))
        globalThis.__nodeFs.appendFileSync(path, data);
      else globalThis.__nodeFs.writeFileSync(path, data);
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
  const stream = new NodeWritable(options);
  const path = nodeFsPath(value);
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
globalThis.__nodeFs.ReadStream = globalThis.__nodeFs.createReadStream;
