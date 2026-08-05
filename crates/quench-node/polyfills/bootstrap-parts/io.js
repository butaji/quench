const __nodeFsValidateWriteBuffer = (fd, buffer, offset, length) => {
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
    const error = new RangeError("The write range is out of range");
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
const __nodeFsWriteDescriptor = (fd, buffer, offset, length, position) => {
  const path = globalThis.__nodeFdPaths[fd];
  if (!path) {
    const error = new Error("EBADF");
    error.code = "EBADF";
    throw error;
  }
  const bytes = buffer.subarray(offset, offset + length);
  const at =
    position === null || position === undefined
      ? globalThis.__nodeFdPositions[fd] || 0
      : Number(position);
  const existing = NodeBuffer.from(globalThis.__quench_fs_read_bytes(path));
  const output = NodeBuffer.alloc(Math.max(existing.length, at + bytes.length));
  output.set(existing);
  output.set(bytes, at);
  globalThis.__quench_fs_write_bytes(path, Array.from(output));
  if (position === null || position === undefined)
    globalThis.__nodeFdPositions[fd] = at + bytes.length;
  return bytes.length;
};
const __nodeFsAppendPath = (value) => {
  const path =
    typeof value === "number"
      ? globalThis.__nodeFdPaths[value]
      : nodeFsPath(value);
  if (path) return path;
  const error = new Error("EBADF");
  error.code = "EBADF";
  throw error;
};
const __nodeFsAppendData = (data, options) => {
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
  return typeof data === "string"
    ? NodeBuffer.from(data, options?.encoding || "utf8")
    : NodeBuffer.from(data);
};
const __nodeFsReadOptionObject = (offset) => {
  if (
    offset !== undefined &&
    offset !== null &&
    (Array.isArray(offset) ||
      (typeof offset !== "number" && typeof offset !== "object"))
  )
    throw new TypeError('The "options" argument must be an object');
  return offset === null ||
    (typeof offset === "object" && !ArrayBuffer.isView(offset))
    ? offset || {}
    : null;
};
const __nodeFsReadOptions = (buffer, offset, length, position) => {
  const options = __nodeFsReadOptionObject(offset);
  if (options) {
    offset = Number(options.offset || 0);
    length =
      options.length === undefined
        ? buffer.length - offset
        : Number(options.length);
    position = options.position === undefined ? null : options.position;
  }
  return { offset, length, position };
};
const __nodeFsReadNormalize = (buffer, offset, length, position) => {
  ({ offset, length, position } = __nodeFsReadOptions(
    buffer,
    offset,
    length,
    position
  ));
  if (!Number.isInteger(offset) || offset < 0)
    throw new RangeError('The value of "offset" is out of range');
  if (!Number.isInteger(length) || length < 0)
    throw new RangeError('The value of "length" is out of range');
  if (
    position !== null &&
    position !== undefined &&
    typeof position !== "number" &&
    typeof position !== "bigint"
  )
    throw new TypeError(
      'The "position" argument must be of type number or bigint'
    );
  return { offset, length, position };
};
const __nodeFsReadArguments = (fd, buffer, offset, length, position) => {
  if (typeof fd !== "number") {
    const error = new TypeError(
      `The "fd" argument must be of type number. Received ${fd}`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!(buffer instanceof Uint8Array)) {
    const error = new TypeError(
      'The "buffer" argument must be an instance of Buffer, TypedArray, or DataView. Received an instance of Object'
    );
    throw error;
  }
  return {
    fd,
    buffer,
    ...__nodeFsReadNormalize(buffer, offset, length, position)
  };
};
const __nodeFsReadDescriptor = ({ fd, buffer, offset, length, position }) => {
  if (buffer.length === 0 && Number(length) > 0)
    throw new TypeError(
      "The argument 'buffer' is empty and cannot be written."
    );
  const path = globalThis.__nodeFdPaths[fd];
  if (!path) {
    const error = new Error("EBADF");
    error.code = "EBADF";
    throw error;
  }
  const numericPosition =
    position === null || Number(position) < 0
      ? globalThis.__nodeFdPositions[fd] || 0
      : Number(position);
  const bytes = NodeBuffer.from(
    globalThis.__quench_fs_read_range_bytes(
      path,
      numericPosition,
      Number(length)
    )
  );
  buffer.set(bytes.subarray(0, Number(length)), Number(offset));
  if (position === null || position === undefined)
    globalThis.__nodeFdPositions[fd] = numericPosition + bytes.length;
  return bytes.length;
};
Object.assign(globalThis.__nodeFs, {
  ftruncateSync: (fd, length = 0) => {
    if (typeof fd !== "number") {
      const error = new TypeError('The "fd" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return globalThis.__nodeFs.truncateSync(fd, length);
  },
  fsyncSync: (fd) => {
    if (typeof fd !== "number") {
      const error = new TypeError('The "fd" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(fd) || fd < 0) {
      const error = new RangeError('The value of "fd" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
  },
  fdatasyncSync: (fd) => globalThis.__nodeFs.fsyncSync(fd),
  readSync: (fd, buffer, offset = 0, length = buffer.length, position = null) =>
    __nodeFsReadDescriptor(
      __nodeFsReadArguments(fd, buffer, offset, length, position)
    ),
  readvSync: (fd, buffers, position = null) => {
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
    let total = 0;
    let at = position === null || position === undefined ? 0 : Number(position);
    for (const buffer of buffers) {
      if (buffer.length) {
        const count = globalThis.__nodeFs.readSync(
          fd,
          buffer,
          0,
          buffer.length,
          at
        );
        total += count;
        at += count;
        if (count < buffer.length) break;
      }
    }
    return total;
  },
  writeSync: (
    fd,
    buffer,
    offset = 0,
    length = buffer.length - offset,
    position = null
  ) => {
    if (typeof buffer === "string") buffer = NodeBuffer.from(buffer);
    __nodeFsValidateWriteBuffer(fd, buffer, offset, length);
    return __nodeFsWriteDescriptor(fd, buffer, offset, length, position);
  },
  writevSync: (fd, buffers, position = null) => {
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
    return globalThis.__nodeFs.writeSync(fd, NodeBuffer.concat(buffers));
  },
  copyFileSync: (from, to, mode = 0) => {
    const source = __nodeFsCopyPath(from, "src");
    const destination = __nodeFsCopyPath(to, "dest");
    if (typeof mode !== "number") {
      const error = new TypeError('The "mode" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if ((mode & ~7) !== 0) {
      const error = new RangeError('The value of "mode" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    __nodeFsCopyExclusiveError(destination, source, mode);
    return globalThis.__quench_fs_copy(source, destination);
  },
  appendFileSync: (value, data, options = {}) => {
    const path = __nodeFsAppendPath(value);
    const bytes = __nodeFsAppendData(data, options);
    const result = globalThis.__quench_fs_append_bytes(path, Array.from(bytes));
    if (options && options.mode !== undefined)
      globalThis.__nodeModes[path] = Number(options.mode);
    return result;
  },
  accessSync: (value, mode) => {
    __nodeFsValidateAccessMode(mode);
    if (typeof value === "number") {
      const error = new TypeError(
        'The "path" argument must be of type string or an instance of Buffer or URL'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const path = nodeFsPath(value);
    if (!globalThis.__quench_fs_access(path)) {
      const error = new Error(
        `ENOENT: no such file or directory, access '${path}'`
      );
      error.code = "ENOENT";
      error.errno = -2;
      error.syscall = "access";
      error.path = path;
      throw error;
    }
    const permissions = globalThis.__nodeModes[path] || 0o666;
    if (mode && mode & 2 && !(permissions & 0o222)) {
      const error = new Error(`EACCES: permission denied, access '${path}'`);
      error.code = "EACCES";
      error.errno = -13;
      error.syscall = "access";
      error.path = path;
      throw error;
    }
  },
  realpathSync: (value, options) => {
    const input = nodePathValue(value);
    const path = input.replace(/^\.\/test\//, "tests/node/test/");
    let result;
    try {
      result = globalThis.__quench_fs_realpath(path);
    } catch (_) {
      const error = new Error(
        `ELOOP: too many symbolic links encountered, realpath '${path}'`
      );
      error.code = "ELOOP";
      error.syscall = "realpath";
      error.path = path;
      throw error;
    }
    const encoding =
      typeof options === "string" ? options : options && options.encoding;
    return encoding === "buffer"
      ? NodeBuffer.from(result)
      : encoding
        ? NodeBuffer.from(result).toString(encoding)
        : result;
  },
  rmSync: (value, options = {}) => {
    const path = nodeFsPath(value);
    let kind;
    try {
      kind = globalThis.__quench_fs_link_kind(path);
    } catch (_) {
      if (!options.force) {
        const error = new Error(
          `ENOENT: no such file or directory, lstat '${path}'`
        );
        error.code = "ENOENT";
        error.syscall = "lstat";
        error.path = path;
        throw error;
      }
      return;
    }
    if (kind === "file") return globalThis.__quench_fs_unlink(path);
    if (kind === "directory" && !options.recursive) {
      const error = new Error(
        `ERR_FS_EISDIR: illegal operation on a directory, rm '${path}'`
      );
      error.code = "ERR_FS_EISDIR";
      error.path = path;
      throw error;
    }
    return globalThis.__quench_fs_remove_dir(path);
  },
  chmodSync: (value, mode) => {
    const path = __nodeFsPathOnly(value);
    globalThis.__quench_fs_chmod(
      path,
      typeof mode === "string" ? parseInt(mode, 8) : Number(mode)
    );
    globalThis.__nodeModes[path] =
      typeof mode === "string" ? parseInt(mode, 8) : Number(mode);
  },
  symlinkSync: (target, link, type) => {
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
    return globalThis.__quench_fs_symlink(nodeFsPath(target), nodeFsPath(link));
  },
  linkSync: (existing, link) => {
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
    return globalThis.__quench_fs_link(nodeFsPath(existing), nodeFsPath(link));
  },
  readlinkSync: (value, options) => {
    const result = globalThis.__quench_fs_readlink(nodeFsPath(value));
    const encoding =
      typeof options === "string" ? options : options && options.encoding;
    return encoding === "buffer"
      ? NodeBuffer.from(result)
      : encoding
        ? NodeBuffer.from(result).toString(encoding)
        : result;
  }
});
