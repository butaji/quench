globalThis.__nodeFs.truncate = (value, length, callback) => {
  if (typeof length === "function") {
    callback = length;
    length = 0;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (typeof length !== "number" || !Number.isFinite(length)) {
    const error = new TypeError('The "len" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!Number.isInteger(length)) {
    const error = new RangeError('The value of "len" is out of range');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  const path =
    typeof value === "number"
      ? globalThis.__nodeFdPaths[value]
      : nodeFsPath(value);
  queueMicrotask(() => {
    try {
      globalThis.__quench_fs_truncate(path, Number(length));
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.ftruncate = (fd, length = 0, callback) => {
  if (typeof length === "function") {
    callback = length;
    length = 0;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.ftruncateSync(fd, length);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.access = (value, mode, callback) => {
  if (typeof mode === "function") callback = mode;
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (typeof value === "number") {
    const error = new TypeError(
      'The "path" argument must be of type string or an instance of Buffer or URL'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.accessSync(path, mode);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.fsync = (fd, callback) => {
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.fsyncSync(fd);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.fdatasync = (fd, callback) => {
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.fdatasyncSync(fd);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.read = (fd, buffer, offset, length, position, callback) => {
  if (typeof buffer === "function" || buffer === undefined) {
    callback = buffer;
    buffer = NodeBuffer.alloc(16384);
    offset = 0;
    length = buffer.length;
    position = null;
  } else if (buffer === null) {
    callback = offset;
    buffer = NodeBuffer.alloc(16384);
    offset = 0;
    length = buffer.length;
    position = null;
  } else if (typeof buffer === "object" && !ArrayBuffer.isView(buffer)) {
    const options = buffer;
    callback = offset;
    buffer =
      options.buffer ||
      NodeBuffer.alloc(
        options.length === undefined ? 16384 : Number(options.length)
      );
    offset = options.offset == null ? 0 : Number(options.offset);
    length =
      options.length === undefined
        ? buffer.length - offset
        : Number(options.length);
    position = options.position === undefined ? null : options.position;
  } else if (typeof offset === "function") {
    callback = offset;
    offset = 0;
    length = buffer.length;
    position = null;
  } else if (
    typeof offset === "object" ||
    offset === null ||
    offset === undefined
  ) {
    const options = offset || {};
    callback = length;
    offset = Number(options.offset || 0);
    length =
      options.length === undefined
        ? buffer.length - offset
        : Number(options.length);
    position = options.position === undefined ? null : options.position;
  } else if (typeof position === "function") {
    callback = position;
    position = null;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (buffer.length === 0 && Number(length) > 0) {
    const error = new TypeError(
      "The argument 'buffer' is empty and cannot be written."
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!(buffer instanceof Uint8Array)) {
    const error = new TypeError(
      'The "buffer" argument must be an instance of Buffer, TypedArray, or DataView'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    !Number.isInteger(offset) ||
    offset < 0 ||
    !Number.isInteger(length) ||
    length < 0
  ) {
    const error = new RangeError("The read range is out of range");
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  if (
    position !== null &&
    position !== undefined &&
    typeof position !== "number" &&
    typeof position !== "bigint"
  ) {
    const error = new TypeError(
      'The "position" argument must be of type number or bigint'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      const count = globalThis.__nodeFs.readSync(
        fd,
        buffer,
        offset,
        length,
        position
      );
      callback(null, count, buffer);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.readv = (fd, buffers, position, callback) => {
  if (typeof position === "function") {
    callback = position;
    position = null;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (
    !Array.isArray(buffers) ||
    buffers.some((buffer) => !(buffer instanceof Uint8Array))
  ) {
    const error = new TypeError(
      'The "buffers" argument must be an array of Buffer or Uint8Array'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      callback(
        null,
        globalThis.__nodeFs.readvSync(fd, buffers, position),
        buffers
      );
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.write = (
  fd,
  buffer,
  offset,
  length,
  position,
  callback
) => {
  if (
    typeof buffer === "object" &&
    buffer !== null &&
    !ArrayBuffer.isView(buffer)
  ) {
    const options = buffer;
    callback = offset;
    buffer = options.buffer;
    offset = options.offset || 0;
    length =
      options.length === undefined
        ? buffer && buffer.length - offset
        : options.length;
    position = options.position;
  } else if (typeof position === "function") {
    callback = position;
    position = null;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (
    typeof fd !== "number" ||
    !(typeof buffer === "string" || buffer instanceof Uint8Array)
  ) {
    const error = new TypeError("Invalid write arguments");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      callback(
        null,
        globalThis.__nodeFs.writeSync(fd, buffer, offset, length, position),
        buffer
      );
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.writev = (fd, buffers, position, callback) => {
  if (typeof position === "function") {
    callback = position;
    position = null;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    !Array.isArray(buffers) ||
    buffers.some((buffer) => !(buffer instanceof Uint8Array))
  ) {
    const error = new TypeError(
      'The "buffers" argument must be an array of Buffer or Uint8Array'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      callback(
        null,
        globalThis.__nodeFs.writevSync(fd, buffers, position),
        buffers
      );
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeModes = {};
globalThis.__nodeFdPaths = {};
globalThis.__nodeFdPositions = {};
const nodeMode = (mode) => {
  const value = typeof mode === "string" ? parseInt(mode, 8) : Number(mode);
  if (!Number.isFinite(value) || value < 0 || value > 0xffffffff) {
    const error = new RangeError('The value of "mode" is out of range');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return value;
};
globalThis.__nodeFs.fchmodSync = (fd, mode) => {
  if (!Number.isInteger(fd) || fd < 0 || fd > 0x7fffffff) {
    const error = new RangeError('The value of "fd" is out of range');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  const value = nodeMode(mode);
  if (globalThis.__nodeFdPaths[fd])
    globalThis.__nodeFs.chmodSync(globalThis.__nodeFdPaths[fd], value);
};
globalThis.__nodeFs.fchmod = (fd, mode, callback) => {
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  globalThis.__nodeFs.fchmodSync(fd, mode);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.closeSync(fd);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.statfsSync = (value, options = {}) => {
  const path = nodeFsPath(value);
  if (!globalThis.__quench_fs_access(path)) throw new Error("ENOENT");
  const values = {
    type: 0,
    bsize: 4096,
    frsize: 4096,
    blocks: 1,
    bfree: 1,
    bavail: 1,
    files: 1,
    ffree: 1
  };
  if (options && options.bigint)
    Object.keys(values).forEach((key) => {
      values[key] = BigInt(values[key]);
    });
  return values;
};
globalThis.__nodeFs.statfs = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.statfsSync(path, options));
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.symlink = (target, link, type, callback) => {
  if (typeof type === "function") {
    callback = type;
    type = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (
    (typeof target !== "string" && !(target instanceof Uint8Array)) ||
    (typeof link !== "string" && !(link instanceof Uint8Array))
  ) {
    const error = new TypeError(
      'The "target" and "path" arguments must be strings or Buffer'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    type !== undefined &&
    type !== "file" &&
    type !== "dir" &&
    type !== "junction"
  ) {
    const error = new TypeError('The "type" argument is invalid');
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  const source = nodePathValue(target);
  const destination = nodeFsPath(link);
  queueMicrotask(() => {
    try {
      globalThis.__quench_fs_symlink(source, destination);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.readlink = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.readlinkSync(path, options));
    } catch (error) {
      callback(error);
      return;
    }
  });
};
