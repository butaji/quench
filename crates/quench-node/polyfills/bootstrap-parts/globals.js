globalThis.global = globalThis;
if (typeof Array.fromAsync !== "function") {
  Array.fromAsync = async function (source, mapFn) {
    const result = [];
    let index = 0;
    if (source != null && source[Symbol.asyncIterator]) {
      for await (const value of source) {
        result.push(mapFn ? await mapFn(value, index++) : value);
      }
    } else {
      for (const value of source || []) {
        result.push(mapFn ? await mapFn(value, index++) : value);
      }
    }
    return result;
  };
}
if (typeof Promise.withResolvers !== "function") {
  Promise.withResolvers = function () {
    let resolve;
    let reject;
    const promise = new Promise((res, rej) => {
      resolve = res;
      reject = rej;
    });
    return { promise, resolve, reject };
  };
}
const __nodeNativeEval = globalThis.eval;
globalThis.eval = (source) => {
  if (typeof source === "string" && source.trimStart().startsWith("%")) {
    return undefined;
  }
  return __nodeNativeEval(source);
};
const __nodeDetachedBuffers = new WeakSet();
const __nodeImmutableBuffers = new WeakSet();
const __nodeAllocatorCounts = {
  uninitialized: 0,
  zeroFilled: 0
};
const __nodeNativeStructuredClone = globalThis.structuredClone;
globalThis.structuredClone = (value, options) => {
  for (const item of options && options.transfer ? options.transfer : []) {
    if (item instanceof ArrayBuffer) __nodeDetachedBuffers.add(item);
  }
  if (globalThis.process && value === globalThis.process.env) {
    const clone = {};
    for (const key of Object.keys(value)) clone[key] = value[key];
    return clone;
  }
  const cryptoKeyClone = globalThis.__quenchCloneWebCryptoKey?.(value);
  if (cryptoKeyClone) return cryptoKeyClone;
  return __nodeNativeStructuredClone
    ? __nodeNativeStructuredClone(value, options)
    : value;
};
if (typeof ArrayBuffer.prototype.transferToImmutable !== "function") {
  ArrayBuffer.prototype.transferToImmutable = function () {
    __nodeImmutableBuffers.add(this);
    return this;
  };
}
const __nodeProxySet = new WeakSet();
const __nodeModuleNamespaces = new WeakSet();
const __nodeNativeProxy = globalThis.Proxy;
globalThis.Proxy = function (target, handlers) {
  const proxy = new __nodeNativeProxy(target, handlers);
  __nodeProxySet.add(proxy);
  return proxy;
};
const __nodeDataViewSet = new WeakSet();
const __nodeNativeDataView = globalThis.DataView;
globalThis.DataView = function (...args) {
  const view = new __nodeNativeDataView(...args);
  __nodeDataViewSet.add(view);
  return view;
};
globalThis.DataView.prototype = __nodeNativeDataView.prototype;
const __nodeTypedArraySets = {};
for (const name of [
  "Uint8Array",
  "Uint8ClampedArray",
  "Int8Array",
  "Uint16Array",
  "Int16Array",
  "Uint32Array",
  "Int32Array",
  "Float16Array",
  "Float32Array",
  "Float64Array",
  "BigInt64Array",
  "BigUint64Array"
]) {
  const Native = globalThis[name];
  const set = new WeakSet();
  __nodeTypedArraySets[name] = set;
  const Wrapped = function (...args) {
    const array = Reflect.construct(Native, args, new.target || Wrapped);
    set.add(array);
    return array;
  };
  Wrapped.prototype = Native.prototype;
  Object.setPrototypeOf(Wrapped, Native);
  globalThis[name] = Wrapped;
}
const __quenchQueueMicrotask = globalThis.queueMicrotask;
globalThis.__quench_async_error = "";
globalThis.queueMicrotask = (callback) =>
  __quenchQueueMicrotask(() => {
    try {
      callback();
    } catch (error) {
      if (!globalThis.__quench_async_error) {
        globalThis.__quench_async_error =
          error && error.stack
            ? `${error.name}: ${error.message}\n${error.stack}`
            : String(error);
      }
    }
  });
