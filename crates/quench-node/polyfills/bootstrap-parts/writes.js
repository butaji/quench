const __nodeFsCollectWriteIterable = (data, options) => {
  const chunks = [];
  for (const chunk of data) {
    if (
      typeof chunk !== "string" &&
      !(chunk instanceof Uint8Array) &&
      !ArrayBuffer.isView(chunk)
    ) {
      const error = new TypeError(
        'The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    chunks.push(chunk);
  }
  return chunks.length === 1
    ? chunks[0]
    : NodeBuffer.concat(
        chunks.map((chunk) =>
          typeof chunk === "string"
            ? NodeBuffer.from(
                chunk,
                typeof options === "string"
                  ? options
                  : options && options.encoding
              )
            : NodeBuffer.from(chunk)
        )
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
    const error = new TypeError(
      'The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
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
    typeof options === "string" ? { encoding: options } : options
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
    const error = new TypeError(
      'The "options" argument must be of type object or string'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const named =
    offset &&
    typeof offset === "object" &&
    !ArrayBuffer.isView(offset) &&
    !(offset instanceof ArrayBuffer);
  const source = named ? buffer : buffer;
  const start = named
    ? offset.offset === undefined
      ? 0
      : offset.offset
    : offset || 0;
  const size = named
    ? offset.length === undefined
      ? source.length - start
      : offset.length
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
    const error = new RangeError("The value is out of range");
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
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
    const error = new TypeError(
      'The "buffer" argument must be an instance of Buffer, TypedArray, or DataView'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const { source, start, size, at } = __nodeFsWriteArguments(
    buffer,
    offset,
    length,
    position
  );
  return {
    bytesWritten: globalThis.__nodeFs.writeSync(
      handle.fd,
      source,
      start,
      size,
      at === undefined ? null : at
    ),
    buffer: source
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
    buffer =
      options.buffer === undefined ? NodeBuffer.alloc(16384) : options.buffer;
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
    position
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
      buffers
    }));
  handle.writev = (buffers, position) =>
    Promise.resolve().then(() => ({
      bytesWritten: globalThis.__nodeFs.writevSync(
        handle.fd,
        buffers,
        position
      ),
      buffers
    }));
};
const __nodeFsAttachHandleOperations = (handle) => {
  handle.createReadStream = (options = {}) =>
    globalThis.__nodeFs.createReadStream(null, {
      ...options,
      fd: handle,
      autoClose: false
    });
  handle.createWriteStream = (options = {}) =>
    globalThis.__nodeFs.createWriteStream(null, {
      ...options,
      fd: handle,
      autoClose: false
    });
  handle.truncate = (length = 0) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.ftruncateSync(handle.fd, length)
    );
  handle.stat = () =>
    Promise.resolve().then(() => {
      if (handle.fd === -1) {
        const error = new Error("EBADF: bad file descriptor, fstat");
        error.code = "EBADF";
        error.syscall = "fstat";
        throw error;
      }
      return globalThis.__nodeFs.fstatSync(handle.fd);
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
      typeof options === "string" ? { encoding: options } : options
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
    typeof options === "string" ? { encoding: options } : options
  );
};
const __nodeOpenWithFilePosition = globalThis.__nodeFs.promises.open;
const __nodeFsValidatePullOptions = (options) => {
  if (
    options.autoClose !== undefined &&
    typeof options.autoClose !== "boolean"
  ) {
    const error = new TypeError(
      'The "autoClose" option must be of type boolean'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    options.signal !== undefined &&
    (!options.signal || typeof options.signal.aborted !== "boolean")
  ) {
    const error = new TypeError('The "signal" option must be an AbortSignal');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  for (const [name, value] of [
    ["start", options.start],
    ["limit", options.limit],
    ["chunkSize", options.chunkSize]
  ]) {
    if (value === undefined) continue;
    __nodeFsValidatePullNumber(name, value);
  }
};
const __nodeFsValidatePullNumber = (name, value) => {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    const error = new TypeError(`The "${name}" option must be of type number`);
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!Number.isInteger(value) || value < 0) {
    const error = new RangeError(`The value of "${name}" is out of range`);
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
const __nodeFsPullBatches = (handle, options) => {
  const source = globalThis.__nodeFs.readFileSync(handle.fd);
  const start =
    options.start === undefined
      ? globalThis.__nodeFdPositions[handle.fd] || 0
      : Number(options.start);
  const end =
    options.limit === undefined
      ? source.length
      : Math.min(source.length, start + Number(options.limit));
  const chunkSize =
    options.chunkSize === undefined ? 128 * 1024 : Number(options.chunkSize);
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
  transform
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
const __nodeFsAttachPull = (handle) => {
  handle.pull = (transformOrOptions, maybeOptions) => {
    if (!globalThis.__nodeFdPaths[handle.fd] || handle._pullLocked) {
      const error = new Error("The file handle is not in a valid state");
      error.code = "ERR_INVALID_STATE";
      throw error;
    }
    const transform =
      typeof transformOrOptions === "function" ? transformOrOptions : undefined;
    const options = transform ? maybeOptions || {} : transformOrOptions || {};
    __nodeFsValidatePullOptions(options);
    handle._pullLocked = true;
    const { batches, end } = __nodeFsPullBatches(handle, options);
    return {
      [Symbol.asyncIterator]: () =>
        __nodeFsPullIterator(handle, batches, end, options, transform)
    };
  };
};
const __nodeFsAttachPositionedWrites = (handle, previousWriteFile) => {
  handle.writeFile = async (data, options) => {
    if (options?.signal) {
      await new Promise((resolve) => queueMicrotask(resolve));
    }
    if (options?.signal?.aborted) {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      throw error;
    }
    if (typeof data === "string") {
      data = NodeBuffer.from(
        data,
        options && options.encoding ? options.encoding : "utf8"
      );
    }
    if (data instanceof ArrayBuffer) data = new Uint8Array(data);
    if (data instanceof Uint8Array || ArrayBuffer.isView(data)) {
      const view =
        data instanceof Uint8Array
          ? data
          : new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      globalThis.__nodeFs.writeSync(handle.fd, view, 0, view.length, null);
      return;
    }
    return previousWriteFile(data, options);
  };
};
const __nodeFsAttachPositionedRead = (handle, previousReadFile) => {
  handle.readFile = async (options) => {
    if (options && options.signal) {
      await new Promise((resolve) => queueMicrotask(resolve));
    }
    if (options && options.signal && options.signal.aborted) {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      throw error;
    }
    const source = globalThis.__nodeFs.readFileSync(handle.fd);
    const position = globalThis.__nodeFdPositions[handle.fd] || 0;
    globalThis.__nodeFdPositions[handle.fd] = source.length;
    const result = NodeBuffer.from(source.subarray(position));
    const encoding =
      typeof options === "string" ? options : options && options.encoding;
    return encoding ? result.toString(encoding) : result;
  };
};
globalThis.__nodeFs.promises.open = async (...args) => {
  const handle = await __nodeOpenWithFilePosition(...args);
  __nodeFsAttachPull(handle);
  __nodeFsAttachPositionedWrites(handle, handle.writeFile);
  __nodeFsAttachPositionedRead(handle, handle.readFile);
  return handle;
};
let __nodePriority = 0;
const __nodeValidatePriorityPid = (pid) => {
  if (pid === undefined) return;
  if (typeof pid !== "number") {
    const error = new TypeError('The "pid" argument must be of type number.');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (pid === -1) {
    const error = new Error(
      "A system error occurred: uv_os_getpriority returned ESRCH"
    );
    error.code = "ERR_SYSTEM_ERROR";
    error.name = "SystemError";
    throw error;
  }
  if (!Number.isSafeInteger(pid) || pid < 0 || pid > 2 ** 32 - 1) {
    const error = new RangeError('The value of "pid" is out of range.');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  if (pid === 0 || pid === globalThis.process.pid) return;
  const error = new Error(
    "A system error occurred: uv_os_getpriority returned ESRCH"
  );
  error.code = "ERR_SYSTEM_ERROR";
  throw error;
};
const __nodeValidatePriorityValue = (priority) => {
  if (typeof priority !== "number") {
    const error = new TypeError(
      'The "priority" argument must be of type number.'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!Number.isSafeInteger(priority) || priority < -20 || priority > 19) {
    const error = new RangeError('The value of "priority" is out of range.');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
const __nodeStartedAt = Date.now();
const __nodeOsExports = {
  EOL: "\n",
  devNull: "/dev/null",
  platform: () => process.platform,
  arch: () => process.arch,
  release: () => "quench-node",
  version: () => "v0.1.0",
  machine: () => process.arch,
  tmpdir: () => {
    const candidate =
      process.env.TMPDIR || process.env.TMP || process.env.TEMP || "/tmp";
    return candidate.length > 1 ? candidate.replace(/\/+$/, "") : candidate;
  },
  homedir: () => globalThis.__quenchOsHomeDirectory(),
  type: () => "Quench",
  endianness: () => "LE",
  hostname: () => globalThis.__quench_hostname,
  loadavg: () => [0, 0, 0],
  freemem: () => 1,
  totalmem: () => 1,
  networkInterfaces: () => ({
    lo: [
      {
        address: "127.0.0.1",
        netmask: "255.0.0.0",
        family: "IPv4",
        mac: "00:00:00:00:00:00",
        internal: true,
        cidr: "127.0.0.1/8"
      }
    ]
  }),
  uptime: () => Math.max(0.001, (Date.now() - __nodeStartedAt) / 1000),
  getPriority: (pid) => {
    __nodeValidatePriorityPid(pid);
    return __nodePriority;
  },
  setPriority: (pid, priority) => {
    const value = priority === undefined ? pid : priority;
    __nodeValidatePriorityPid(priority === undefined ? undefined : pid);
    __nodeValidatePriorityValue(value);
    __nodePriority = value;
  },
  cpus: () =>
    Array.from({ length: globalThis.__quench_cpu_count }, () => ({
      model: "unknown",
      speed: 0,
      times: { user: 0, nice: 0, sys: 0, idle: 0, irq: 0 }
    })),
  availableParallelism: () => globalThis.__quench_cpu_count,
  userInfo: (options = {}) => {
    const value = {
      username: "quench",
      uid: 0,
      gid: 0,
      shell: "/bin/sh",
      homedir: globalThis.__quench_homedir || "/"
    };
    if (options.encoding === "buffer") {
      return Object.fromEntries(
        Object.entries(value).map(([key, item]) => [
          key,
          typeof item === "string" ? NodeBuffer.from(item) : item
        ])
      );
    }
    return value;
  },
  constants: Object.freeze({
    signals: Object.freeze({
      SIGHUP: 1,
      SIGINT: 2,
      SIGQUIT: 3,
      SIGILL: 4,
      SIGTRAP: 5,
      SIGABRT: 6,
      SIGIOT: 6,
      SIGBUS: 10,
      SIGFPE: 8,
      SIGKILL: 9,
      SIGUSR1: 30,
      SIGSEGV: 11,
      SIGUSR2: 31,
      SIGPIPE: 13,
      SIGALRM: 14,
      SIGTERM: 15,
      SIGCHLD: 20,
      SIGCONT: 19,
      SIGSTOP: 17,
      SIGTSTP: 18,
      SIGTTIN: 21,
      SIGTTOU: 22,
      SIGURG: 16,
      SIGXCPU: 24,
      SIGXFSZ: 25,
      SIGVTALRM: 26,
      SIGPROF: 27,
      SIGWINCH: 28,
      SIGIO: 23,
      SIGINFO: 29,
      SIGSYS: 12
    }),
    errno: Object.freeze({ ENOENT: 2, EACCES: 13 }),
    priority: Object.freeze({
      PRIORITY_LOW: 19,
      PRIORITY_BELOW_NORMAL: 10,
      PRIORITY_NORMAL: 0,
      PRIORITY_ABOVE_NORMAL: -7,
      PRIORITY_HIGH: -14,
      PRIORITY_HIGHEST: -20
    })
  })
};
globalThis.__nodeOs = __nodeOsExports;
for (const [name, getter] of [
  ["uptime", () => globalThis.__nodeOs.uptime()],
  ["availableParallelism", () => globalThis.__nodeOs.availableParallelism()],
  ["freemem", () => globalThis.__nodeOs.freemem()],
  ["totalmem", () => globalThis.__nodeOs.totalmem()]
]) {
  globalThis.__nodeOs[name].valueOf = getter;
}
for (const name of [
  "hostname",
  "homedir",
  "release",
  "type",
  "endianness",
  "tmpdir",
  "arch",
  "platform",
  "version",
  "machine"
]) {
  globalThis.__nodeOs[name].toString = () =>
    String(globalThis.__nodeOs[name]());
}
globalThis.__nodeOsInitialized = false;
globalThis.__nodeOs = new Proxy(
  {},
  {
    get: (target, key) => {
      globalThis.__nodeOsInitialized = true;
      return Reflect.has(target, key) ? target[key] : __nodeOsExports[key];
    },
    ownKeys: () => Reflect.ownKeys(__nodeOsExports),
    getOwnPropertyDescriptor: (_, key) => ({
      enumerable: true,
      configurable: true,
      value: __nodeOsExports[key]
    })
  }
);
const __nodePerformanceMarks = new Map();
const __nodePerformanceEntries = [];
