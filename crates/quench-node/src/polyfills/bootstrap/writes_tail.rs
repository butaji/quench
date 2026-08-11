//! Polyfill: `writes-tail`

pub const JS: &str = quench_js_check::checked_js!(r#"const __nodeFsAttachPull = (handle) => {
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
  handle.pullSync = (transformOrOptions, maybeOptions) => {
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
      *[Symbol.iterator]() {
        try {
          if (options.signal?.aborted) {
            const error = new Error("The operation was aborted");
            error.name = "AbortError";
            throw error;
          }
          for (const batch of batches) {
            const result = transform ? transform(batch) : batch;
            if (result !== null && result !== undefined) yield result;
          }
          globalThis.__nodeFdPositions[handle.fd] = end;
          if (options.autoClose) globalThis.__nodeFs.closeSync(handle.fd);
        } finally {
          if (options.autoClose && globalThis.__nodeFdPaths[handle.fd]) {
            try {
              globalThis.__nodeFs.closeSync(handle.fd);
            } catch (_) {}
          }
          handle._pullLocked = false;
        }
      }
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
    throw Object.assign(new TypeError('The "pid" argument must be of type number.'), { code: "ERR_INVALID_ARG_TYPE" });
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
    throw Object.assign(new RangeError('The value of "pid" is out of range.'), { code: "ERR_OUT_OF_RANGE" });
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
    throw Object.assign(new TypeError('The "priority" argument must be of type number.'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!Number.isSafeInteger(priority) || priority < -20 || priority > 19) {
    throw Object.assign(new RangeError('The value of "priority" is out of range.'), { code: "ERR_OUT_OF_RANGE" });
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
for (const name of "hostname homedir release type endianness tmpdir arch platform version machine".split(
  " "
)) {
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
"#);