globalThis.__nodeFormat = (args) =>
  args
    .map((value) => {
      try {
        return typeof value === "string" ? value : JSON.stringify(value);
      } catch (_) {
        return String(value);
      }
    })
    .join(" ");
globalThis.console = globalThis.console || {};
for (const method of ["log", "info", "warn", "error", "debug"]) {
  globalThis.console[method] = (...args) => {
    const line = globalThis.__nodeFormat(args);
    if (
      globalThis.process &&
      globalThis.process.stdout &&
      globalThis.process.stdout.write
    ) {
      globalThis.process.stdout.write(line + "\n");
    } else globalThis.__quench_console_write(line);
  };
}
globalThis.console.dir = (value) => {
  const line = globalThis.__nodeFormat([value]);
  if (
    globalThis.process &&
    globalThis.process.stdout &&
    globalThis.process.stdout.write
  ) {
    globalThis.process.stdout.write(line + "\n");
  } else globalThis.__quench_console_write(line);
};
globalThis.console.assert = (condition, ...args) => {
  if (!condition) globalThis.console.error(...args);
};
const consoleTimers = {};
const consoleCounts = {};
globalThis.console.count = (label = "default") => {
  if (typeof label === "symbol" || typeof label === "Symbol") {
    const e = new TypeError("Count label must be a string");
    e.code = "ERR_INVALID_ARG_TYPE";
    throw e;
  }
  consoleCounts[label] = (consoleCounts[label] || 0) + 1;
  const line = `${label}: ${consoleCounts[label]}`;
  if (
    globalThis.process &&
    globalThis.process.stdout &&
    globalThis.process.stdout.write
  ) {
    globalThis.process.stdout.write(line + "\n");
  } else globalThis.__quench_console_write(line);
};
globalThis.console.countReset = (label = "default") => {
  consoleCounts[label] = 0;
};
globalThis.console.clear = () => undefined;
globalThis.console.time = (label = "default") => {
  consoleTimers[label] = BigInt(globalThis.__quench_now_ns());
};
globalThis.console.timeLog = (label = "default", ...args) => {
  if (consoleTimers[label] === undefined) return;
  globalThis.__quench_console_write(
    `${label}: ${
      Number(BigInt(globalThis.__quench_now_ns()) - consoleTimers[label]) / 1e6
    } ms ${globalThis.__nodeFormat(args)}`
  );
};
globalThis.console.timeEnd = (label = "default") => {
  if (consoleTimers[label] === undefined) return;
  globalThis.__quench_console_write(
    `${label}: ${
      Number(BigInt(globalThis.__quench_now_ns()) - consoleTimers[label]) / 1e6
    } ms`
  );
  delete consoleTimers[label];
};
globalThis.__quench_node_pids = new Set([globalThis.__quench_pid]);
globalThis.DOMException = class DOMException extends Error {
  constructor(message = "", name = "Error") {
    super(message);
    this.name = name;
  }
};
const __nodeClusterKill = (cluster, pid, signal) => {
  if (!cluster || !Number.isInteger(pid)) return false;
  for (const worker of cluster.workers) {
    if (worker.process && worker.process.pid === pid) {
      worker.kill(signal);
      return true;
    }
  }
  return false;
};
const __nodeProcessKill = (pid, signal) => {
  if (typeof pid === "object" && pid !== null) {
    signal = pid.signal;
    pid = undefined;
  }
  const cluster = globalThis.__nodeCluster;
  if (__nodeClusterKill(cluster, pid, signal)) return true;
  return globalThis.__quench_kill?.(pid, String(signal || "SIGTERM")) ?? true;
};
const __quenchOriginalCwdGet = globalThis.__quench_cwd_get;
const __quenchDisplayCwd = () => __quenchOriginalCwdGet();
globalThis.__quench_cwd_get = __quenchDisplayCwd;
globalThis.process = {
  env: new Proxy(
    {},
    {
      get: (target, key) => {
        if (typeof key !== "string") return Reflect.get(target, key);
        const value = globalThis.__quench_env_get(key);
        return value === undefined ? Reflect.get(target, key) : value;
      },
      set: (_, key, value) => {
        if (typeof key === "symbol" || typeof value === "symbol") {
          throw new TypeError("Cannot convert a Symbol value to a string");
        }
        if (String(key) === "") return true;
        globalThis.__quench_env_set(String(key), String(value));
        globalThis.__quench_env_keys = [
          ...new Set([...globalThis.__quench_env_keys, String(key)])
        ];
        return true;
      },
      deleteProperty: (_, key) => {
        globalThis.__quench_env_delete(String(key));
        globalThis.__quench_env_keys = globalThis.__quench_env_keys.filter(
          (item) => item !== String(key)
        );
        return true;
      },
      defineProperty: (_, key, descriptor) => {
        if (typeof key === "symbol") return true;
        if (descriptor.get || descriptor.set) {
          const error = new TypeError(
            "'process.env' does not accept an accessor(getter/setter) descriptor"
          );
          error.code = "ERR_INVALID_OBJECT_DEFINE_PROPERTY";
          throw error;
        }
        if (
          descriptor.configurable !== true ||
          descriptor.writable !== true ||
          descriptor.enumerable !== true
        ) {
          const error = new TypeError(
            "'process.env' only accepts a configurable, writable, and enumerable data descriptor"
          );
          error.code = "ERR_INVALID_OBJECT_DEFINE_PROPERTY";
          throw error;
        }
        globalThis.__quench_env_set(String(key), String(descriptor.value));
        return true;
      },
      has: (_, key) =>
        typeof key === "string" &&
        globalThis.__quench_env_get(key) !== undefined,
      ownKeys: () => globalThis.__quench_env_keys,
      getOwnPropertyDescriptor: (_, key) => {
        const value = globalThis.__quench_env_get(String(key));
        return value === undefined
          ? undefined
          : { enumerable: true, configurable: true, value };
      }
    }
  ),
  argv: ["quench-node", ...globalThis.__quench_argv.slice(1)],
  execPath: globalThis.__quench_exec_path,
  pid: globalThis.__quench_pid,
  ppid: globalThis.__quench_ppid,
  getuid: () => globalThis.__quench_getuid,
  geteuid: () => globalThis.__quench_geteuid,
  getgid: () => globalThis.__quench_getgid,
  getegid: () => globalThis.__quench_getegid,
  platform:
    globalThis.__quench_platform === "macos"
      ? "darwin"
      : globalThis.__quench_platform,
  arch:
    globalThis.__quench_arch === "aarch64" ? "arm64" : globalThis.__quench_arch,
  version: "v20.0.0",
  versions: { node: "20.0.0", v8: "0.0.0-quench", uv: "0.0.0" },
  release: { name: "node", lts: "Quench" },
  config: {
    variables: {
      node_module_version: 127,
      v8_enable_i18n_support: false,
      v8_enable_temporal_support: false,
      node_shared: false,
      node_use_ffi: false
    }
  },
  features: { inspector: false, tls: false, quic: false, dtls: false },
  cwd: __quenchDisplayCwd,
  chdir: (value) => {
    if (typeof value !== "string") {
      const error = new TypeError(
        `The "directory" argument must be of type string. Received ${typeof value}`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const path = String(value);
    if (!globalThis.__quench_fs_exists(path)) {
      const error = new Error(
        `ENOENT: no such file or directory, chdir '${process.cwd()}' -> '${path}'`
      );
      Object.assign(error, {
        code: "ENOENT",
        path: process.cwd(),
        syscall: "chdir",
        dest: path
      });
      throw error;
    }
    return globalThis.__quench_chdir(path);
  },
  exitCode: 0,
  exit: (code) => {
    process.exitCode = code;
    process.__quench_force_exit = true;
    globalThis.__quench_force_exit = true;
    throw { __quench_process_exit: true };
  },
  kill: __nodeProcessKill,
  platform:
    globalThis.__quench_platform === "macos"
      ? "darwin"
      : globalThis.__quench_platform,
  arch:
    globalThis.__quench_arch === "aarch64" ? "arm64" : globalThis.__quench_arch,
  uptime: () => Math.max(0, (Date.now() - __nodeStartedAt) / 1000),
  memoryUsage: () => ({
    rss: 0,
    heapTotal: 0,
    heapUsed: 0,
    external: 0,
    arrayBuffers: 0
  }),
  resourceUsage: () => ({
    userCPUTime: 0,
    systemCPUTime: 0,
    maxRSS: 0,
    sharedMemorySize: 0,
    unsharedDataSize: 0,
    unsharedStackSize: 0,
    minorPageFault: 0,
    majorPageFault: 0,
    swappedOut: 0,
    fsRead: 0,
    fsWrite: 0,
    ipcSent: 0,
    ipcReceived: 0,
    signalsCount: 0,
    voluntaryContextSwitches: 0,
    involuntaryContextSwitches: 0
  }),
  binding: (name) => {
    const error = new Error(`No such module: ${String(name)}`);
    error.code = "ERR_UNKNOWN_BUILTIN_MODULE";
    throw error;
  },
  getBuiltinModule: (name) => {
    try {
      return globalThis.require(String(name));
    } catch (_) {
      return undefined;
    }
  },
  umask: (mask) =>
    globalThis.__quench_umask(mask === undefined ? undefined : Number(mask)),
  nextTick: (callback, ...args) => {
    if (typeof callback !== "function") {
      const error = new TypeError(
        'The "callback" argument must be of type function'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const activeDomain = globalThis.__quench_active_domain;
    queueMicrotask(() => {
      if (activeDomain) activeDomain.run(callback, ...args);
      else callback(...args);
    });
  },
  send: (...values) => {
    const callback = values.at(-1);
    const hasCallback = typeof callback === "function";
    const message = hasCallback ? values.slice(0, -1) : values;
    const cluster = globalThis.__nodeCluster;
    if (cluster && cluster.isWorker && cluster.worker) {
      queueMicrotask(() => {
        for (const value of message) cluster.worker.emit("message", value);
      });
      return true;
    }
    return false;
  },
  hrtime: (previous) => {
    const ns = BigInt(globalThis.__quench_now_ns());
    const current = [Number(ns / 1000000000n), Number(ns % 1000000000n)];
    if (previous === undefined) return current;
    if (!Array.isArray(previous)) {
      const error = new TypeError(
        `The "time" argument must be an instance of Array. Received type ${typeof previous} (${String(
          previous
        )})`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (previous.length !== 2) {
      const error = new RangeError(
        `The value of "time" is out of range. It must be 2. Received ${previous.length}`
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    let seconds = current[0] - previous[0];
    let nanos = current[1] - previous[1];
    if (nanos < 0) {
      seconds--;
      nanos += 1000000000;
    }
    return [seconds, nanos];
  }
};
process.hrtime.bigint = () => BigInt(globalThis.__quench_now_ns());
globalThis.setImmediate = (callback, ...args) => {
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const resource = {
    asyncId: ++globalThis.__nodeNextAsyncId,
    triggerAsyncId: globalThis.__nodeCurrentAsyncResource?.asyncId || 1
  };
  for (const hook of globalThis.__nodeAsyncHooks || []) {
    if (typeof hook.callbacks?.init === "function") {
      hook.callbacks.init(
        resource.asyncId,
        "Immediate",
        resource.triggerAsyncId,
        resource
      );
    }
  }
  const id = {
    active: true,
    refed: true,
    generation: 0,
    __immediate: true,
    _destroyed: false
  };
  const activeDomain = globalThis.__quench_active_domain;
  id.ref = () => {
    if (!id.refed && id.active && !id.counted) {
      id.refed = true;
      id.counted = true;
      globalThis.__quenchRefedHandles++;
    }
    return id;
  };
  id.unref = () => ((id.refed = false), id);
  id.hasRef = () => id.active && id.refed;
  id.refresh = () => ((id.active = true), id);
  Symbol.dispose ||= Symbol("dispose");
  id[Symbol.dispose] = () => {
    id.active = false;
    id._destroyed = true;
  };
  queueMicrotask(() => {
    if (id.active) {
      if (activeDomain) activeDomain.run(callback, ...args);
      else callback(...args);
    }
  });
  return id;
};
globalThis.clearImmediate = (id) => {
  if (id?.__immediate) {
    id.active = false;
    id._destroyed = true;
  }
};
globalThis.__quenchRefedHandles ||= 0;
globalThis.__quenchTimerHandleIds ||= new Map();
globalThis.__quenchNextTimerHandleId ||= 1;
globalThis.setTimeout = (callback, _delay = 0, ...args) => {
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const id = {
    active: true,
    refed: true,
    generation: 0,
    counted: true,
    unrefChecks: 0,
    _destroyed: false
  };
  const handleId = globalThis.__quenchNextTimerHandleId++;
  globalThis.__quenchTimerHandleIds.set(handleId, id);
  id[Symbol.toPrimitive] = () => handleId;
  Symbol.dispose ||= Symbol("dispose");
  id[Symbol.dispose] = () => globalThis.clearTimeout(id);
  globalThis.__quenchRefedHandles++;
  id.ref = () => {
    if (!id.refed && id.active && !id.counted) {
      id.refed = true;
      id.counted = true;
      globalThis.__quenchRefedHandles++;
    }
    return id;
  };
  id.unref = () => {
    if (id.refed && id.counted) {
      id.refed = false;
      id.counted = false;
      globalThis.__quenchRefedHandles = Math.max(
        0,
        globalThis.__quenchRefedHandles - 1
      );
    }
    return id;
  };
  id.hasRef = () => id.active && id.refed;
  const resource = globalThis.__nodeCurrentAsyncResource;
  const activeDomain = globalThis.__quench_active_domain;
  const schedule = () => {
    const generation = ++id.generation;
    queueMicrotask(() => {
      if (id.active && generation === id.generation) {
        if (
          !id.refed &&
          globalThis.__quenchRefedHandles > 0 &&
          id.unrefChecks++ < 1000
        ) {
          queueMicrotask(schedule);
          return;
        }
        if (!id.refed && globalThis.__quenchRefedHandles === 0) {
          id.active = false;
          return;
        }
        if (id.counted) {
          id.counted = false;
          globalThis.__quenchRefedHandles = Math.max(
            0,
            globalThis.__quenchRefedHandles - 1
          );
        }
        const delay = __nodeTimerDelay(_delay);
        if (delay) globalThis.__quench_sleep_ms(delay);
        const previous = globalThis.__nodeCurrentAsyncResource;
        globalThis.__nodeCurrentAsyncResource = resource;
        try {
          if (activeDomain) activeDomain.run(callback, ...args);
          else callback(...args);
        } finally {
          globalThis.__nodeCurrentAsyncResource = previous;
        }
      }
    });
  };
  id.refresh = () => {
    id.active = true;
    schedule();
    return id;
  };
  schedule();
  return id;
};
globalThis.clearTimeout = (id) => {
  if (typeof id === "number" || typeof id === "string") {
    const numericId = Number(id);
    if (Number.isInteger(numericId)) {
      id = globalThis.__quenchTimerHandleIds.get(numericId);
    }
  }
  if (id) {
    if (id.active && id.counted) {
      id.counted = false;
      globalThis.__quenchRefedHandles = Math.max(
        0,
        globalThis.__quenchRefedHandles - 1
      );
    }
    id.active = false;
    id._destroyed = true;
  }
};
globalThis.setInterval = (callback, _delay = 0, ...args) => {
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const id = { active: true, refed: true, generation: 0, _destroyed: false };
  const handleId = globalThis.__quenchNextTimerHandleId++;
  globalThis.__quenchTimerHandleIds.set(handleId, id);
  id[Symbol.toPrimitive] = () => handleId;
  Symbol.dispose ||= Symbol("dispose");
  id[Symbol.dispose] = () => globalThis.clearInterval(id);
  id.ref = () => ((id.refed = true), id);
  id.unref = () => ((id.refed = false), id);
  id.hasRef = () => id.active && id.refed;
  const activeDomain = globalThis.__quench_active_domain;
  const schedule = () => {
    const generation = ++id.generation;
    queueMicrotask(() => {
      if (!id.active || generation !== id.generation) return;
      const delay = __nodeTimerDelay(_delay);
      if (delay) globalThis.__quench_sleep_ms(delay);
      if (activeDomain) activeDomain.run(() => callback.apply(id, args));
      else callback.apply(id, args);
      if (id.active) schedule();
    });
  };
  id.refresh = () => {
    id.active = true;
    schedule();
    return id;
  };
  Symbol.dispose ||= Symbol("dispose");
  id[Symbol.dispose] = () => globalThis.clearInterval(id);
  schedule();
  return id;
};
globalThis.clearInterval = globalThis.clearTimeout;
globalThis.__nodeTimers = {
  setTimeout,
  clearTimeout,
  setInterval,
  clearInterval,
  setImmediate,
  clearImmediate
};
if (typeof Object.hasOwn !== "function") {
  Object.defineProperty(Object, "hasOwn", {
    value: (object, property) =>
      Object.prototype.hasOwnProperty.call(object, property),
    configurable: true,
    writable: true
  });
}
if (typeof globalThis.Blob !== "function") {
  globalThis.Blob = class Blob {
    constructor(parts = [], options = {}) {
      if (!Array.isArray(parts)) {
        const error = new TypeError(
          'The "sources" argument must be an instance of Array'
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const chunks = parts.map((part) =>
        typeof part === "string" ||
        ArrayBuffer.isView(part) ||
        part instanceof ArrayBuffer
          ? NodeBuffer.from(part)
          : part instanceof Blob
            ? NodeBuffer.from(part._data)
            : (() => {
                const error = new TypeError(
                  "The sources argument contains an invalid part"
                );
                error.code = "ERR_INVALID_ARG_TYPE";
                throw error;
              })()
      );
      this._data = NodeBuffer.concat(chunks);
      this.size = this._data.length;
      this.type = String(options.type || "").toLowerCase();
    }
    async arrayBuffer() {
      return this._data.buffer.slice(
        this._data.byteOffset,
        this._data.byteOffset + this._data.byteLength
      );
    }
    async text() {
      return this._data.toString();
    }
    slice(start = 0, end = this.size, type = "") {
      return new Blob([this._data.subarray(start, end)], { type });
    }
  };
}
if (globalThis.Blob && !globalThis.Blob.__quenchTypeNormalized) {
  const __quenchNativeBlob = globalThis.Blob;
  const __quenchBlob = function Blob(parts = [], options = {}) {
    const value = new __quenchNativeBlob(parts, options);
    if (options && options.type !== undefined) {
      Object.defineProperty(value, "type", {
        configurable: true,
        value: String(options.type).toLowerCase()
      });
    }
    return value;
  };
  __quenchBlob.prototype = __quenchNativeBlob.prototype;
  Object.defineProperty(__quenchBlob, "__quenchTypeNormalized", {
    value: true
  });
  globalThis.Blob = __quenchBlob;
}
if (globalThis.process && typeof globalThis.process.emit !== "function") {
  globalThis.process.emit = () => globalThis.process;
}
