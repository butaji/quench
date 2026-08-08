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
  if (position === null || position === undefined) {
    globalThis.__nodeFdPositions[fd] = at + bytes.length;
  }
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
  ) {
    throw new TypeError('The "options" argument must be an object');
  }
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
  if (!Number.isInteger(offset) || offset < 0) {
    throw Object.assign(
      new RangeError('The value of "offset" is out of range'),
      { code: "ERR_OUT_OF_RANGE" }
    );
  }
  if (!Number.isInteger(length) || length < 0) {
    throw Object.assign(
      new RangeError('The value of "length" is out of range'),
      { code: "ERR_OUT_OF_RANGE" }
    );
  }
  if (
    position !== null &&
    position !== undefined &&
    typeof position !== "number" &&
    typeof position !== "bigint"
  ) {
    throw new TypeError(
      'The "position" argument must be of type number or bigint'
    );
  }
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
  if (buffer.length === 0 && Number(length) > 0) {
    throw new TypeError(
      "The argument 'buffer' is empty and cannot be written."
    );
  }
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
  if (position === null || position === undefined) {
    globalThis.__nodeFdPositions[fd] = numericPosition + bytes.length;
  }
  return bytes.length;
};
const __nodeFsRemoveTree = (path) => {
  const mode = globalThis.__nodeModes[path];
  if (mode !== undefined && (mode & 0o300) !== 0o300) {
    const code =
      process.platform === "darwin" && mode & 0o111 ? "ENOTEMPTY" : "EACCES";
    const error = new Error(`${code}: permission denied, rm '${path}'`);
    error.code = code;
    error.syscall = "rm";
    error.path = path;
    throw error;
  }
  for (const name of globalThis.__nodeFs.readdirSync(path)) {
    const child = `${path.replace(/\/$/, "")}/${name}`;
    if (globalThis.__nodeFs.lstatSync(child).isSymbolicLink()) {
      globalThis.__quench_fs_unlink(child);
      continue;
    }
    const kind = globalThis.__quench_fs_link_kind(child);
    if (kind === "directory") __nodeFsRemoveTree(child);
    else globalThis.__quench_fs_unlink(child);
  }
  return globalThis.__quench_fs_remove_dir(path);
};
const __nodeFsRemovePath = (path, kind, options) => {
  if (globalThis.__nodeFs.lstatSync(path).isSymbolicLink()) {
    return globalThis.__quench_fs_unlink(path);
  }
  if (kind === "file") return globalThis.__quench_fs_unlink(path);
  if (
    kind === "directory" &&
    !options.recursive &&
    (options.__sync ||
      (!options.__async && globalThis.__nodeFs.readdirSync(path).length > 0))
  ) {
    const error = new Error(
      `ERR_FS_EISDIR: illegal operation on a directory, rm '${path}'`
    );
    error.code = "ERR_FS_EISDIR";
    error.path = path;
    throw error;
  }
  return options.recursive
    ? __nodeFsRemoveTree(path)
    : globalThis.__quench_fs_remove_dir(path);
};
const __nodeFsRunRemoval = (path, kind, options) => {
  try {
    return __nodeFsRemovePath(path, kind, options);
  } catch (error) {
    error.syscall ||= "rm";
    error.path ||= path;
    throw error;
  }
};
Object.assign(globalThis.__nodeFs, {
  ftruncateSync: (fd, length = 0) => {
    if (typeof fd !== "number") {
      const error = new TypeError(
        `The "fd" argument must be of type number.${__nodeInvalidArgSuffix(fd)}`
      );
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
  writeSync: (fd, buffer, offset = 0, length, position = null) => {
    if (typeof buffer === "string") {
      position = offset;
      if (length === "hex" && buffer.length % 2 !== 0) {
        const error = new TypeError(
          `'encoding' is invalid for data of length ${buffer.length}`
        );
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      buffer = NodeBuffer.from(
        buffer,
        typeof length === "string" ? length : "utf8"
      );
      offset = 0;
      length = buffer.length;
    }
    if (length === undefined && buffer instanceof Uint8Array) {
      length = buffer.length - offset;
    }
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
    const trackedMode = globalThis.__nodeModes?.[destination];
    if (trackedMode !== undefined && (trackedMode & 0o222) === 0) {
      const error = new Error(
        `EACCES: permission denied, copyfile '${source}' -> '${destination}'`
      );
      error.code = "EACCES";
      error.syscall = "copyfile";
      error.path = destination;
      throw error;
    }
    __nodeFsCopyExclusiveError(destination, source, mode);
    // Node replaces a destination symlink for copyFileSync().  The host
    // filesystem primitive follows that symlink, so remove only the link
    // before copying; real destination directories/files retain the native
    // overwrite and error behavior.
    try {
      if (__nodeFs.lstatSync(destination).isSymbolicLink?.()) {
        __nodeFs.unlinkSync(destination);
      }
    } catch (_) {
      // Let the host copy operation report the canonical error for missing or
      // inaccessible destinations.
    }
    return globalThis.__quench_fs_copy(source, destination);
  },
  appendFileSync: (value, data, options = {}) => {
    const encoding =
      typeof options === "string" ? options : options && options.encoding;
    if (
      encoding !== undefined &&
      encoding !== "buffer" &&
      !NodeBuffer.isEncoding(encoding)
    ) {
      const error = new TypeError(
        `The argument 'encoding' is invalid. Received '${encoding}'`
      );
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    const path = __nodeFsAppendPath(value);
    const bytes = __nodeFsAppendData(data, options);
    const result =
      bytes instanceof Uint8Array
        ? globalThis.__quench_fs_append_typed(path, bytes)
        : globalThis.__quench_fs_append_bytes(path, Array.from(bytes));
    if (options && options.mode !== undefined) {
      globalThis.__nodeModes[path] = Number(options.mode);
    }
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
    const runningAsRoot = process.getuid?.() === 0;
    if (mode && mode & 2 && !runningAsRoot && !(permissions & 0o222)) {
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
    const encoding =
      typeof options === "string" ? options : options && options.encoding;
    if (
      encoding !== undefined &&
      encoding !== "buffer" &&
      !NodeBuffer.isEncoding(encoding)
    ) {
      const error = new TypeError(
        `The argument 'encoding' is invalid. Received '${encoding}'`
      );
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
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
    return encoding === "buffer"
      ? NodeBuffer.from(result)
      : encoding
        ? NodeBuffer.from(result).toString(encoding)
        : result;
  },
  rmSync: (value, options = {}) => {
    const path = nodeFsPath(value);
    const parent = path.slice(0, path.lastIndexOf("/")) || ".";
    const parentMode = globalThis.__nodeModes[parent];
    if (parentMode !== undefined && (parentMode & 0o300) !== 0o300) {
      const error = new Error(`EACCES: permission denied, rm '${path}'`);
      error.code = "EACCES";
      error.syscall = "rm";
      error.path = path;
      throw error;
    }
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
    return __nodeFsRunRemoval(path, kind, { ...options, __sync: true });
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
    const encoding =
      typeof options === "string" ? options : options && options.encoding;
    if (encoding !== undefined && !NodeBuffer.isEncoding(encoding)) {
      const error = new TypeError(
        `The argument 'encoding' is invalid. Received '${encoding}'`
      );
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    const result = globalThis.__quench_fs_readlink(nodeFsPath(value));
    return encoding === "buffer"
      ? NodeBuffer.from(result)
      : encoding
        ? NodeBuffer.from(result).toString(encoding)
        : result;
  }
});
