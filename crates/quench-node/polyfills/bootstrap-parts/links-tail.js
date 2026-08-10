globalThis.__nodeFs.fstatSync = (fd, options = {}) => {
  if (typeof fd !== "number") {
    const error = new TypeError(
      `The "fd" argument must be of type number.${globalThis.__nodeCommon.invalidArgTypeHelper(
        fd
      )}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (globalThis.__nodeFdPaths[fd] === undefined) {
    const error = new Error("EBADF: bad file descriptor, fstat");
    error.code = "EBADF";
    error.syscall = "fstat";
    throw error;
  }
  return globalThis.__nodeFs.statSync(globalThis.__nodeFdPaths[fd], options);
};
globalThis.__nodeFs.fstat = (fd, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
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
      result = globalThis.__nodeFs.fstatSync(fd, options);
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
globalThis.__nodeFs.createWriteStream = function (value, options = {}) {
  const compatTarget =
    this !== globalThis.__nodeFs &&
    this instanceof globalThis.__nodeFs.WriteStream
      ? this
      : null;
  const encoding =
    typeof options === "string" ? options : options && options.encoding;
  if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
    const error = new TypeError(
      `The argument 'encoding' is invalid. Received '${encoding}'`
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  const stream = options.__target || compatTarget || new NodeWritable(options);
  if (compatTarget && !compatTarget._events) {
    Object.assign(
      compatTarget,
      new NodeWritable({
        ...options,
        __quenchCompatConstruct: true
      })
    );
  }
  const fileHandle =
    options.fd && typeof options.fd === "object" ? options.fd : null;
  const path = fileHandle
    ? globalThis.__nodeFdPaths[fileHandle.fd]
    : nodeFsPath(value);
  const chunks = [];
  stream.path = path;
  stream.flags = options.flags || "w";
  stream.mode = options.mode;
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
  if ((options.__target || compatTarget) && typeof stream.open === "function") {
    queueMicrotask(() => stream.open());
  }
  return stream;
};
globalThis.__nodeFs.WriteStream = globalThis.__nodeFs.createWriteStream;
globalThis.__nodeFs.WriteStream.prototype = Object.create(
  NodeWritable.prototype
);
globalThis.__nodeFs.WriteStream.prototype.constructor =
  globalThis.__nodeFs.WriteStream;
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
      callback(null, globalThis.__nodeFs.lstatSync(path, options));
    } catch (error) {
      callback(error);
      return;
    }
  });
};
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
