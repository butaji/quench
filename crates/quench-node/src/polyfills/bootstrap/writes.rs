//! Polyfill: `writes`

pub const JS: &str = r#"const __nodeFsCollectWriteIterable = (data, options) => {
  const chunks = [];
  for (const chunk of data) {
    if (
      typeof chunk !== "string" &&
      !(chunk instanceof Uint8Array) &&
      !ArrayBuffer.isView(chunk)
    ) {
      throw Object.assign(new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    chunks.push(chunk);
  }
  return chunks.length === 1 ? chunks[0] : NodeBuffer.concat(
    chunks.map((chunk) =>
      typeof chunk === "string"
        ? NodeBuffer.from(
          chunk,
          typeof options === "string" ? options : options && options.encoding,
        )
        : NodeBuffer.from(chunk)
    ),
  );
};
const __nodeFsNormalizeAsyncWriteData = async (data) => {
  if (
    data &&
    typeof data !== "string" &&
    !(data instanceof Uint8Array) &&
    !(data instanceof ArrayBuffer) &&
    typeof data[Symbol.asyncIterator] === "function"
  ) {
    const chunks = [];
    for await (const chunk of data) chunks.push(chunk);
    data = chunks;
  }
  return data;
};
const __nodeFsNormalizeWriteData = async (data, options) => {
  if (data && Array.isArray(data._sourceChunks)) {
    const stream = data;
    data = stream._sourceChunks.slice(stream._index || 0);
    stream._index = stream._sourceChunks.length;
    stream._ended = true;
    stream.readableEnded = true;
  } else if (data && Array.isArray(data._chunks)) {
    data = data._chunks.splice(0);
  }
  data = await __nodeFsNormalizeAsyncWriteData(data);
  if (
    data &&
    typeof data !== "string" &&
    !(data instanceof Uint8Array) &&
    !ArrayBuffer.isView(data) &&
    typeof data[Symbol.iterator] === "function"
  ) {
    data = __nodeFsCollectWriteIterable(data, options);
  }
  return data;
};
const __nodeFsValidateWriteData = (data) => {
  if (
    typeof data !== "string" &&
    !(data instanceof Uint8Array) &&
    !ArrayBuffer.isView(data) &&
    !(data instanceof ArrayBuffer)
  ) {
    throw Object.assign(new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView'), { code: "ERR_INVALID_ARG_TYPE" });
  }
};
const __nodeFsHandleWriteFile = async (fd, data, options) => {
  if (options && options.signal) {
    await new Promise((resolve) => queueMicrotask(resolve));
  }
  if (options && options.signal && options.signal.aborted) {
    const error = new Error("The operation was aborted");
    error.name = "AbortError";
    error.code = "ABORT_ERR";
    throw error;
  }
  data = await __nodeFsNormalizeWriteData(data, options);
  __nodeFsValidateWriteData(data);
  return globalThis.__nodeFs.writeFileSync(
    fd,
    data,
    typeof options === "string" ? { encoding: options } : options,
  );
};
const __nodeFsWriteArguments = (buffer, offset, length, position) => {
  if (
    offset !== undefined &&
    offset !== null &&
    typeof offset !== "number" &&
    typeof offset !== "object" &&
    typeof offset !== "string"
  ) {
    throw Object.assign(new TypeError('The "options" argument must be of type object or string'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  const named = offset &&
    typeof offset === "object" &&
    !ArrayBuffer.isView(offset) &&
    !(offset instanceof ArrayBuffer);
  const source = named ? buffer : buffer;
  const start = named
    ? offset.offset === undefined ? 0 : offset.offset
    : offset || 0;
  const size = named
    ? offset.length === undefined ? source.length - start : offset.length
    : length === undefined
    ? source.length - start
    : length;
  const at = named ? offset.position : position;
  if (
    typeof start !== "number" ||
    typeof size !== "number" ||
    !Number.isInteger(start) ||
    !Number.isInteger(size) ||
    start < 0 ||
    size < 0 ||
    start + size > source.length
  ) {
    throw Object.assign(new RangeError("The value is out of range"), { code: "ERR_OUT_OF_RANGE" });
  }
  return { source, start, size, at };
};
const __nodeFsHandleWrite = (handle, buffer, offset, length, position) => {
  if (
    buffer == null ||
    (typeof buffer !== "string" &&
      !(buffer instanceof Uint8Array) &&
      !ArrayBuffer.isView(buffer) &&
      !(buffer instanceof ArrayBuffer))
  ) {
    throw Object.assign(new TypeError('The "buffer" argument must be an instance of Buffer, TypedArray, or DataView'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  const { source, start, size, at } = __nodeFsWriteArguments(
    buffer,
    offset,
    length,
    position,
  );
  return {
    bytesWritten: globalThis.__nodeFs.writeSync(
      handle.fd,
      source,
      start,
      size,
      at === undefined ? null : at,
    ),
    buffer: source,
  };
};
const __nodeFsHandleRead = (handle, buffer, offset, length, position) => {
  if (
    offset &&
    typeof offset === "object" &&
    !ArrayBuffer.isView(offset) &&
    !(offset instanceof ArrayBuffer)
  ) {
    const options = offset;
    offset = options.offset === undefined ? 0 : options.offset;
    length = options.length == null ? buffer.length - offset : options.length;
    position = options.position;
  }
  if (buffer && typeof buffer === "object" && !ArrayBuffer.isView(buffer)) {
    const options = buffer;
    buffer = options.buffer === undefined
      ? NodeBuffer.alloc(16384)
      : options.buffer;
    offset = options.offset === undefined ? 0 : options.offset;
    length = options.length == null ? buffer.length - offset : options.length;
    position = options.position;
  } else if (buffer === undefined) {
    buffer = NodeBuffer.alloc(16384);
    offset = 0;
    length = buffer.length;
    position = null;
  } else {
    offset = offset === undefined ? 0 : offset;
    length = length == null ? buffer.length - offset : length;
    position = position === undefined ? null : position;
  }
  const bytesRead = globalThis.__nodeFs.readSync(
    handle.fd,
    buffer,
    Number(offset),
    Number(length),
    position,
  );
  return { bytesRead, buffer };
};
const __nodeFsAttachHandleIo = (handle) => {
  handle.read = (...args) =>
    Promise.resolve().then(() => __nodeFsHandleRead(handle, ...args));
  handle.write = (buffer, offset, length, position) =>
    Promise.resolve().then(() =>
      __nodeFsHandleWrite(handle, buffer, offset, length, position)
    );
  handle.readv = (buffers, position) =>
    Promise.resolve().then(() => ({
      bytesRead: globalThis.__nodeFs.readvSync(handle.fd, buffers, position),
      buffers,
    }));
  handle.writev = (buffers, position) =>
    Promise.resolve().then(() => ({
      bytesWritten: globalThis.__nodeFs.writevSync(
        handle.fd,
        buffers,
        position,
      ),
      buffers,
    }));
};
const __nodeFsAttachHandleOperations = (handle) => {
  handle.createReadStream = (options = {}) =>
    globalThis.__nodeFs.createReadStream(null, {
      ...options,
      fd: handle,
      autoClose: false,
    });
  handle.createWriteStream = (options = {}) =>
    globalThis.__nodeFs.createWriteStream(null, {
      ...options,
      fd: handle,
      autoClose: false,
    });
  handle.truncate = (length = 0) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.ftruncateSync(handle.fd, length)
    );
  handle.stat = (options) =>
    Promise.resolve().then(() => {
      if (handle.fd === -1) {
        const error = new Error("EBADF: bad file descriptor, fstat");
        error.code = "EBADF";
        error.syscall = "fstat";
        throw error;
      }
      return globalThis.__nodeFs.fstatSync(handle.fd, options);
    });
  handle.sync = () =>
    Promise.resolve().then(() => globalThis.__nodeFs.fsyncSync(handle.fd));
  handle.datasync = () =>
    Promise.resolve().then(() => globalThis.__nodeFs.fdatasyncSync(handle.fd));
  handle.chmod = (mode) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.chmodSync(globalThis.__nodeFdPaths[handle.fd], mode)
    );
  handle.readFile = (options) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.readFileSync(handle.fd, options)
    );
  handle.writeFile = (data, options) =>
    __nodeFsHandleWriteFile(handle.fd, data, options);
  handle.appendFile = async (data, options) => {
    if (options?.signal) {
      await new Promise((resolve) => queueMicrotask(resolve));
    }
    if (options?.signal?.aborted) {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      throw error;
    }
    data = await __nodeFsNormalizeWriteData(data, options);
    __nodeFsValidateWriteData(data);
    return globalThis.__nodeFs.appendFileSync(
      handle.fd,
      data,
      typeof options === "string" ? { encoding: options } : options,
    );
  };
  handle.close = () => {
    const fd = handle.fd;
    if (fd === -1) return Promise.resolve();
    handle.fd = -1;
    handle.emit("close");
    return Promise.resolve().then(() => globalThis.__nodeFs.closeSync(fd));
  };
  Symbol.asyncDispose ||= Symbol("Symbol.asyncDispose");
  handle[Symbol.asyncDispose] = handle.close;
};
globalThis.__nodeFs.promises.lchmod = async (value, mode) =>
  globalThis.__nodeFs.lchmodSync(value, mode);
globalThis.__nodeFs.promises.lchown = async (value, uid, gid) =>
  globalThis.__nodeFs.lchownSync(value, uid, gid);
globalThis.__nodeFs.promises.open = async (...args) => {
  const handle = await __nodePromiseOpen(...args);
  Object.setPrototypeOf(handle, NodeEventEmitter.prototype);
  handle._events = Object.create(null);
  handle.captureRejections = false;
  __nodeFsAttachHandleIo(handle);
  __nodeFsAttachHandleOperations(handle);
  return handle;
};
globalThis.__nodeFs.promises.appendFile = async (value, data, options) => {
  if (options?.signal) {
    await new Promise((resolve) => queueMicrotask(resolve));
    if (options.signal.aborted) {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      throw error;
    }
  }
  data = await __nodeFsNormalizeWriteData(data, options);
  __nodeFsValidateWriteData(data);
  if (options?.signal?.aborted) {
    const error = new Error("The operation was aborted");
    error.name = "AbortError";
    error.code = "ABORT_ERR";
    throw error;
  }
  const target =
    value && typeof value === "object" && typeof value.fd === "number"
      ? value.fd
      : value;
  globalThis.__nodeFs.appendFileSync(target, data, options);
};
globalThis.__nodeFs.promises.writeFile = async (value, data, options) => {
  if (options?.signal) {
    await new Promise((resolve) => queueMicrotask(resolve));
    if (options.signal.aborted) {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      throw error;
    }
  }
  data = await __nodeFsNormalizeWriteData(data, options);
  __nodeFsValidateWriteData(data);
  const target =
    value && typeof value === "object" && typeof value.fd === "number"
      ? value.fd
      : value;
  return globalThis.__nodeFs.writeFileSync(
    target,
    data,
    typeof options === "string" ? { encoding: options } : options,
  );
};
const __nodeOpenWithFilePosition = globalThis.__nodeFs.promises.open;
const __nodeFsValidatePullOptions = (options) => {
  if (
    options.autoClose !== undefined &&
    typeof options.autoClose !== "boolean"
  ) {
    throw Object.assign(new TypeError('The "autoClose" option must be of type boolean'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (
    options.signal !== undefined &&
    (!options.signal || typeof options.signal.aborted !== "boolean")
  ) {
    throw Object.assign(new TypeError('The "signal" option must be an AbortSignal'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  for (
    const [name, value] of [
      ["start", options.start],
      ["limit", options.limit],
      ["chunkSize", options.chunkSize],
    ]
  ) {
    if (value === undefined) continue;
    __nodeFsValidatePullNumber(name, value);
  }
};
const __nodeFsValidatePullNumber = (name, value) => {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw Object.assign(new TypeError(`The "${name}" option must be of type number`), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!Number.isInteger(value) || value < 0) {
    throw Object.assign(new RangeError(`The value of "${name}" is out of range`), { code: "ERR_OUT_OF_RANGE" });
  }
};
const __nodeFsPullBatches = (handle, options) => {
  const source = globalThis.__nodeFs.readFileSync(handle.fd);
  const start = options.start === undefined
    ? globalThis.__nodeFdPositions[handle.fd] || 0
    : Number(options.start);
  const end = options.limit === undefined
    ? source.length
    : Math.min(source.length, start + Number(options.limit));
  const chunkSize = options.chunkSize === undefined
    ? 128 * 1024
    : Number(options.chunkSize);
  const batches = [];
  for (let offset = start; offset < end; offset += chunkSize) {
    batches.push([source.subarray(offset, Math.min(end, offset + chunkSize))]);
  }
  if (start >= end) batches.push([]);
  return { batches, end };
};
const __nodeFsPullIterator = async function* (
  handle,
  batches,
  end,
  options,
  transform,
) {
  try {
    if (options.signal && options.signal.aborted) {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      throw error;
    }
    for (const batch of batches) {
      const result = transform ? transform(batch) : batch;
      if (result && typeof result[Symbol.asyncIterator] === "function") {
        for await (const value of result) yield value;
      } else {
        yield result;
      }
    }
    globalThis.__nodeFdPositions[handle.fd] = end;
    if (options.autoClose) await handle.close();
  } finally {
    handle._pullLocked = false;
  }
};
"#;
