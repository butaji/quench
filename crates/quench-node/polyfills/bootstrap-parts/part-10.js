globalThis.__nodeFs.promises.open = async (...args) => {
  const handle = await __nodePromiseOpen(...args);
  handle.write = (buffer, offset, length, position) =>
    Promise.resolve().then(() => {
      const start =
        typeof offset === "object" ? offset.offset || 0 : offset || 0;
      const source =
        typeof offset === "object" ? offset.buffer || offset : buffer;
      const size =
        typeof offset === "object"
          ? offset.length === undefined
            ? source.length - start
            : offset.length
          : length === undefined
            ? source.length - start
            : length;
      const at = typeof offset === "object" ? offset.position : position;
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
    });
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
  handle.truncate = (length = 0) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.ftruncateSync(handle.fd, length)
    );
  handle.stat = () =>
    Promise.resolve().then(() => globalThis.__nodeFs.fstatSync(handle.fd));
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
  handle.writeFile = async (data, options) => {
    if (options && options.signal)
      await new Promise((resolve) => queueMicrotask(resolve));
    if (options && options.signal && options.signal.aborted) {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      throw error;
    }
    if (data && data._chunks) data = data._chunks;
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
    if (
      data &&
      typeof data !== "string" &&
      !(data instanceof Uint8Array) &&
      !ArrayBuffer.isView(data) &&
      typeof data[Symbol.iterator] === "function"
    ) {
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
      data =
        chunks.length === 1
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
    }
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
    return globalThis.__nodeFs.writeFileSync(
      handle.fd,
      data,
      typeof options === "string" ? { encoding: options } : options
    );
  };
  handle.appendFile = (data, options) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.appendFileSync(handle.fd, data, options)
    );
  handle.close = () =>
    Promise.resolve().then(() => globalThis.__nodeFs.closeSync(handle.fd));
  return handle;
};
const __nodeOpenWithFilePosition = globalThis.__nodeFs.promises.open;
globalThis.__nodeFs.promises.open = async (...args) => {
  const handle = await __nodeOpenWithFilePosition(...args);
  const previousWriteFile = handle.writeFile;
  const previousReadFile = handle.readFile;
  handle.pull = (transformOrOptions, maybeOptions) => {
    if (!globalThis.__nodeFdPaths[handle.fd] || handle._pullLocked) {
      const error = new Error("The file handle is not in a valid state");
      error.code = "ERR_INVALID_STATE";
      throw error;
    }
    handle._pullLocked = true;
    const transform =
      typeof transformOrOptions === "function" ? transformOrOptions : undefined;
    const options = transform ? maybeOptions || {} : transformOrOptions || {};
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
      if (typeof value !== "number" || !Number.isFinite(value)) {
        const error = new TypeError(
          `The "${name}" option must be of type number`
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (!Number.isInteger(value) || value < 0) {
        const error = new RangeError(`The value of "${name}" is out of range`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
    }
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
    for (let offset = start; offset < end; offset += chunkSize)
      batches.push([
        source.subarray(offset, Math.min(end, offset + chunkSize))
      ]);
    if (start >= end) batches.push([]);
    return {
      async *[Symbol.asyncIterator]() {
        try {
          if (options.signal && options.signal.aborted) {
            const error = new Error("The operation was aborted");
            error.name = "AbortError";
            throw error;
          }
          for (const batch of batches)
            yield transform ? transform(batch) : batch;
          globalThis.__nodeFdPositions[handle.fd] = end;
          if (options.autoClose) await handle.close();
        } finally {
          handle._pullLocked = false;
        }
      }
    };
  };
  handle.writeFile = async (data, options) => {
    if (typeof data === "string")
      data = NodeBuffer.from(
        data,
        options && options.encoding ? options.encoding : "utf8"
      );
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
  handle.readFile = async (options) => {
    if (options && options.signal)
      await new Promise((resolve) => queueMicrotask(resolve));
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
  return handle;
};
let __nodePriority = 0;
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
  homedir: () => globalThis.__quench_homedir,
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
  getPriority: () => __nodePriority,
  setPriority: (pid, priority) => {
    __nodePriority = Number(priority === undefined ? pid : priority);
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
    if (options.encoding === "buffer")
      return Object.fromEntries(
        Object.entries(value).map(([key, item]) => [
          key,
          typeof item === "string" ? NodeBuffer.from(item) : item
        ])
      );
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
    errno: Object.freeze({ ENOENT: -2, EACCES: -13 }),
    priority: Object.freeze({
      PRIORITY_LOW: 19,
      PRIORITY_BELOW_NORMAL: 10,
      PRIORITY_NORMAL: 0,
      PRIORITY_ABOVE_NORMAL: -7,
      PRIORITY_HIGH: -14
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
    get: (_, key) => {
      globalThis.__nodeOsInitialized = true;
      return __nodeOsExports[key];
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
