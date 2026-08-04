const __quenchRequireStreamIter = () => {
  if (!globalThis.__quench_argv.includes("--experimental-stream-iter")) {
    const error = new Error("No such built-in module: node:stream/iter");
    error.code = "ERR_UNKNOWN_BUILTIN_MODULE";
    throw error;
  }
  return {
    text: async (readable) => {
      const chunks = [];
      for await (const batch of readable)
        for (const chunk of batch) chunks.push(chunk);
      return new TextDecoder().decode(NodeBuffer.concat(chunks));
    },
    bytes: async (readable) => {
      const chunks = [];
      for await (const batch of readable)
        for (const chunk of batch) chunks.push(chunk);
      return NodeBuffer.concat(chunks);
    },
    pull: (readable, transform) => ({
      async *[Symbol.asyncIterator]() {
        for await (const batch of readable)
          yield transform ? transform(batch) : batch;
      }
    })
  };
};
const __quenchFinishClusterWorker = (cluster, worker, workerError) => {
  queueMicrotask(() => {
    if (worker.state !== "online" && worker.state !== "listening") return;
    const exitCode =
      process.exitCode !== undefined && process.exitCode !== 0
        ? process.exitCode
        : workerError
          ? 1
          : 0;
    worker.process.exitCode = exitCode;
    worker.process.signalCode = null;
    worker._markDead();
    cluster.emit("disconnect", worker);
    worker.emit("disconnect");
    cluster.emit("exit", worker, exitCode, null);
    worker.emit("exit", exitCode, null);
  });
};
const __quenchRunClusterWorker = (cluster, worker, env, reentry) => {
  if (typeof globalThis.__quench_script_source !== "string" || reentry) return;
  const previousAsyncResource = globalThis.__nodeCurrentAsyncResource;
  const previousIsWorker = cluster.isWorker;
  const previousArgv = process.argv;
  for (const [key, value] of Object.entries(env)) {
    if (value !== undefined) process.env[key] = value;
  }
  const setupArgs = cluster.settings?.args || [];
  if (setupArgs.length) process.argv = [...process.argv, ...setupArgs];
  cluster.isWorker = true;
  cluster.worker = worker;
  globalThis.__quench_in_cluster_worker = true;
  globalThis.__nodeCurrentAsyncResource = { id: worker.id };
  let workerError = null;
  try {
    (0, globalThis.eval)(globalThis.__quench_script_source);
  } catch (error) {
    workerError = error;
  }
  cluster.isWorker = previousIsWorker;
  process.argv = previousArgv;
  globalThis.__nodeCurrentAsyncResource = previousAsyncResource;
  if (worker.state !== "dead")
    __quenchFinishClusterWorker(cluster, worker, workerError);
};
const __quenchVmCopyProperties = (sandbox, keys, originalGlobalKeys) => {
  for (const key of keys) {
    const descriptor = Object.getOwnPropertyDescriptor(sandbox, key);
    if (descriptor && "value" in descriptor && descriptor.writable !== false)
      sandbox[key] = globalThis[key];
  }
  for (const key of Object.getOwnPropertyNames(globalThis))
    if (
      !originalGlobalKeys.has(key) &&
      !keys.includes(key) &&
      key !== "globalThis"
    )
      sandbox[key] = globalThis[key];
};
const __quenchVmContexts = new WeakSet();
const __quenchVmIsObject = (value) =>
  value !== null && (typeof value === "object" || typeof value === "function");
