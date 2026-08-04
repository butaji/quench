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
globalThis.__quench_require_part_02 = (name, specifier) => {
  if (name === "cluster") {
    if (globalThis.__nodeCluster) return globalThis.__nodeCluster;
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
    return cluster;
  }
  if (name === "internal/event_target")
    return { kWeakHandler: Symbol("kWeakHandler") };
  if (name === "stream") return globalThis.__nodeStream;
  if (name === "stream/iter") return __quenchRequireStreamIter();
  if (name === "vm")
    return {
      SourceTextModule: class SourceTextModule {
        constructor() {
          this.namespace = Object.create(null);
          __nodeModuleNamespaces.add(this.namespace);
        }
        async link() {}
        async evaluate() {}
      },
      createContext: (sandbox = {}) => sandbox,
      runInThisContext: (code) => (0, eval)(String(code)),
      runInNewContext: (code, sandbox = {}) => {
        const previous = {};
        for (const key of Object.keys(sandbox)) {
          previous[key] = globalThis[key];
          globalThis[key] = sandbox[key];
        }
        try {
          return (0, eval)(String(code));
        } finally {
          for (const key of Object.keys(sandbox))
            globalThis[key] = previous[key];
        }
      },
      runInContext: (code, sandbox = {}) => {
        const previous = {};
        for (const key of Object.keys(sandbox)) {
          previous[key] = globalThis[key];
          globalThis[key] = sandbox[key];
        }
        try {
          return (0, eval)(String(code));
        } finally {
          for (const key of Object.keys(sandbox))
            globalThis[key] = previous[key];
        }
      }
    };
  if (name === "worker_threads") return { isMainThread: true };
  if (name === "node:test" || name === "test")
    return {
      describe: (_name, callback) => callback(),
      it: (_name, callback) => callback(),
      test: (_name, options, callback) =>
        (typeof options === "function" ? options : callback)()
    };
  if (name === "internal/test/binding")
    return {
      internalBinding: (binding) =>
        binding === "debug"
          ? {
              getGenericUsageCount: (name) =>
                name.includes("Uninitialized")
                  ? __nodeAllocatorCounts.uninitialized
                  : __nodeAllocatorCounts.zeroFilled
            }
          : binding === "uv"
            ? { UV_ENOENT: -2, UV_EEXIST: -17 }
            : binding === "js_stream"
              ? {
                  JSStream: class JSStream {
                    constructor() {
                      this._externalStream = { __quench_external: true };
                    }
                  }
                }
              : binding === "util"
                ? {
                    arrayBufferViewHasBuffer: (() => {
                      const observed = new WeakSet();
                      return (value) => {
                        if (value.byteLength >= 96 || observed.has(value))
                          return true;
                        observed.add(value);
                        return false;
                      };
                    })(),
                    previewEntries: () => []
                  }
                : { fstat: () => undefined }
    };
  if (name === "internal/errors")
    return {
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
  if (name === "internal/buffer")
    return {
      utf8Write: (
        buffer,
        string,
        offset = 0,
        length = buffer.length - offset
      ) => buffer.write(string, offset, length, "utf8")
    };
  if (name === "internal/fs/utils")
    return {
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
  if (name === "zlib/iter")
    return {
      compressGzip: () => (chunks) => chunks,
      decompressGzip: () => (chunks) => chunks
    };
};