const __quenchVmRunCallback = (callback, sandbox, args) => {
  const state = __quenchVmInstallContext(sandbox);
  try {
    return callback(...args);
  } finally {
    __quenchVmCopyProperties(sandbox, state.keys, state.originalGlobalKeys);
    __quenchVmRestoreProperties(
      state.keys,
      state.previous,
      state.hiddenProcess
    );
  }
};
const __quenchVmFormatError = (error, options, code) => {
  const match = /'([^']+)' is read-only/.exec(error.message || "");
  if (match)
    error.message = `Cannot assign to read only property '${match[1]}'`;
  const filename = typeof options === "string" ? options : options?.filename;
  if (filename) {
    const lineOffset =
      typeof options === "object" ? options.lineOffset || 0 : 0;
    const columnOffset =
      typeof options === "object" ? options.columnOffset || 0 : 0;
    error.stack = __quenchVmFormatStack(
      error,
      filename,
      lineOffset,
      columnOffset,
      code
    );
  }
};
const __quenchVmEvaluateContext = (code, sandbox, options, state) => {
  try {
    const result = (0, eval)(String(code));
    __quenchVmCopyProperties(sandbox, state.keys, state.originalGlobalKeys);
    return result;
  } catch (error) {
    __quenchVmFormatError(
      error,
      options,
      state.formatCode ? String(code) : null
    );
    throw error;
  }
};
const __quenchVmValidateContext = (sandbox) => {
  if (!__quenchVmIsObject(sandbox))
    __quenchVmTypeError(
      'The "contextifiedObject" argument must be of type object.'
    );
  if (!__quenchVmContexts.has(sandbox)) {
    __quenchVmTypeError(
      'The "contextifiedObject" argument must be an vm.Context'
    );
  }
};
const __quenchVmRunInContext = (code, sandbox, options) => {
  __quenchVmValidateContext(sandbox);
  const state = __quenchVmInstallContext(sandbox);
  try {
    return __quenchVmEvaluateContext(code, sandbox, options, state);
  } finally {
    __quenchVmRestoreProperties(
      state.keys,
      state.previous,
      state.hiddenProcess
    );
  }
};
const __quenchVmModule = {
  Script: class Script {
    constructor(code, options) {
      __quenchVmValidateScriptOptions(options);
      this.code = String(code);
    }
    runInContext(context, options) {
      __quenchVmValidateScriptOptions(options);
      return __quenchVmRunInContext(this.code, context, options);
    }
    runInThisContext(options) {
      __quenchVmValidateScriptOptions(options);
      return (0, eval)(this.code);
    }
    runInNewContext(context = {}, options) {
      if (!(this instanceof __quenchVmModule.Script))
        throw new TypeError("this.runInContext is not a function");
      __quenchVmValidateScriptOptions(options);
      return __quenchVmRunInNewContext(this.code, context, options);
    }
  },
  createScript: (code) => new __quenchVmModule.Script(code),
  compileFunction: (code, params, options) =>
    __quenchVmCompileFunction(code, params, options),
  SourceTextModule: class SourceTextModule {
    constructor() {
      this.namespace = Object.create(null);
      __nodeModuleNamespaces.add(this.namespace);
    }
    async link() {}
    async evaluate() {}
  },
  createContext: (sandbox = {}, options) => {
    if (!__quenchVmIsObject(sandbox))
      __quenchVmTypeError("The options argument must be an object");
    __quenchVmValidateContextOptions(options);
    __quenchVmContexts.add(sandbox);
    return sandbox;
  },
  runInThisContext: (code, options) => {
    try {
      return (0, eval)(String(code));
    } catch (error) {
      __quenchVmFormatError(error, options, null);
      throw error;
    }
  },
  runInNewContext: (code, sandbox = {}, options) =>
    __quenchVmRunInNewContext(code, sandbox, options),
  runInContext: (code, sandbox, options) =>
    __quenchVmRunInContext(code, sandbox, options)
};
const __quenchTrackTestResult = (result) => {
  if (!result?.then) return result;
  (globalThis.__quench_test_promises ||= []).push(result);
  result.catch((error) => {
    if (!globalThis.__quench_async_error)
      globalThis.__quench_async_error = String(error?.stack || error);
  });
  return result;
};
const __quenchNodeTestModule = (name, options, callback) =>
  __quenchTrackTestResult(
    (typeof options === "function" ? options : callback)({
      assert: globalThis.__nodeAssert
    })
  );
__quenchNodeTestModule.describe = (_name, callback) =>
  __quenchTrackTestResult(callback({ assert: globalThis.__nodeAssert }));
__quenchNodeTestModule.it = __quenchNodeTestModule.describe;
__quenchNodeTestModule.test = __quenchNodeTestModule;
const __quenchInternalBindingModule = {
  internalBinding: (binding) => {
    if (binding === "os") return globalThis.__quenchInternalOsBinding;
    if (binding === "debug")
      return {
        getGenericUsageCount: (name) =>
          name.includes("Uninitialized")
            ? __nodeAllocatorCounts.uninitialized
            : __nodeAllocatorCounts.zeroFilled
      };
    if (binding === "uv")
      return {
        UV_ENOENT: -2,
        UV_EEXIST: -17,
        errname: (errorNumber) =>
          globalThis.__nodeUtil.getSystemErrorName(errorNumber),
        getErrorMessage: (errorNumber) =>
          globalThis.__nodeUtil.getSystemErrorMessage(errorNumber)
      };
    if (binding === "js_stream")
      return {
        JSStream: class JSStream {
          constructor() {
            this._externalStream = { __quench_external: true };
          }
        }
      };
    if (binding === "util")
      return {
        arrayBufferViewHasBuffer: (() => {
          const observed = new WeakSet();
          return (value) => {
            if (value.byteLength >= 96 || observed.has(value)) return true;
            observed.add(value);
            return false;
          };
        })(),
        previewEntries: () => []
      };
    return { fstat: () => undefined };
  }
};
const __quenchInternalErrorsModule = {
  codes: {
    ERR_OUT_OF_RANGE: class ERR_OUT_OF_RANGE extends RangeError {},
    ERR_IPC_CHANNEL_CLOSED: class ERR_IPC_CHANNEL_CLOSED extends Error {
      constructor() {
        super("Channel closed");
        this.code = "ERR_IPC_CHANNEL_CLOSED";
      }
    }
  }
};
const __quenchInternalBufferModule = {
  utf8Write: (buffer, string, offset = 0, length = buffer.length - offset) =>
    buffer.write(string, offset, length, "utf8")
};
const __quenchInternalFsUtilsModule = {
  stringToFlags: (flags) => {
    const values = {
      r: 0,
      "r+": 2,
      rs: 1052674,
      "rs+": 1052674,
      sr: 1052674,
      "sr+": 1052674,
      w: 577,
      "w+": 578,
      wx: 705,
      xw: 705,
      "wx+": 706,
      "xw+": 706,
      a: 1089,
      "a+": 1090,
      ax: 1217,
      xa: 1217,
      "ax+": 1218,
      "xa+": 1218,
      as: 1051713,
      sa: 1051713,
      "as+": 1051714,
      "sa+": 1051714
    };
    if (typeof flags !== "string" || values[flags] === undefined) {
      const error = new TypeError(`Unknown file open flag: ${flags}`);
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    return values[flags];
  }
};
const __quenchRequireClusterInternal = (name) => {
  if (name === "internal/test/binding") return __quenchInternalBindingModule;
  if (name === "internal/errors") return __quenchInternalErrorsModule;
  if (name === "internal/buffer") return __quenchInternalBufferModule;
  if (name === "internal/fs/utils") return __quenchInternalFsUtilsModule;
  if (name === "zlib/iter")
    return {
      compressGzip: () => (chunks) => chunks,
      decompressGzip: () => (chunks) => chunks
    };
};
let __quenchClusterModule;
{
  if (!globalThis.__nodeCluster) {
    let forks = 0;
    class NodeClusterWorker extends globalThis.__nodeEventEmitter {
      constructor(id) {
        super();
        this.id = id;
        this.state = "none";
        this.exitedAfterDisconnect = false;
        const pid = 1000 + id;
        this.process = {
          pid,
          exitCode: undefined,
          signalCode: undefined,
          kill: (signal) => this.kill(signal)
        };
        this._sends = 0;
        const alive = globalThis.__quench_node_pids || new Set();
        globalThis.__quench_node_pids = alive;
        alive.add(pid);
      }
      send(...values) {
        const callback = values.at(-1);
        const hasCallback = typeof callback === "function";
        const message = hasCallback ? values.slice(0, -1) : values;
        const result = this._sends < 2;
        this._sends = hasCallback && this._sends === 3 ? 0 : this._sends + 1;
        queueMicrotask(() => {
          for (const value of message) this.emit("message", value);
        });
        if (hasCallback)
          queueMicrotask(() => {
            this._sends = 0;
            callback(null);
          });
        return result;
      }
      _markDead() {
        if (this.state === "dead") return;
        this.state = "dead";
        const alive = globalThis.__quench_node_pids;
        if (alive) alive.delete(this.process.pid);
      }
      kill(signal) {
        if (this.state === "dead") return this;
        const previousState = this.state;
        this.process.exitCode = null;
        this.process.signalCode = String(signal || "SIGTERM");
        if (previousState === "online" || previousState === "listening") {
          this.state = "disconnected";
        } else {
          this._markDead();
        }
        queueMicrotask(() => {
          if (previousState === "online" || previousState === "listening") {
            cluster.emit("disconnect", this);
            this.emit("disconnect");
            this._markDead();
            cluster.emit(
              "exit",
              this,
              this.process.exitCode,
              this.process.signalCode
            );
            this.emit("exit", this.process.exitCode, this.process.signalCode);
          } else {
            cluster.emit(
              "exit",
              this,
              this.process.exitCode,
              this.process.signalCode
            );
            this.emit("exit", this.process.exitCode, this.process.signalCode);
          }
        });
        return this;
      }
      disconnect() {
        if (this.state === "dead") return this;
        this.exitedAfterDisconnect = true;
        const previousState = this.state;
        this.process.exitCode = 0;
        this.process.signalCode = null;
        this._markDead();
        queueMicrotask(() => {
          cluster.emit("disconnect", this);
          this.emit("disconnect");
          if (previousState === "online" || previousState === "listening") {
            cluster.emit("exit", this, 0, null);
            this.emit("exit", 0, null);
          }
        });
        return this;
      }
    }
    const cluster = new globalThis.__nodeEventEmitter();
    cluster.isPrimary = true;
    cluster.isMaster = true;
    cluster.isWorker = false;
    cluster.settings = {};
    cluster.workers = [];
    cluster.Worker = NodeClusterWorker;
    cluster.setupPrimary = (settings = {}) => {
      cluster.settings = { ...settings };
      queueMicrotask(() => cluster.emit("setup"));
      return cluster.settings;
    };
    cluster.setupMaster = cluster.setupPrimary;
    cluster.fork = (env = {}) => {
      const worker = new NodeClusterWorker(++forks);
      cluster.workers.push(worker);
      worker._env = env;
      const reentry = globalThis.__quench_in_cluster_worker;
      queueMicrotask(() => {
        cluster.emit("fork", worker);
        if (worker.state !== "none") return;
        worker.state = "online";
        worker.emit("online");
        cluster.emit("online", worker);
        __quenchRunClusterWorker(cluster, worker, env, reentry);
      });
      return worker;
    };
    cluster.disconnect = (callback) => {
      for (const worker of cluster.workers) worker.disconnect();
      if (typeof callback === "function") queueMicrotask(callback);
      return cluster;
    };
    globalThis.__nodeCluster = cluster;
    globalThis.__nodeClusterListening = (info) => {
      const worker = cluster.worker;
      if (!worker) return;
      if (worker.state !== "online") return;
      worker.state = "listening";
      cluster.emit("listening", worker);
      worker.emit("listening", info);
    };
    __quenchClusterModule = cluster;
  }
}
globalThis.__quench_require_part_02 = (name, specifier) => {
  if (name === "cluster")
    return globalThis.__nodeCluster || __quenchClusterModule;
  if (name === "internal/event_target")
    return {
      Event,
      EventTarget,
      CustomEvent,
      NodeEventTarget,
      kWeakHandler: Symbol("kWeakHandler")
    };
  if (name === "stream") return globalThis.__nodeStream;
  if (name === "stream/iter") return __quenchRequireStreamIter();
  if (name === "vm") return __quenchVmModule;
  if (name === "worker_threads")
    return { isMainThread: true, MessageChannel, MessagePort };
  if (name === "node:test" || name === "test") return __quenchNodeTestModule;
  return __quenchRequireClusterInternal(name);
};
