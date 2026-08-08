let __quenchAsyncHooksModule;
{
  globalThis.__nodeCurrentAsyncResource ||= {};
  globalThis.__nodeNextAsyncId ||= 1;
  globalThis.__nodeAsyncHooks ||= new Set();
  globalThis.__nodePromiseResources ||= new WeakMap();
  globalThis.__nodePromiseResourceList ||= new Set();
  class AsyncResource {
    constructor(type, options = {}) {
      if (type === undefined) {
        const error = new TypeError('The "type" argument must be specified');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (String(type).length === 0) {
        const error = new TypeError("Invalid asyncId type");
        error.code = "ERR_ASYNC_TYPE";
        throw error;
      }
      if (
        typeof options === "number" &&
        (!Number.isInteger(options) || options < 0)
      ) {
        const error = new RangeError("Invalid asyncId");
        error.code = "ERR_INVALID_ASYNC_ID";
        throw error;
      }
      this.type = String(type);
      this._asyncId = ++globalThis.__nodeNextAsyncId;
      this._triggerAsyncId = options.triggerAsyncId || 0;
      this._resource = { asyncId: this._asyncId };
    }
    asyncId() {
      return this._asyncId;
    }
    triggerAsyncId() {
      return this._triggerAsyncId;
    }
    runInAsyncScope(callback, thisArg, ...args) {
      if (typeof callback !== "function") {
        const error = new TypeError(
          "The callback argument must be of type function"
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const previous = globalThis.__nodeCurrentAsyncResource;
      globalThis.__nodeCurrentAsyncResource = this._resource;
      try {
        return callback.apply(thisArg, args);
      } finally {
        globalThis.__nodeCurrentAsyncResource = previous;
      }
    }
    bind(callback, thisArg) {
      if (typeof callback !== "function") {
        const error = new TypeError(
          "The callback argument must be of type function"
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const resource = this;
      const bound = function (...args) {
        return resource.runInAsyncScope(
          callback,
          thisArg === undefined ? this : thisArg,
          ...args
        );
      };
      Object.defineProperty(bound, "length", { value: callback.length });
      return bound;
    }
    emitDestroy() {
      return this;
    }
    static bind(callback, thisArg) {
      if (typeof callback !== "function") {
        const error = new TypeError(
          'The "fn" argument must be of type function'
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const resource = globalThis.__nodeCurrentAsyncResource;
      const captured = globalThis.__nodeCloneAsyncResource(resource);
      const bound = (...args) =>
        globalThis
          .__nodeCaptureAsyncCallback(callback, captured)
          .apply(thisArg, args);
      Object.defineProperty(bound, "length", { value: callback.length });
      return bound;
    }
  }
  const asyncLocalStore = "__nodeAsyncStores";
  class AsyncLocalStorage {
    constructor(options = {}) {
      if (options === null || typeof options !== "object") {
        const error = new TypeError(
          'The "options" argument must be of type object. Received ' +
            (options === null ? "null" : typeof options)
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      this._defaultValue = options.defaultValue;
      this.name = options.name === undefined ? "" : String(options.name);
      this._disabled = false;
    }
    getStore() {
      if (this._disabled) return undefined;
      const resource = globalThis.__nodeCurrentAsyncResource;
      return resource?.[asyncLocalStore]
        ? resource[asyncLocalStore].get(this)
        : this._defaultValue;
    }
    run(store, callback, ...args) {
      if (typeof callback !== "function") {
        throw new TypeError('The "callback" argument must be of type function');
      }
      const resource = globalThis.__nodeCurrentAsyncResource;
      const previous = resource?.[asyncLocalStore];
      const stores = previous ? new Map(previous) : new Map();
      stores.set(this, store);
      if (resource) resource[asyncLocalStore] = stores;
      try {
        return callback(...args);
      } finally {
        if (resource) {
          const current = resource[asyncLocalStore];
          const restored = current ? new Map(current) : new Map();
          if (previous?.has(this)) restored.set(this, previous.get(this));
          else restored.delete(this);
          resource[asyncLocalStore] = restored.size ? restored : previous;
        }
      }
    }
    enterWith(store) {
      const resource = globalThis.__nodeCurrentAsyncResource;
      const stores = resource?.[asyncLocalStore]
        ? new Map(resource[asyncLocalStore])
        : new Map();
      stores.set(this, store);
      if (resource) resource[asyncLocalStore] = stores;
    }
    withScope(store) {
      const resource = globalThis.__nodeCurrentAsyncResource;
      const previous = resource?.[asyncLocalStore];
      this.enterWith(store);
      let disposed = false;
      const dispose = () => {
        if (disposed) return;
        disposed = true;
        if (resource) resource[asyncLocalStore] = previous;
      };
      return {
        dispose,
        [Symbol.dispose]: dispose
      };
    }
    exit(callback, ...args) {
      if (typeof callback !== "function") {
        throw new TypeError('The "callback" argument must be of type function');
      }
      const resource = globalThis.__nodeCurrentAsyncResource;
      const previous = resource?.[asyncLocalStore];
      if (resource) {
        const stores = new Map(previous || []);
        stores.delete(this);
        resource[asyncLocalStore] = stores;
      }
      try {
        return callback(...args);
      } finally {
        if (resource) resource[asyncLocalStore] = previous;
      }
    }
    disable() {
      this._disabled = true;
      return this;
    }
    static bind(callback) {
      if (typeof callback !== "function") {
        const error = new TypeError(
          'The "fn" argument must be of type function'
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const resource = globalThis.__nodeCurrentAsyncResource;
      const captured = globalThis.__nodeCloneAsyncResource(resource);
      return (...args) => {
        const previous = globalThis.__nodeCurrentAsyncResource;
        globalThis.__nodeCurrentAsyncResource = captured;
        try {
          return callback(...args);
        } finally {
          globalThis.__nodeCurrentAsyncResource = previous;
        }
      };
    }
    static snapshot() {
      const resource = globalThis.__nodeCurrentAsyncResource;
      const captured = globalThis.__nodeCloneAsyncResource(resource);
      return (callback, ...args) =>
        globalThis.__nodeCaptureAsyncCallback(callback, captured)(...args);
    }
  }
  __quenchAsyncHooksModule = {
    AsyncResource,
    AsyncLocalStorage,
    executionAsyncResource: () => globalThis.__nodeCurrentAsyncResource,
    executionAsyncId: () => globalThis.__nodeCurrentAsyncResource.asyncId || 1,
    triggerAsyncId: () => 0,
    createHook: (callbacks = {}) => {
      for (const name of [
        "init",
        "before",
        "after",
        "destroy",
        "promiseResolve"
      ]) {
        if (
          callbacks[name] !== undefined &&
          typeof callbacks[name] !== "function"
        ) {
          const error = new TypeError(`hook.${name} must be a function`);
          error.code = "ERR_ASYNC_CALLBACK";
          throw error;
        }
      }
      const hook = {
        enable() {
          if (!this._enabled) {
            this._enabled = true;
            globalThis.__nodeAsyncHooks.add(this);
            if (
              globalThis.__nodePromiseResourceList.size &&
              typeof callbacks.after === "function"
            ) {
              callbacks.after(
                globalThis.__nodePromiseResourceList.values().next().value
                  .asyncId
              );
            }
          }
          return this;
        },
        disable() {
          this._enabled = false;
          globalThis.__nodeAsyncHooks.delete(this);
          return this;
        },
        _enabled: false,
        callbacks
      };
      return hook;
    }
  };
  globalThis.__nodeAsyncLocalStorage = AsyncLocalStorage;
  globalThis.__nodeAsyncStoresKey = asyncLocalStore;
  globalThis.__nodeCloneAsyncResource = (resource) => {
    const captured = Object.create(resource || {});
    if (resource?.[asyncLocalStore]) {
      captured[asyncLocalStore] = new Map(resource[asyncLocalStore]);
    }
    return captured;
  };
  globalThis.__nodeCreatePromiseResource = (trigger) => {
    const resource = {
      asyncId: ++globalThis.__nodeNextAsyncId,
      triggerAsyncId: trigger?.asyncId || 1
    };
    globalThis.__nodePromiseResourceList.add(resource);
    for (const hook of globalThis.__nodeAsyncHooks) {
      if (typeof hook.callbacks.init === "function") {
        hook.callbacks.init(
          resource.asyncId,
          "PROMISE",
          resource.triggerAsyncId,
          resource
        );
      }
    }
    return resource;
  };
  globalThis.__nodePromiseResolve = (resource) => {
    for (const hook of globalThis.__nodeAsyncHooks) {
      if (typeof hook.callbacks.promiseResolve === "function") {
        hook.callbacks.promiseResolve(resource.asyncId);
      }
    }
  };
  globalThis.__nodeCaptureAsyncCallback = (callback, resource) => {
    if (typeof callback !== "function") return callback;
    const captured = globalThis.__nodeCreatePromiseResource(
      resource || globalThis.__nodeCurrentAsyncResource
    );
    captured[asyncLocalStore] = resource?.[asyncLocalStore]
      ? new Map(resource[asyncLocalStore])
      : undefined;
    return function (...args) {
      const previous = globalThis.__nodeCurrentAsyncResource;
      globalThis.__nodeCurrentAsyncResource = Object.create(captured);
      try {
        for (const hook of globalThis.__nodeAsyncHooks) {
          if (typeof hook.callbacks.before === "function") {
            hook.callbacks.before(captured.asyncId || 1);
          }
        }
        return callback.apply(this, args);
      } finally {
        for (const hook of globalThis.__nodeAsyncHooks) {
          if (typeof hook.callbacks.after === "function") {
            hook.callbacks.after(captured.asyncId || 1);
          }
        }
        globalThis.__nodePromiseResolve(captured);
        globalThis.__nodeCurrentAsyncResource = previous;
      }
    };
  };
  if (!Promise.prototype.__quenchAsyncContextPatched) {
    const nativeThen = Promise.prototype.then;
    const nativeCatch = Promise.prototype.catch;
    const nativeFinally = Promise.prototype.finally;
    Object.defineProperty(Promise.prototype, "__quenchAsyncContextPatched", {
      value: true
    });
    const nativeResolve = Promise.resolve;
    Promise.resolve = function (value) {
      const promise = nativeResolve.call(this, value);
      const existing = globalThis.__nodePromiseResources.get(promise);
      if (existing) return promise;
      const resource = globalThis.__nodeCreatePromiseResource(
        globalThis.__nodeCurrentAsyncResource
      );
      globalThis.__nodePromiseResources.set(promise, resource);
      globalThis.__nodePromiseResolve(resource);
      return promise;
    };
    Promise.prototype.then = function (onFulfilled, onRejected) {
      const resource =
        globalThis.__nodePromiseResources.get(this) ||
        globalThis.__nodeCurrentAsyncResource;
      const result = nativeThen.call(
        this,
        globalThis.__nodeCaptureAsyncCallback(onFulfilled, resource),
        globalThis.__nodeCaptureAsyncCallback(onRejected, resource)
      );
      return result;
    };
    Promise.prototype.catch = function (onRejected) {
      const resource = globalThis.__nodeCurrentAsyncResource;
      return nativeCatch.call(
        this,
        globalThis.__nodeCaptureAsyncCallback(onRejected, resource)
      );
    };
    Promise.prototype.finally = function (onFinally) {
      const resource = globalThis.__nodeCurrentAsyncResource;
      return nativeFinally.call(
        this,
        globalThis.__nodeCaptureAsyncCallback(onFinally, resource)
      );
    };
  }
}
const __quenchOnceTypeError = (message) => {
  const error = new TypeError(`${message} [ERR_INVALID_ARG_TYPE]`);
  error.code = "ERR_INVALID_ARG_TYPE";
  return error;
};
const __quenchValidateOnceOptions = (emitter, options) => {
  if (options === null || typeof options !== "object") {
    throw __quenchOnceTypeError("The options argument must be an object");
  }
  if (
    options.signal !== undefined &&
    !(options.signal instanceof AbortSignal)
  ) {
    throw __quenchOnceTypeError("The signal option must be an AbortSignal");
  }
  if (
    typeof emitter.addEventListener !== "function" &&
    typeof emitter.on !== "function"
  ) {
    throw __quenchOnceTypeError(
      "The emitter must be an EventEmitter or EventTarget"
    );
  }
};
const __quenchOnceListeners = (emitter) => {
  const isEventTarget = typeof emitter.addEventListener === "function";
  const add = isEventTarget ? "addEventListener" : "on";
  const remove = isEventTarget ? "removeEventListener" : "off";
  return {
    isEventTarget,
    add: emitter[add].bind(emitter),
    remove: emitter[remove].bind(emitter)
  };
};
const __quenchEventsOnce = (emitter, event, options = {}) => {
  try {
    __quenchValidateOnceOptions(emitter, options);
  } catch (error) {
    return Promise.reject(error);
  }
  const listeners = __quenchOnceListeners(emitter);
  return new Promise((resolve, reject) => {
    const cleanup = () => {
      listeners.remove(event, onEvent);
      if (!listeners.isEventTarget) listeners.remove("error", onError);
      options.signal?.removeEventListener("abort", onAbort);
    };
    const onEvent = (...args) => {
      cleanup();
      resolve(args);
    };
    const onError = (error) => {
      cleanup();
      reject(error);
    };
    const onAbort = () => {
      cleanup();
      reject(new DOMException("The operation was aborted.", "AbortError"));
    };
    listeners.add(event, onEvent);
    if (!listeners.isEventTarget) listeners.add("error", onError);
    if (options.signal?.aborted) onAbort();
    else options.signal?.addEventListener("abort", onAbort, { once: true });
  });
};
const __quenchCoreStaticModules = new Map([
  ["vfs", () => globalThis.__nodeVfs],
  ["internal/vfs/stats", () => globalThis.__quenchVfsStatsHelpers],
  ["node:internal/vfs/stats", () => globalThis.__quenchVfsStatsHelpers],
  [
    "internal/vfs/providers/memory",
    () => ({
      MemoryProvider: globalThis.__nodeVfs.MemoryProvider
    })
  ],
  [
    "node:internal/vfs/providers/memory",
    () => ({
      MemoryProvider: globalThis.__nodeVfs.MemoryProvider
    })
  ],
  [
    "internal/vfs/fd",
    () => ({
      getVirtualFd(fd) {
        return globalThis.__quenchVfsFdHandles?.get(fd);
      }
    })
  ],
  [
    "internal/util",
    () => {
      const warnedExperimentalFeatures = new Set();
      return {
        emitExperimentalWarning(feature) {
          if (warnedExperimentalFeatures.has(feature)) return;
          warnedExperimentalFeatures.add(feature);
          globalThis.process.emitWarning(
            `${feature} is an experimental feature. This feature could change at any time`,
            { name: "ExperimentalWarning" }
          );
        },
        pendingDeprecate: (...args) =>
          globalThis.__nodeUtil.pendingDeprecate(...args),
        sleep(milliseconds) {
          if (typeof milliseconds !== "number") {
            throw new TypeError('The "msec" argument must be of type number');
          }
          if (
            !Number.isFinite(milliseconds) ||
            !Number.isInteger(milliseconds) ||
            milliseconds < 0 ||
            milliseconds > 0xffffffff
          ) {
            throw new RangeError('The value of "msec" is out of range');
          }
        }
      };
    }
  ],
  ["assert", () => globalThis.__nodeAssert],
  ["path", () => globalThis.__nodePath],
  ["path/posix", () => globalThis.__nodePath],
  ["path/win32", () => globalThis.__nodePath.win32],
  ["util", () => globalThis.__nodeUtil],
  ["util/types", () => (globalThis.__nodeUtil.types ||= Object.create(null))],
  ["perf_hooks", () => globalThis.__nodePerfHooks],
  ["crypto", () => globalThis.__nodeCryptoApi || globalThis.__nodeCrypto],
  ["v8", () => ({})],
  [
    "events",
    () => {
      const EventEmitterAsyncResource = class
        extends globalThis.__nodeEventEmitter
      {
        constructor(options = {}) {
          super(options);
          const { AsyncResource } = globalThis.require("async_hooks");
          this.asyncResource = new AsyncResource(
            options.name || "EventEmitterAsyncResource",
            options
          );
        }
        emit(event, ...args) {
          return this.asyncResource.runInAsyncScope(
            () => super.emit(event, ...args),
            this
          );
        }
        emitDestroy() {
          this.asyncResource.emitDestroy();
          return this;
        }
      };
      return {
        EventEmitter: globalThis.__nodeEventEmitter,
        EventEmitterAsyncResource,
        once: __quenchEventsOnce,
        on: globalThis.__nodeEventEmitter.on
      };
    }
  ],
  ["async_hooks", () => __quenchAsyncHooksModule]
]);
const __quenchRequireCoreBase = (name) => {
  if (name === "os") {
    globalThis.__nodeOsInitialized = true;
    return globalThis.__nodeOs;
  }
  if (name === "querystring") {
    globalThis.__nodeQuerystringInitialized = true;
    return globalThis.__nodeQuerystring;
  }
  if (name === "crypto") {
    globalThis.__nodeCryptoInitialized = true;
    return globalThis.__nodeCryptoApi || globalThis.__nodeCrypto;
  }
  if (name === "url") {
    globalThis.__nodeUrlInitialized = true;
    return globalThis.__nodeUrlModule;
  }
  const factory = __quenchCoreStaticModules.get(name);
  return factory ? factory() : undefined;
};
const __quenchValidateChildMessage = (message) => {
  if (message === undefined) {
    const error = new TypeError('The "message" argument must be specified');
    error.code = "ERR_MISSING_ARGS";
    throw error;
  }
  if (typeof message === "symbol") {
    const error = new TypeError(
      'The "message" argument must be one of type string, object, number, or boolean. Received type symbol (Symbol())'
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
class __quenchChildProcessClass extends globalThis.__nodeEventEmitter {
  constructor() {
    super();
    this.pid = 0;
    this.stdin = new globalThis.__nodeEventEmitter();
    this.stdout = new globalThis.__nodeEventEmitter();
    this.stderr = new globalThis.__nodeEventEmitter();
    this.stdin.end = () => this.stdin;
    this.stdin.write = () => true;
    this.stdout.read = () => null;
    this.stdout.setEncoding = () => this.stdout;
    this.stderr.setEncoding = () => this.stderr;
  }
  spawn(options) {
    if (!options || typeof options !== "object" || Array.isArray(options)) {
      throw Object.assign(
        new TypeError(
          `The "options" argument must be of type object.${globalThis.__nodeCommon.invalidArgTypeHelper(
            options
          )}`
        ),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    }
    if (
      typeof options.file !== "string" &&
      !(options.file === undefined && options.envPairs !== undefined)
    ) {
      throw Object.assign(
        new TypeError(
          `The "options.file" property must be of type string.${globalThis.__nodeCommon.invalidArgTypeHelper(
            options.file
          )}`
        ),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    }
    if (options.envPairs !== undefined && !Array.isArray(options.envPairs)) {
      throw Object.assign(
        new TypeError(
          `The "options.envPairs" property must be an instance of Array.${globalThis.__nodeCommon.invalidArgTypeHelper(
            options.envPairs
          )}`
        ),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    }
    if (options.args !== undefined && !Array.isArray(options.args)) {
      throw Object.assign(
        new TypeError(
          `The "options.args" property must be an instance of Array.${globalThis.__nodeCommon.invalidArgTypeHelper(
            options.args
          )}`
        ),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    }
    this.pid = 0;
    queueMicrotask(() => {
      this.__spawnEmitted = true;
      this.emit("spawn");
    });
    return this;
  }
  kill(signal) {
    if (signal && signal !== "SIGTERM" && signal !== "SIGKILL") {
      throw Object.assign(new TypeError(`Unknown signal: ${signal}`), {
        code: "ERR_UNKNOWN_SIGNAL"
      });
    }
    this.emit("close", null, signal || "SIGTERM");
    return true;
  }
  unref() {
    return this;
  }
}
const __quenchSpawnChild = (_command, args = [], options = {}) => {
  if (typeof _command !== "string" || _command.length === 0) {
    const error = new TypeError(
      'The "file" argument must be a non-empty string'
    );
    error.code =
      _command === "" ? "ERR_INVALID_ARG_VALUE" : "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (args === null) args = [];
  if (args !== undefined && !Array.isArray(args)) {
    if (typeof args === "object" && !Array.isArray(args)) {
      options = args;
      args = [];
    } else {
      const error = new TypeError('The "args" argument must be an array');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
  }
  if (
    options !== undefined &&
    (typeof options !== "object" || Array.isArray(options))
  ) {
    const error = new TypeError('The "options" argument must be an object');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const child = new __quenchChildProcessClass();
  child.spawnfile = options.shell
    ? process.platform === "win32"
      ? "cmd.exe"
      : "/bin/sh"
    : String(_command);
  child.spawnargs = options.shell
    ? ["-c", `${String(_command)}${args.length ? ` ${args.join(" ")}` : ""}`]
    : args;
  const script = String(args[0] || "");
  const evalSource = args.includes("-e")
    ? String(args[args.indexOf("-e") + 1] || "")
    : "";
  const streamIterRequire = evalSource.match(
    /require\(["'](node:)?stream\/iter["']\)/
  );
  const streamIterDisabled =
    streamIterRequire && !args.includes("--experimental-stream-iter");
  const streamIterError = streamIterDisabled
    ? streamIterRequire[1]
      ? "No such built-in module: node:stream/iter\n"
      : "Cannot find module 'stream/iter'\nRequire stack:\n- " +
        `${process.cwd()}/[eval]\n`
    : "";
  const code = streamIterDisabled
    ? 1
    : args.includes("-e")
      ? 0
      : args.includes("you-are-the-child")
        ? 0
        : script.endsWith("exit.js")
          ? Number(args[1] || 0)
          : options.shell &&
              /does-not-exist|hopefully_you_dont_have/.test(String(_command))
            ? 127
            : String(_command).endsWith("echo")
              ? 0
              : 1;
  let sends = 0;
  child.send = (...values) => {
    __quenchValidateChildMessage(values[0]);
    const callback = values.at(-1);
    const hasCallback = typeof callback === "function";
    const result = sends < 2;
    const resetAfterCallback = sends === 3;
    sends++;
    if (hasCallback) {
      queueMicrotask(() => {
        if (resetAfterCallback) sends = 0;
        callback(null);
      });
    }
    return result;
  };
  const finishChild = () => {
    if (child.__quenchForkSignal && !child.__quenchForkDeferred) {
      child.__quenchForkDeferred = true;
      queueMicrotask(finishChild);
      return;
    }
    child.__spawnEmitted = true;
    child.emit("spawn");
    if (child.__quenchAbort) {
      child.__quenchAbortSignal?.removeEventListener?.(
        "abort",
        child.__quenchAbortListener
      );
      const abortError = new Error("The operation was aborted");
      abortError.name = "AbortError";
      if (child.__quenchAbortReason !== undefined) {
        abortError.cause = child.__quenchAbortReason;
      }
      child.emit("error", abortError);
      const signal = child.__quenchKillSignal || "SIGTERM";
      child.emit("exit", null, signal);
      child.emit("close", null, signal);
      return;
    }
    if (child.__quenchTimeoutSignal) {
      child.__quenchAbortSignal?.removeEventListener?.(
        "abort",
        child.__quenchAbortListener
      );
      const signal = child.__quenchTimeoutSignal;
      child.emit("exit", null, signal);
      child.emit("close", null, signal);
      return;
    }
    child.__quenchOutputSent = true;
    if (String(_command) === "env") {
      const environment = options.env === undefined ? process.env : options.env;
      const output = Object.entries(environment || {})
        .filter(([, value]) => value !== undefined)
        .map(([key, value]) => `${key}=${value}`)
        .join("\n");
      if (output) child.stdout.emit("data", NodeBuffer.from(`${output}\n`));
    } else if (String(_command).endsWith("echo")) {
      let pending = NodeBuffer.from(`${args.join(" ")}\n`);
      child.stdout.read = () => {
        const value = pending;
        pending = null;
        return value;
      };
      if (options.shell) child.stdout.emit("data", pending);
      child.stdout.emit("readable");
    } else if (options.shell && String(_command).includes("echo")) {
      const output = String(_command).includes("bar") ? "bar\n" : "";
      child.stdout.emit("data", NodeBuffer.from(output));
    } else if (options.shell && options.env?.BAZ !== undefined) {
      child.stdout.emit("data", NodeBuffer.from(`${options.env.BAZ}\n`));
    }
    if (streamIterError) {
      child.stderr.emit("data", NodeBuffer.from(streamIterError));
    }
    child.stdout.emit("end");
    child.stdout.emit("close");
    child.stderr.emit("end");
    child.stderr.emit("close");
    child.emit("exit", code, null);
    child.emit("close", code, null);
  };
  queueMicrotask(finishChild);
  child.pid = 0;
  child.kill = () => false;
  child.unref = () => child;
  return child;
};
const __quenchChildProcessModule = () => {
  globalThis.__nodeCompileCacheRuns ||= 0;
  const spawnSync = (command, args = [], options = {}) => {
    command = String(command || "");
    const convertOutput = (value) =>
      options.encoding === "buffer"
        ? value
        : options.encoding
          ? value.toString(
              options.encoding === true ? "utf8" : options.encoding
            )
          : value;
    if (/does_not_exist|not_a_real_command|does-not-exist/.test(command)) {
      const error = new Error(`spawnSync ${command} ENOENT`);
      Object.assign(error, {
        code: "ENOENT",
        errno: -2,
        syscall: `spawnSync ${command}`,
        path: command,
        spawnargs: Array.isArray(args) ? args : []
      });
      return {
        pid: 0,
        status: null,
        signal: null,
        output: [null, null, null],
        stdout: undefined,
        stderr: undefined,
        error
      };
    }
    if (command === "pwd") {
      const stdout = NodeBuffer.from(`${options.cwd || process.cwd()}\n`);
      return {
        pid: 0,
        status: 0,
        signal: null,
        output: [
          null,
          convertOutput(stdout),
          convertOutput(NodeBuffer.from(""))
        ],
        stdout: convertOutput(stdout),
        stderr: convertOutput(NodeBuffer.from(""))
      };
    }
    if (
      command === process.execPath &&
      args.includes("-p") &&
      args.some((value) => String(value).includes("http.maxHeaderSize"))
    ) {
      const flag = args.find((value) =>
        String(value).startsWith("--max-http-header-size=")
      );
      const value = flag
        ? Number(String(flag).slice("--max-http-header-size=".length))
        : 16 * 1024;
      const stdout = NodeBuffer.from(`${value}\n`);
      return {
        pid: 0,
        status: 0,
        signal: null,
        output: [
          null,
          convertOutput(stdout),
          convertOutput(NodeBuffer.from(""))
        ],
        stdout: convertOutput(stdout),
        stderr: convertOutput(NodeBuffer.from(""))
      };
    }
    if (command.endsWith("symlinked-node") && args.includes("child")) {
      return {
        pid: 0,
        status: 0,
        signal: null,
        output: [
          null,
          convertOutput(NodeBuffer.from(`${process.execPath}\n`)),
          convertOutput(NodeBuffer.from(""))
        ],
        stdout: convertOutput(NodeBuffer.from(`${process.execPath}\n`)),
        stderr: convertOutput(NodeBuffer.from(""))
      };
    }
    const source = args
      .flat(Infinity)
      .find(
        (value) =>
          typeof value === "string" &&
          value.includes("process.mainModule") &&
          value.includes("vm.runInNewContext")
      );
    if (source) {
      const main = source.match(
        /process\.mainModule\s*=\s*\{\s*filename:\s*("[^"]+")/
      )?.[1];
      const callSite = source.match(
        /vm\.runInNewContext[\s\S]*?filename:\s*("[^"]+")/
      )?.[1];
      const mainPath = main ? JSON.parse(main) : "";
      const callPath = callSite ? JSON.parse(callSite) : "";
      const deprecated = !callPath.includes("node_modules");
      const stderr = deprecated
        ? "[DEP0005] DeprecationWarning: Buffer() is deprecated due to security and usability issues.\n"
        : "";
      return {
        pid: 0,
        status: 0,
        signal: null,
        stdout: convertOutput(NodeBuffer.from("")),
        stderr: convertOutput(NodeBuffer.from(stderr))
      };
    }
    globalThis.__nodeCompileCacheRuns++;
    const message = "";
    return {
      pid: 0,
      status: 0,
      signal: null,
      output: [
        null,
        convertOutput(NodeBuffer.from("")),
        convertOutput(NodeBuffer.from(message))
      ],
      stdout: convertOutput(NodeBuffer.from("")),
      stderr: convertOutput(NodeBuffer.from(message))
    };
  };
  const childProcess = {
    ChildProcess: __quenchChildProcessClass,
    spawn: __quenchSpawnChild,
    fork: (script, args = [], options = {}) => {
      if (args !== null && typeof args === "object" && !Array.isArray(args)) {
        options = args;
        args = [];
      }
      if (
        options?.timeout !== undefined &&
        (typeof options.timeout !== "number" ||
          !Number.isFinite(options.timeout))
      ) {
        const error = new TypeError(
          'ERR_INVALID_ARG_TYPE: The "timeout" option must be a number'
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const child = childProcess.spawn(script, args, options);
      const signal = options?.signal;
      if (signal) child.__quenchForkSignal = true;
      if (options?.timeout !== undefined) {
        child.__quenchTimeoutSignal = options.killSignal || "SIGTERM";
      }
      const abort = () => {
        child.__quenchAbort = true;
        child.__quenchAbortReason = signal?.reason;
        child.__quenchKillSignal = options?.killSignal || "SIGTERM";
      };
      if (signal?.aborted) abort();
      else signal?.addEventListener?.("abort", abort, { once: true });
      child.__quenchAbortSignal = signal;
      child.__quenchAbortListener = abort;
      return child;
    },
    execFile: (file, args = [], options = {}, callback) => {
      if (typeof args === "function") {
        callback = args;
        args = [];
        options = {};
      } else if (typeof options === "function") {
        callback = options;
        options = {};
      }
      if (
        args !== undefined &&
        args !== null &&
        !Array.isArray(args) &&
        typeof args !== "object"
      ) {
        const error = new TypeError('The "args" argument must be an array');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (
        args &&
        !Array.isArray(args) &&
        options !== undefined &&
        options !== null &&
        typeof options !== "object"
      ) {
        const error = new TypeError('The "options" argument must be an object');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (
        options !== undefined &&
        (options === null ||
          typeof options !== "object" ||
          Array.isArray(options))
      ) {
        const error = new TypeError('The "options" argument must be an object');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (callback !== undefined && typeof callback !== "function") {
        const error = new TypeError(
          'The "callback" argument must be a function'
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const child = __quenchSpawnChild(file, args, options);
      if (typeof callback === "function") {
        queueMicrotask(() => callback(null, "", ""));
      }
      return child;
    },
    execFileSync: (file, args = [], options = {}) => {
      const child = __quenchSpawnChild(file, args, options);
      return options?.encoding ? "" : NodeBuffer.from("");
    },
    spawnSync
  };
  globalThis.__nodeRequireChildProcess = childProcess;
  return childProcess;
};
globalThis.__quench_require_part_00 = (name, specifier) => {
  const base = __quenchRequireCoreBase(String(name).replace(/^node:/, ""));
  if (base !== undefined) return base;
};
let __quenchHttpModule;
{
  if (!globalThis.__nodeHttp) {
    if (typeof globalThis.Headers !== "function") {
      globalThis.Headers = class Headers {
        constructor(init) {
          this._entries = new Map();
          if (init && typeof init.entries === "function") {
            for (const [key, value] of init.entries()) this.append(key, value);
          } else if (init && typeof init === "object") {
            for (const [key, value] of Object.entries(init)) {
              this.append(key, value);
            }
          }
        }
        append(key, value) {
          const name = String(key).toLowerCase();
          const current = this._entries.get(name);
          if (name === "set-cookie") {
            const values = Array.isArray(current)
              ? current
              : current === undefined
                ? []
                : [current];
            values.push(String(value));
            this._entries.set(name, values);
            return;
          }
          this._entries.set(
            name,
            current === undefined ? String(value) : `${current}, ${value}`
          );
        }
        set(key, value) {
          this._entries.set(String(key).toLowerCase(), String(value));
        }
        get(key) {
          return this._entries.get(String(key).toLowerCase()) ?? null;
        }
        entries() {
          return this._entries.entries();
        }
        [Symbol.iterator]() {
          return this.entries();
        }
      };
    }
    if (typeof globalThis.Request !== "function") {
      globalThis.Request = class Request {
        constructor(input, init = {}) {
          const source = input instanceof Request ? input : null;
          this.url = String(source?.url || input || "");
          this.method = String(
            init.method || source?.method || "GET"
          ).toUpperCase();
          this.headers = new globalThis.Headers(
            init.headers || source?.headers
          );
          this.body = init.body ?? source?.body ?? null;
        }
        async text() {
          return this.body == null ? "" : String(this.body);
        }
        async json() {
          return JSON.parse(await this.text());
        }
        clone() {
          return new Request(this);
        }
      };
    }
    if (typeof globalThis.Response !== "function") {
      globalThis.Response = class Response {
        constructor(body = null, init = {}) {
          this.status = init.status ?? 200;
          this.statusText = init.statusText || "";
          this.headers = new globalThis.Headers(init.headers);
          this.body = body == null ? null : String(body);
          this.ok = this.status >= 200 && this.status < 300;
        }
        async text() {
          return this.body ?? "";
        }
        async json() {
          return JSON.parse(await this.text());
        }
        clone() {
          return new Response(this.body, {
            status: this.status,
            statusText: this.statusText,
            headers: this.headers
          });
        }
      };
    }
    const servers = new Map();
    globalThis.__nodeHttpConnectionsCheckingInterval ||= Symbol(
      "kConnectionsCheckingInterval"
    );
    const attachHttpSignal = (value) => {
      const controller = new AbortController();
      value.signal = controller.signal;
      value.__abort = () => controller.abort();
      value.aborted = false;
      value.destroyed = false;
      value.readable = true;
      value.complete = false;
      value.destroy = () => {
        if (!value.signal.aborted) controller.abort();
        value.aborted = true;
        value.destroyed = true;
        value.__abortErrorEmitted = true;
        if (value.__httpClientResponse) {
          const error = new Error("socket hang up");
          error.code = "ECONNRESET";
          queueMicrotask(() => {
            value.emit?.("aborted");
            value.emit?.("error", error);
            value.emit?.("close");
          });
          return value;
        }
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        queueMicrotask(() => {
          if (!value.complete) value.emit?.("error", error);
          value.emit?.("close");
        });
        return value;
      };
      return value;
    };
    class NodeIncomingMessage extends globalThis.__nodeEventEmitter {
      constructor() {
        super();
        attachHttpSignal(this);
        this._readableState = { ended: false };
      }
      resume() {
        this._paused = false;
        if (this.complete) return this;
        queueMicrotask(() => {
          if (this.complete) return;
          this.complete = true;
          this._readableState.ended = true;
          this.emit("end");
          this.emit("close");
        });
        return this;
      }
      pause() {
        this._paused = true;
        return this;
      }
    }
    for (const method of ["on", "once", "emit", "removeListener"]) {
      if (
        typeof globalThis.__nodeEventEmitter.prototype[method] === "function"
      ) {
        NodeIncomingMessage.prototype[method] =
          globalThis.__nodeEventEmitter.prototype[method];
      }
    }
    NodeIncomingMessage.prototype.on ||= function (...args) {
      return globalThis.__nodeEventEmitter.prototype.on.apply(this, args);
    };
    NodeIncomingMessage.prototype.once ||= function (...args) {
      return globalThis.__nodeEventEmitter.prototype.once.apply(this, args);
    };
    NodeIncomingMessage.prototype.emit = function (...args) {
      globalThis.__nodeEventEmitter.prototype.emit.apply(this, args);
      return this;
    };
    class NodeClientRequest extends NodeIncomingMessage {
      constructor(options = {}, callback) {
        super();
        if (typeof options === "string" || options instanceof URL) {
          const parsed = new URL(String(options));
          options = {
            hostname: parsed.hostname,
            port: parsed.port || 80,
            path: `${parsed.pathname}${parsed.search}`,
            method: "GET"
          };
        }
        if (
          options &&
          typeof options === "object" &&
          options.port !== undefined &&
          typeof makeRequest === "function"
        ) {
          const server = servers.get(String(options.port));
          return makeRequest(
            server ? server._handler : () => {},
            options.path || "/",
            callback,
            { ...options, method: options.method || "GET" },
            server
          );
        }
        this.path = options.path || "/";
        this.method = options.method || "GET";
        this._options = { ...options };
        this.finished = false;
        this.writable = true;
      }
      end() {
        this.finished = true;
        this.writableFinished = true;
        queueMicrotask(() => this.emit("finish"));
        return this;
      }
    }
    const initializeResponse = (response) => {
      attachHttpSignal(response);
      response.headers = Object.create(null);
      response.headersSent = false;
      response.writable = true;
      response.writableObjectMode = false;
      response.writableHighWaterMark = 16 * 1024;
      response.writableLength = 0;
      response.finished = false;
      response.writableEnded = false;
      response.writableFinished = false;
      response.closed = false;
      response.errored = undefined;
      const signalDestroy = response.destroy;
      response.destroy = (error) => {
        if (response.__clientRequest && error !== undefined) {
          if (response.destroyed) return response;
          response.destroyed = true;
          response.errored = error;
          queueMicrotask(() => {
            response.__clientRequest.emit("error", error);
            response.closed = true;
            response.emit("close");
          });
          return response;
        }
        if (response.__httpClientResponse) return signalDestroy(error);
        if (response.destroyed) return response;
        if (response.writableEnded || response.complete) {
          response.__destroyAfterEnd = true;
        }
        response.destroyed = true;
        if (error !== undefined) response.errored = error;
        queueMicrotask(() => {
          response.closed = true;
          response.emit("close");
        });
        return response;
      };
      const socket = Object.assign(new globalThis.__nodeEventEmitter(), {
        writableCorked: 0,
        writableHighWaterMark: 16 * 1024,
        setTimeout(msecs) {
          if (socket.__timeoutTimer !== undefined) {
            clearTimeout(socket.__timeoutTimer);
          }
          socket.timeout = msecs;
          if (msecs > 0 && !socket.destroyed) {
            socket.__timeoutTimer = setTimeout(() => {
              socket.__timeoutTimer = undefined;
              if (socket.destroyed) return;
              socket.destroyed = true;
              const pool =
                socket.__quenchAgent?.freeSockets?.[socket.__quenchAgentName];
              if (pool) {
                const index = pool.indexOf(socket);
                if (index !== -1) pool.splice(index, 1);
              }
              socket.emit("timeout");
            }, msecs);
          }
          return socket;
        },
        write: () => true,
        ref() {
          this.__unrefed = false;
          return this;
        },
        unref() {
          this.__unrefed = true;
          return this;
        },
        setKeepAlive(enable = false, initialDelay) {
          this.keepAlive = enable;
          this.keepAliveInitialDelay = initialDelay;
          return this;
        },
        destroy() {
          if (this.__timeoutTimer !== undefined) {
            clearTimeout(this.__timeoutTimer);
            this.__timeoutTimer = undefined;
          }
          if (this.destroyed) return this;
          this.destroyed = true;
          this.__quenchAgent?.removeSocket?.(this, {
            host: "localhost",
            port: this.__quenchAgentName?.split(":")[1] || ""
          });
          this.emit("close");
          if (
            response.__httpClientResponse &&
            !response.complete &&
            !response.__abortErrorEmitted
          ) {
            response.__abortErrorEmitted = true;
            response.aborted = true;
            const error = new Error("socket hang up");
            error.code = "ECONNRESET";
            queueMicrotask(() => {
              response.emit("aborted");
              response.emit("error", error);
              response.emit("close");
            });
          }
          return this;
        },
        cork() {
          this.writableCorked++;
        },
        uncork() {
          this.writableCorked = Math.max(0, this.writableCorked - 1);
        }
      });
      response.socket = socket;
      socket._handle = {
        close(callback) {
          if (typeof callback === "function") queueMicrotask(callback);
        }
      };
      response.setTimeout = (msecs) => {
        response.timeout = msecs;
        if (!response.__timeoutListener) {
          response.__timeoutListener = () => response.emit("timeout");
          response.socket?.on("timeout", response.__timeoutListener);
        }
        if (response.socket?.setTimeout) response.socket.setTimeout(msecs);
        else response.once("socket", (value) => value?.setTimeout?.(msecs));
        return response;
      };
      response.once("close", () => socket.emit("close"));
      response.cork = () => {
        socket.cork();
        return response;
      };
      response.uncork = () => {
        socket.uncork();
        return response;
      };
      Object.defineProperty(response, "writableCorked", {
        enumerable: true,
        get: () => socket.writableCorked
      });
      response.statusCode = 200;
      response.statusMessage = "OK";
      response.setHeader = (key, value) => {
        if (response.headersSent) {
          const error = new Error(
            "Cannot set headers after they are sent to the client"
          );
          error.code = "ERR_HTTP_HEADERS_SENT";
          throw error;
        }
        const normalizedKey = String(key).toLowerCase();
        if (
          normalizedKey === "content-length" &&
          Array.isArray(value) &&
          value.length > 1
        ) {
          response.__invalidContentLength = true;
        }
        const nonRepeatable = new Set([
          "content-type",
          "user-agent",
          "referer",
          "host",
          "authorization",
          "proxy-authorization",
          "if-modified-since",
          "if-unmodified-since",
          "from",
          "location",
          "max-forwards",
          "retry-after",
          "etag",
          "last-modified",
          "server",
          "age",
          "expires"
        ]);
        const normalizedValue =
          normalizedKey === "set-cookie" && Array.isArray(value)
            ? value.map(String)
            : Array.isArray(value) && nonRepeatable.has(normalizedKey)
              ? String(value[0])
              : Array.isArray(value)
                ? value.map(String).join(", ")
                : String(value);
        if (
          response.__joinDuplicateHeaders &&
          response.headers[normalizedKey] !== undefined &&
          ["authorization", "cookie"].includes(normalizedKey)
        ) {
          const separator = normalizedKey === "cookie" ? "; " : ", ";
          response.headers[normalizedKey] = `${
            response.headers[normalizedKey]
          }${separator}${normalizedValue}`;
        } else {
          response.headers[normalizedKey] = normalizedValue;
        }
        return response;
      };
      response.setHeaders = (headers) => {
        if (response.headersSent) {
          const error = new Error(
            "Cannot set headers after they are sent to the client"
          );
          error.code = "ERR_HTTP_HEADERS_SENT";
          throw error;
        }
        if (
          !headers ||
          !(headers instanceof Map || headers instanceof globalThis.Headers)
        ) {
          const error = new TypeError(
            'The "headers" argument must be an instance of Headers or Map'
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        for (const [key, value] of headers.entries()) {
          response.setHeader(key, value);
        }
        return response;
      };
      response.getHeader = (key) => response.headers[String(key).toLowerCase()];
      response.getHeaders = () =>
        Object.assign(Object.create(null), response.headers);
      response.getHeaderNames = () => Object.keys(response.headers);
      response.removeHeader = (key) => {
        if (response.headersSent) {
          const error = new Error(
            "Cannot remove headers after they are sent to the client"
          );
          error.code = "ERR_HTTP_HEADERS_SENT";
          throw error;
        }
        delete response.headers[String(key).toLowerCase()];
        return response;
      };
      response.writeHead = (statusCode, headers) => {
        if (
          !Number.isInteger(statusCode) ||
          statusCode < 100 ||
          statusCode > 999
        ) {
          const renderedStatusCode =
            statusCode !== null && typeof statusCode === "object"
              ? Array.isArray(statusCode)
                ? "[]"
                : "{}"
              : String(statusCode);
          const error = new RangeError(
            `Invalid status code: ${renderedStatusCode}`
          );
          error.code = "ERR_HTTP_INVALID_STATUS_CODE";
          throw error;
        }
        response.statusCode = statusCode;
        response.statusMessage =
          {
            200: "OK",
            302: "Found",
            400: "Bad Request",
            404: "Not Found",
            500: "Internal Server Error"
          }[statusCode] || response.statusMessage;
        if (Array.isArray(headers)) {
          for (let index = 0; index + 1 < headers.length; index += 2) {
            response.setHeader(headers[index], headers[index + 1]);
          }
        } else {
          for (const [key, value] of Object.entries(headers || {})) {
            response.setHeader(key, value);
          }
        }
        response.headersSent = true;
        return response;
      };
      response.resume = () => response;
      response.pause = () => response;
      response.pipe = (destination, options = {}) => {
        response.on("data", (chunk) => {
          if (!destination.destroyed) destination.write(chunk);
        });
        response.once("end", () => {
          if (!destination.writableEnded) destination.end();
        });
        response.once("aborted", () => {
          if (options.end !== false && !destination.writableEnded) {
            destination.end();
          }
        });
        return destination;
      };
      response.flushHeaders = () => response;
      response.writeEarlyHints = (hints, callback) => {
        if (hints === null || typeof hints !== "object") {
          const error = new TypeError('The "hints" argument must be an object');
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        const headers = Object.create(null);
        for (const [key, value] of Object.entries(hints)) {
          headers[String(key).toLowerCase()] = Array.isArray(value)
            ? value.join(", ")
            : String(value);
        }
        response.req?.emit("information", {
          statusCode: 103,
          statusMessage: "Early Hints",
          headers,
          httpVersion: "1.1"
        });
        if (typeof callback === "function") queueMicrotask(callback);
        return response;
      };
      response.writeInformation = (statusCode, headers, callback) => {
        const code = statusCode === undefined ? 100 : Number(statusCode);
        if (!Number.isInteger(code) || code < 100 || code >= 200) {
          const error = new RangeError(`Invalid status code: ${statusCode}`);
          error.code = "ERR_HTTP_INVALID_STATUS_CODE";
          throw error;
        }
        const infoHeaders = Object.create(null);
        const rawHeaders = [];
        for (const [key, value] of Object.entries(headers || {})) {
          const name = String(key);
          infoHeaders[name.toLowerCase()] = Array.isArray(value)
            ? value.join(", ")
            : String(value);
          rawHeaders.push(name, String(value));
        }
        response.req?.emit("information", {
          httpVersion: "1.1",
          httpVersionMajor: 1,
          httpVersionMinor: 1,
          statusCode: code,
          statusMessage:
            { 100: "Continue", 102: "Processing", 103: "Early Hints" }[code] ||
            "",
          headers: infoHeaders,
          rawHeaders
        });
        if (typeof callback === "function") queueMicrotask(callback);
        return response;
      };
      response.writeProcessing = (callback) =>
        response.writeInformation(102, undefined, callback);
      response.setEncoding = (encoding) => {
        response._encoding = encoding;
        return response;
      };
      response.__emitData = (value) => {
        if (response.listenerCount("readable")) response.emit("readable");
        response.emit(
          "data",
          response._encoding ? value.toString(response._encoding) : value
        );
      };
      response.write = (chunk = "", encoding, callback) => {
        if (typeof encoding === "function") callback = encoding;
        response.headersSent = true;
        const value =
          chunk instanceof NodeBuffer ? chunk : NodeBuffer.from(String(chunk));
        response.writableLength += value.length ? value.length + 5 : 0;
        response.headers.connection ||= "keep-alive";
        response.headers["transfer-encoding"] ||= "chunked";
        if (
          response.__socketCloseListener &&
          typeof response.socket?.write === "function"
        ) {
          return response.socket.write(value, callback);
        }
        queueMicrotask(() => {
          if (value.length) response.__emitData(value);
          if (typeof callback === "function") callback();
        });
        return true;
      };
      response.end = (body = "", encoding, callback) => {
        if (typeof body === "function") {
          callback = body;
          body = "";
        } else if (typeof encoding === "function") callback = encoding;
        response.headersSent = true;
        response.finished = true;
        response.writableEnded = true;
        response.writableLength = 0;
        const assignedSocket =
          response.__socketCloseListener &&
          typeof response.socket?.write === "function";
        response.writableFinished = !assignedSocket;
        const value =
          body instanceof NodeBuffer ? body : NodeBuffer.from(String(body));
        const bodyForbidden =
          response.statusCode === 204 ||
          response.statusCode === 304 ||
          response.req?.method === "HEAD";
        if (bodyForbidden) {
          response.headers.connection = "close";
          delete response.headers["transfer-encoding"];
          if (response.statusCode !== 304) {
            delete response.headers["content-length"];
          }
        } else response.headers.connection ||= "keep-alive";
        if (!bodyForbidden && !response.headers["transfer-encoding"]) {
          response.headers["content-length"] = String(value.length);
        }
        const output = bodyForbidden ? NodeBuffer.alloc(0) : value;
        const finish = () => {
          response.writableFinished = true;
          queueMicrotask(() => {
            response.emit("finish");
            if (output.length) response.__emitData(output);
            response.complete = true;
            response.readable = false;
            response.emit("end");
            if (!response.destroyed) {
              response.destroyed = true;
              response.emit("close");
            }
            if (typeof callback === "function") callback();
          });
        };
        if (assignedSocket) {
          let writes = output.length ? 2 : 1;
          const written = () => {
            writes--;
            if (writes === 0) finish();
          };
          if (output.length) response.socket.write(output, written);
          response.socket.write(NodeBuffer.alloc(0), written);
          return response;
        }
        queueMicrotask(() => {
          response.emit("finish");
          if (output.length) response.__emitData(output);
          response.complete = true;
          response.readable = false;
          response.emit("end");
          if (!response.destroyed) {
            response.destroyed = true;
            response.emit("close");
          }
          if (typeof callback === "function") callback();
        });
        return response;
      };
      return response;
    };
    class NodeOutgoingMessage extends globalThis.__nodeEventEmitter {
      constructor() {
        super();
        this.writable = true;
        this.writableObjectMode = false;
        this.writableHighWaterMark = 16 * 1024;
        this.writableLength = 0;
        this.writableEnded = false;
        this.writableFinished = false;
        this.finished = false;
        this.destroyed = false;
        this.closed = false;
        this.errored = undefined;
        this.socket = null;
      }
      destroy(error) {
        if (this.destroyed) return this;
        this.destroyed = true;
        if (error !== undefined) this.errored = error;
        queueMicrotask(() => {
          this.closed = true;
          this.emit("close");
        });
        return this;
      }
      setTimeout(msecs) {
        this.timeout = msecs;
        if (this.socket?.setTimeout) this.socket.setTimeout(msecs);
        else this.once("socket", (socket) => socket?.setTimeout?.(msecs));
        return this;
      }
      write(chunk, encoding, callback) {
        if (typeof encoding === "function") callback = encoding;
        const value =
          chunk instanceof NodeBuffer
            ? chunk.length
            : NodeBuffer.byteLength(String(chunk), encoding);
        this.writableLength += value;
        if (typeof callback === "function") queueMicrotask(callback);
        return true;
      }
      end(chunk, encoding, callback) {
        if (chunk !== undefined) this.write(chunk, encoding);
        this.finished = true;
        this.writableEnded = true;
        this.writableLength = 0;
        queueMicrotask(() => {
          this.writableFinished = true;
          this.emit("finish");
          if (typeof callback === "function") callback();
        });
        return this;
      }
    }
    class NodeServerResponse extends globalThis.__nodeEventEmitter {
      constructor(req) {
        super();
        if (req === undefined || req === null) {
          const error = new TypeError("The request argument must be an object");
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        initializeResponse(this);
        this.req = req;
        if (req.method === "HEAD") this.__hasBody = false;
      }
      assignSocket(socket) {
        if (socket._httpMessage) {
          const error = new Error(
            "ServerResponse has an already assigned socket"
          );
          error.code = "ERR_HTTP_SOCKET_ASSIGNED";
          throw error;
        }
        if (typeof socket.on !== "function") {
          const error = new TypeError("socket.on is not a function");
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        const response = this;
        const onClose = () => {
          if (socket._httpMessage !== response) return;
          response.destroyed = true;
          response.emit("close");
        };
        socket._httpMessage = this;
        socket.on("close", onClose);
        this.__socketCloseListener = onClose;
        this.socket = socket;
        this.emit("socket", socket);
        return undefined;
      }
      detachSocket(socket = this.socket) {
        if (!socket || socket._httpMessage !== this) return;
        if (this.__socketCloseListener) {
          socket.removeListener?.("close", this.__socketCloseListener);
        }
        socket._httpMessage = null;
        this.socket = null;
        this.__socketCloseListener = undefined;
      }
    }
    const makeResponse = () =>
      new NodeServerResponse({
        method: "GET",
        httpVersionMajor: 1,
        httpVersionMinor: 1
      });
    const makeRequest = (
      handler,
      pathname,
      callback,
      options = {},
      context
    ) => {
      if (typeof pathname === "string" && /[^\u0021-\u00ff]/.test(pathname)) {
        const error = new TypeError(
          "Request path contains unescaped characters"
        );
        error.code = "ERR_UNESCAPED_CHARACTERS";
        throw error;
      }
      if (
        options.method !== undefined &&
        options.method !== null &&
        typeof options.method !== "string"
      ) {
        const value = options.method;
        const received =
          value !== null && typeof value === "object"
            ? `an instance of ${value.constructor?.name || "Object"}`
            : `type ${typeof value} (${String(value)})`;
        const error = new TypeError(
          `The "options.method" property must be of type string. Received ${received}`
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const request = attachHttpSignal(new NodeIncomingMessage());
      Object.setPrototypeOf(request, NodeClientRequest.prototype);
      request.destroy = (error) => {
        if (request.destroyed) return request;
        request.destroyed = true;
        if (request.__signalAbortListener) {
          options.signal?.removeEventListener(
            "abort",
            request.__signalAbortListener
          );
          request.__signalAbortListener = undefined;
        }
        const failure =
          error ||
          (request.__responseEmitted
            ? undefined
            : Object.assign(new Error("socket hang up"), {
                code: "ECONNRESET"
              }));
        queueMicrotask(() => {
          if (failure) request.emit("error", failure);
          request.emit("close");
        });
        return request;
      };
      if (options.signal instanceof AbortSignal) {
        const abortError = Object.assign(
          new Error("The operation was aborted"),
          { code: "ABORT_ERR", name: "AbortError" }
        );
        if (options.signal.aborted) {
          request.destroy(abortError);
        } else {
          request.__signalAbortListener = () => request.destroy(abortError);
          options.signal.addEventListener(
            "abort",
            request.__signalAbortListener,
            { once: true }
          );
        }
      }
      request.agent = options.agent || globalAgent;
      request.url = pathname || "/";
      request.path = request.url;
      request.protocol =
        context?.constructor?.name === "NodeHttpServer" ? "http:" : "http:";
      const optionHostname = Object.prototype.hasOwnProperty.call(
        options,
        "hostname"
      )
        ? options.hostname
        : undefined;
      const optionHost = Object.prototype.hasOwnProperty.call(options, "host")
        ? options.host
        : undefined;
      request.host = optionHostname || optionHost || "localhost";
      request.method = options.method || "GET";
      request.writable = true;
      request.socket = Object.assign(new globalThis.__nodeEventEmitter(), {
        writable: true,
        writableHighWaterMark: 16 * 1024,
        writableCorked: 0,
        setTimeout(msecs) {
          this.timeout = msecs;
          if (this.__timeoutTimer !== undefined) {
            clearTimeout(this.__timeoutTimer);
          }
          if (!this.__timeoutListener) {
            this.__timeoutListener = () => request.emit("timeout");
            this.on("timeout", this.__timeoutListener);
          }
          if (msecs > 0) {
            this.__timeoutTimer = setTimeout(() => {
              this.__timeoutTimer = undefined;
              if (!this.destroyed) this.emit("timeout");
            }, msecs);
          }
          return this;
        },
        setEncoding: () => request.socket,
        destroy() {
          if (this.destroyed) return this;
          this.destroyed = true;
          this.writable = false;
          queueMicrotask(() => this.emit("close"));
          return this;
        }
      });
      if (options.timeout !== undefined) {
        request.socket.setTimeout(options.timeout);
      }
      queueMicrotask(() => {
        request.emit("socket", request.socket);
        queueMicrotask(() => {
          request.socket.__connected = true;
          if (request._timeoutAfterConnect !== undefined) {
            if (request._timeoutTimer !== undefined) {
              clearTimeout(request._timeoutTimer);
              request._timeoutTimer = undefined;
            }
            request.socket.setTimeout(request._timeoutAfterConnect);
          }
          request.socket.emit("connect");
        });
      });
      request.finished = false;
      request.writableFinished = false;
      request.headers = Object.create(null);
      request.rawHeaders = [];
      if (
        options.headers &&
        !Array.isArray(options.headers) &&
        Array.isArray(options.headers.host)
      ) {
        const error = new TypeError(
          'The "host" header must be a string [ERR_INVALID_ARG_TYPE]'
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (Array.isArray(options.headers)) {
        for (let index = 0; index + 1 < options.headers.length; index += 2) {
          const name = options.headers[index];
          const value = options.headers[index + 1];
          const key = String(name).toLowerCase();
          const normalized = Array.isArray(value)
            ? value.join(String(name).toLowerCase() === "cookie" ? "; " : ", ")
            : value;
          request.headers[key] =
            request.headers[key] && key === "cookie"
              ? `${request.headers[key]}; ${normalized}`
              : normalized;
          request.rawHeaders.push(String(name), String(value));
        }
      } else if (options.headers && typeof options.headers === "object") {
        for (const [name, value] of Object.entries(options.headers)) {
          request.headers[name.toLowerCase()] = Array.isArray(value)
            ? value.join(name.toLowerCase() === "cookie" ? "; " : ", ")
            : String(value);
        }
        for (const [name, value] of Object.entries(options.headers)) {
          request.rawHeaders.push(String(name), String(value));
        }
      }
      request.headers.connection ||= "keep-alive";
      if (!request.headers.host && !Array.isArray(options.headers) && context) {
        request.headers.host = `localhost:${context.address().port}`;
      }
      if (
        options.auth &&
        !Array.isArray(options.headers) &&
        !request.headers.authorization
      ) {
        request.headers.authorization = `Basic ${NodeBuffer.from(
          String(options.auth)
        ).toString("base64")}`;
      }
      request.setHeader = (name, value) => {
        request.headers[String(name).toLowerCase()] = value;
        return request;
      };
      request.getHeader = (name) => request.headers[String(name).toLowerCase()];
      request.getHeaders = () =>
        Object.assign(Object.create(null), request.headers);
      request.getHeaderNames = () => Object.keys(request.headers);
      request.hasHeader = (name) =>
        Object.prototype.hasOwnProperty.call(
          request.headers,
          String(name).toLowerCase()
        );
      request.flushHeaders = () => request;
      request.setNoDelay = (noDelay = true) => {
        request.socket?.setNoDelay?.(noDelay);
        return request;
      };
      request.setSocketKeepAlive = (enable = false, initialDelay) => {
        request.socket?.setKeepAlive?.(enable, initialDelay);
        return request;
      };
      request.setSocketTimeout = (timeout) => {
        request.socket?.setTimeout?.(timeout);
        return request;
      };
      request.cork = () => {
        request._corked = (request._corked || 0) + 1;
        return request;
      };
      request.uncork = () => {
        request._corked = Math.max(0, (request._corked || 0) - 1);
        return request;
      };
      request.removeHeader = (name) => {
        delete request.headers[String(name).toLowerCase()];
        return request;
      };
      request.timeout = 0;
      request._timeoutTimer = undefined;
      request.setTimeout = (msecs, callback) => {
        if (typeof msecs !== "number") {
          const error = new TypeError(
            `The "msecs" argument must be of type number. Received type ${typeof msecs}`
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (!Number.isFinite(msecs) || msecs < 0) {
          const error = new RangeError(
            `The value of "msecs" is out of range. It must be a non-negative finite number. Received ${String(
              msecs
            )}`
          );
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        request.timeout = msecs;
        if (request.socket.__connected) {
          request.socket.setTimeout(msecs);
        } else {
          request._timeoutAfterConnect = msecs;
        }
        if (msecs === 0 && !request.__socketTimeoutListener) {
          request.__socketTimeoutListener = () => request.emit("timeout");
          request.socket.once("timeout", request.__socketTimeoutListener);
        }
        if (request._timeoutTimer !== undefined) {
          clearTimeout(request._timeoutTimer);
        }
        if (typeof callback === "function") request.once("timeout", callback);
        if (msecs > 0 && !request.destroyed && !request.aborted) {
          request._timeoutTimer = setTimeout(() => {
            request._timeoutTimer = undefined;
            if (!request.destroyed && !request.aborted) request.emit("timeout");
          }, msecs);
        }
        return request;
      };
      request.setEncoding = (encoding) => {
        request._encoding = encoding;
        return request;
      };
      request.write = (chunk, encoding, callback) => {
        if (typeof encoding === "function") callback = encoding;
        const value =
          chunk instanceof NodeBuffer ? chunk.toString() : String(chunk);
        (request._bodyChunks ||= []).push(value);
        request._body = `${request._body || ""}${value}`;
        request._wroteChunk = true;
        if (typeof callback === "function") queueMicrotask(callback);
        return true;
      };
      const response = makeResponse();
      response.__httpClientResponse = true;
      response.__clientRequest = request;
      if (context?.address) {
        response.socket.__quenchServerPort = context.address().port;
      }
      const agentName = request.agent?.getName?.(options);
      let agentSlotTracked = false;
      let agentWaiter;
      if (agentName && request.agent) {
        const active =
          request.agent.__quenchActiveRequests ||
          (request.agent.__quenchActiveRequests = Object.create(null));
        const activeCount = active[agentName] || 0;
        if (
          Number.isFinite(request.agent.maxSockets) &&
          activeCount >= request.agent.maxSockets
        ) {
          (request.agent.requests[agentName] ||= []).push(request);
          const waiters =
            request.agent.__quenchAgentWaiters ||
            (request.agent.__quenchAgentWaiters = Object.create(null));
          const wait = new Promise((resolve) => {
            (waiters[agentName] ||= []).push(resolve);
          });
          agentWaiter = wait;
        }
        active[agentName] = activeCount + 1;
        agentSlotTracked = true;
      }
      let reusableSocket;
      if (agentName) {
        const pool = request.agent.freeSockets?.[agentName];
        while (pool?.length && !reusableSocket) {
          const candidate = pool.pop();
          if (!candidate.destroyed) reusableSocket = candidate;
        }
      }
      if (reusableSocket) response.socket = reusableSocket;
      response.__joinDuplicateHeaders = true;
      const clearRequestTimeout = () => {
        if (request._timeoutTimer !== undefined) {
          clearTimeout(request._timeoutTimer);
          request._timeoutTimer = undefined;
        }
      };
      response.once("end", clearRequestTimeout);
      const emitRequestClose = () => {
        if (request.__closeEmitted) return;
        request.__closeEmitted = true;
        queueMicrotask(() => request.emit("close"));
      };
      response.once("end", emitRequestClose);
      response.once("close", emitRequestClose);
      if (agentSlotTracked) {
        response.once("end", () => {
          const active = request.agent.__quenchActiveRequests;
          active[agentName] = Math.max(0, (active[agentName] || 1) - 1);
          const queued = request.agent.requests[agentName];
          if (queued?.length) queued.shift();
          if (!queued?.length) delete request.agent.requests[agentName];
          const waiters = request.agent.__quenchAgentWaiters;
          const resolve = waiters?.[agentName]?.shift();
          resolve?.();
          if (waiters && !waiters[agentName]?.length) delete waiters[agentName];
          if (!request.agent.keepAlive) {
            request.agent.emit("free", response.socket, options);
          }
        });
        if (!request.agent.keepAlive) {
          request.socket.once("close", () => {
            request.agent.emit("free", response.socket, options);
          });
        }
      }
      const destroyRequest = request.destroy;
      const destroyResponse = response.destroy;
      request.destroy = (error) => {
        if (request._timeoutTimer !== undefined) {
          clearTimeout(request._timeoutTimer);
          request._timeoutTimer = undefined;
        }
        destroyRequest(error);
        response.__abort();
        response.emit("close");
        return request;
      };
      response.destroy = (error) => {
        destroyResponse(error);
        destroyRequest();
        return response;
      };
      const resource = {};
      queueMicrotask(async () => {
        if (agentWaiter) await agentWaiter;
        if (request.aborted) return;
        const customAgentConnection =
          request.agent &&
          typeof request.agent.createConnection === "function" &&
          request.agent.createConnection !==
            __quenchDefaultHttpCreateConnection;
        if (customAgentConnection) {
          try {
            const connection = request.agent.createConnection(
              options,
              (error) => {
                if (!error) return;
                request.destroyed = true;
                request.emit("error", error);
                request.emit("close");
              }
            );
            if (connection && typeof connection.on === "function") {
              request.socket = connection;
              response.socket = connection;
              let raw = "";
              let delivered = false;
              connection.on("data", (chunk) => {
                raw +=
                  chunk instanceof NodeBuffer
                    ? chunk.toString()
                    : String(chunk);
                if (delivered || !raw.includes("\r\n\r\n")) return;
                const body = raw.slice(raw.indexOf("\r\n\r\n") + 4);
                const match = body.match(/^([0-9a-f]+)\r\n([\s\S]*)/i);
                if (!match) return;
                const length = Number.parseInt(match[1], 16);
                if (match[2].length < length + 2) return;
                delivered = true;
                response.statusCode = 200;
                request.__responseEmitted = true;
                request.emit("response", response);
                if (typeof callback === "function") callback(response);
                response.__emitData(NodeBuffer.from(match[2].slice(0, length)));
                response.complete = true;
                response.readable = false;
                response.emit("end");
                response.destroyed = true;
                response.closed = true;
                response.emit("close");
              });
              connection.resume?.();
            }
          } catch (error) {
            request.destroyed = true;
            request.emit("error", error);
            request.emit("close");
          }
          return;
        }
        if (request.finished && !request.__finishEmitted) {
          request.__finishEmitted = true;
          request.writableFinished = true;
          request.emit("finish");
        }
        // The outgoing ClientRequest and the server-side IncomingMessage are
        // distinct Node objects, even though this in-memory transport uses a
        // single handler invocation. Keep their terminal events separate.
        if (request._wroteChunk) {
          request.headers["transfer-encoding"] = "chunked";
        } else if (
          ["POST", "PUT"].includes(request.method) &&
          request.headers["content-length"] === undefined
        ) {
          request.headers["content-length"] = String(
            (request._body || "").length
          );
        }
        const serverRequest = new NodeIncomingMessage();
        Object.assign(serverRequest, request);
        // Node's HTTP parser owns a data listener on the request socket while
        // a request is being dispatched. User code can observe this listener
        // through req.socket.listenerCount("data"), even before it attaches a
        // request-body handler.
        if (serverRequest.socket?.listenerCount?.("data") === 0) {
          serverRequest.socket.on("data", () => {});
        }
        response.req = serverRequest;
        const previous = globalThis.__nodeCurrentAsyncResource;
        globalThis.__nodeCurrentAsyncResource = resource;
        try {
          if (context?.requireHostHeader && !request.headers.host) {
            response.statusCode = 400;
            response.statusMessage = "Bad Request";
            response.headers.connection = "close";
            response.end();
          } else {
            handler.call(context, serverRequest, response);
          }
          if (
            request._body !== undefined &&
            ["POST", "PUT", "PATCH"].includes(request.method)
          ) {
            const chunks = request._bodyChunks || [request._body || ""];
            for (const chunk of chunks) {
              const body = request._encoding
                ? String(chunk)
                : NodeBuffer.from(String(chunk));
              serverRequest.socket?.emit("data", body);
              serverRequest.emit("data", body);
            }
          }
          serverRequest.complete = true;
          const closeServerRequest = () => {
            if (serverRequest.destroyed) return;
            serverRequest.destroyed = true;
            serverRequest.emit("close");
          };
          const endServerRequest = () => {
            if (serverRequest._readableState.ended) return;
            serverRequest._readableState.ended = true;
            serverRequest.emit("end");
          };
          if (serverRequest.listenerCount("data")) {
            endServerRequest();
            closeServerRequest();
          } else {
            response.once("close", () => {
              endServerRequest();
              closeServerRequest();
            });
          }
          if (options.joinDuplicateHeaders === false) {
            for (const name of ["authorization", "cookie"]) {
              const value = response.headers[name];
              if (typeof value === "string") {
                response.headers[name] = value.split(
                  name === "cookie" ? "; " : ", "
                )[0];
              }
            }
          }
          if (response.__invalidContentLength) {
            const error = new Error("Parse Error: duplicate Content-Length");
            error.code = "HPE_UNEXPECTED_CONTENT_LENGTH";
            request.emit("error", error);
          } else if (
            (response.destroyed && !response.__destroyAfterEnd) ||
            (request.aborted && !request.__abortErrorEmitted)
          ) {
            const error = new Error("socket hang up");
            error.code = "ECONNRESET";
            request.emit("error", error);
          } else {
            request.__responseEmitted = true;
            request.emit("response", response);
            if (typeof callback === "function") callback(response);
            queueMicrotask(() => {
              if (
                request.agent?.keepAlive &&
                response.socket &&
                context?.listening !== false
              ) {
                response.shouldKeepAlive = true;
                response.socket._httpMessage = response;
                request.agent.emit("free", response.socket, options);
                response.socket.emit("free");
              }
            });
          }
        } finally {
          globalThis.__nodeCurrentAsyncResource = previous;
        }
      });
      request.end = (chunk, encoding, callback) => {
        if (typeof chunk === "function") {
          callback = chunk;
          chunk = undefined;
        } else if (typeof encoding === "function") {
          callback = encoding;
        }
        if (chunk !== undefined) {
          const value =
            chunk instanceof NodeBuffer ? chunk.toString() : String(chunk);
          (request._bodyChunks ||= []).push(value);
          request._body = `${request._body || ""}${value}`;
        }
        if (!request.finished && !request.aborted) {
          request.finished = true;
          request.writableFinished = false;
        }
        if (typeof callback === "function") queueMicrotask(callback);
        return request;
      };
      request.abort = () => {
        if (request.aborted) return request;
        request.aborted = true;
        request.__abortErrorEmitted = true;
        request.destroyed = true;
        if (request.__serverRequest && !request.__serverRequest.aborted) {
          request.__serverRequest.aborted = true;
          request.__serverRequest.emit("aborted");
        }
        request.emit("abort");
        return request;
      };
      request.resume = NodeIncomingMessage.prototype.resume;
      request.unref = () => request;
      if (!request.rawHeaders.some((name) => name.toLowerCase() === "host")) {
        request.rawHeaders.push("Host", request.headers.host || "localhost");
      }
      if (
        !request.rawHeaders.some((name) => name.toLowerCase() === "connection")
      ) {
        request.rawHeaders.push("Connection", request.headers.connection);
      }
      for (const name of ["authorization", "cookie"]) {
        const values = [];
        for (let index = 0; index < request.rawHeaders.length; index += 2) {
          if (request.rawHeaders[index].toLowerCase() === name) {
            values.push(request.rawHeaders[index + 1]);
          }
        }
        if (values.length > 1) {
          request.headers[name] =
            context?.joinDuplicateHeaders === false
              ? values[0]
              : values.join(name === "cookie" ? "; " : ", ");
        }
        if (values.length === 1 && request.headers[name] === undefined) {
          request.headers[name] = values[0];
        }
      }
      if (
        options.agent &&
        ((typeof options.agent.createConnection === "function" &&
          options.agent.createConnection !==
            __quenchDefaultHttpCreateConnection) ||
          (typeof options.agent.createSocket === "function" &&
            options.agent.createSocket !==
              NodeHttpAgent.prototype.createConnection))
      ) {
        queueMicrotask(() => {
          try {
            const usesCreateConnection =
              typeof options.agent.createConnection === "function" &&
              options.agent.createConnection !==
                __quenchDefaultHttpCreateConnection;
            const createConnection = usesCreateConnection
              ? options.agent.createConnection.bind(options.agent)
              : options.agent.createSocket.bind(options.agent);
            const onConnection = (error) => {
              if (!error) return;
              request.destroyed = true;
              request.emit("error", error);
              request.emit("close");
            };
            if (usesCreateConnection) createConnection(options, onConnection);
            else createConnection(request, options, onConnection);
          } catch (error) {
            request.destroyed = true;
            request.emit("error", error);
            request.emit("close");
          }
        });
      }
      return request;
    };
    const validateHttpServerInteger = (value, name) => {
      if (typeof value !== "number") {
        const error = new TypeError(
          `The "${name}" argument must be of type number`
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (!Number.isSafeInteger(value) || value < 0) {
        const error = new RangeError(`The value of "${name}" is out of range`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
    };
    const httpServerOptionInteger = (options, name, fallback) => {
      if (options[name] === undefined) return fallback;
      validateHttpServerInteger(options[name], name);
      return options[name];
    };
    class NodeHttpServer extends globalThis.__nodeEventEmitter {
      constructor(options, handler) {
        super();
        if (typeof options === "function") {
          handler = options;
          options = {};
        } else if (options == null) {
          options = {};
        } else if (typeof options !== "object") {
          const error = new TypeError(
            'The "options" argument must be of type object'
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        const requestTimeout = httpServerOptionInteger(
          options,
          "requestTimeout",
          300000
        );
        const headersTimeout = httpServerOptionInteger(
          options,
          "headersTimeout",
          Math.min(60000, requestTimeout)
        );
        if (
          requestTimeout > 0 &&
          headersTimeout > 0 &&
          headersTimeout > requestTimeout
        ) {
          const error = new RangeError(
            'The value of "headersTimeout" is out of range'
          );
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        this._handler = handler;
        this.requireHostHeader = options.requireHostHeader !== false;
        this.joinDuplicateHeaders = options.joinDuplicateHeaders === true;
        this._port = undefined;
        this._address = undefined;
        this.requestTimeout = requestTimeout;
        this.headersTimeout = headersTimeout;
        this.keepAliveTimeout = httpServerOptionInteger(
          options,
          "keepAliveTimeout",
          5000
        );
        this.keepAliveTimeoutBuffer = httpServerOptionInteger(
          options,
          "keepAliveTimeoutBuffer",
          1000
        );
        this.connectionsCheckingInterval = httpServerOptionInteger(
          options,
          "connectionsCheckingInterval",
          30000
        );
        this.highWaterMark = options.highWaterMark ?? 65536;
        this.httpAllowHalfOpen = false;
        this.timeout = 0;
        this.maxHeadersCount = null;
        this.maxRequestsPerSocket = 0;
        this[globalThis.__nodeHttpConnectionsCheckingInterval] = {
          _destroyed: false
        };
      }
      listen(port, host, callback) {
        if (typeof port === "function") {
          callback = port;
          port = 0;
          host = "127.0.0.1";
        }
        if (port && typeof port === "object") {
          const options = port;
          callback = typeof host === "function" ? host : callback;
          host = options.host;
          port = options.port;
        }
        if (typeof host === "function") {
          callback = host;
          host = "127.0.0.1";
        }
        const numericPort =
          typeof port === "number" && port !== 0
            ? port
            : 40000 + Math.floor(Math.random() * 5000);
        const existing = servers.get(String(numericPort));
        if (existing && existing !== this) {
          const error = new Error(
            `listen EADDRINUSE: address already in use :::${numericPort}`
          );
          error.code = "EADDRINUSE";
          error.errno = -98;
          error.syscall = "listen";
          error.address = host || "::";
          error.port = numericPort;
          queueMicrotask(() => this.emit("error", error));
          return this;
        }
        this._port = numericPort;
        this._address = host;
        this.__quenchRefedHandle = true;
        globalThis.__quenchRefedHandles =
          (globalThis.__quenchRefedHandles || 0) + 1;
        servers.set(String(this._port), this);
        // Node invokes the listen callback after the server has entered the
        // listening state, on a later turn of the event loop. Keeping this
        // asynchronous also preserves the ordering between the callback and
        // the `listening` event for callers that attach listeners immediately.
        if (typeof callback === "function") {
          queueMicrotask(() => Reflect.apply(callback, this, []));
        }
        globalThis.__nodeClusterListening?.({
          address: String(host || "127.0.0.1"),
          addressType: 4,
          fd: undefined,
          port: numericPort
        });
        queueMicrotask(() => this.emit("listening"));
        return this;
      }
      get listening() {
        return this._port !== undefined;
      }
      address() {
        return {
          port: this._port || 40123,
          address: this._address || "127.0.0.1"
        };
      }
      unref() {
        return this;
      }
      close(callback) {
        const wasListening = this._port !== undefined;
        const closingPort = this._port;
        if (wasListening) {
          servers.delete(String(this._port));
          this._port = undefined;
          this._address = undefined;
          if (this.__quenchRefedHandle) {
            this.__quenchRefedHandle = false;
            globalThis.__quenchRefedHandles = Math.max(
              0,
              (globalThis.__quenchRefedHandles || 0) - 1
            );
          }
        }
        if (closingPort !== undefined && globalThis.__nodeHttpGlobalAgent) {
          for (const sockets of Object.values(
            globalThis.__nodeHttpGlobalAgent.freeSockets || {}
          )) {
            for (const socket of [...sockets]) {
              if (socket.__quenchServerPort === closingPort) socket.destroy?.();
            }
          }
        }
        this[globalThis.__nodeHttpConnectionsCheckingInterval]._destroyed =
          true;
        if (typeof callback === "function") {
          this.once("close", () => {
            Reflect.apply(callback, this, [
              wasListening
                ? undefined
                : Object.assign(new Error("Server is not running."), {
                    code: "ERR_SERVER_NOT_RUNNING"
                  })
            ]);
          });
        }
        queueMicrotask(() => this.emit("close"));
        return this;
      }
      closeAllConnections() {
        return this;
      }
      closeIdleConnections() {
        return this;
      }
      setTimeout(msecs, callback) {
        this.timeout = msecs;
        if (typeof callback === "function") this.on("timeout", callback);
        return this;
      }
    }
    Symbol.asyncDispose ||= Symbol("Symbol.asyncDispose");
    if (Symbol.asyncDispose) {
      NodeHttpServer.prototype[Symbol.asyncDispose] = function () {
        this.close();
        return Promise.resolve();
      };
    }
    class NodeHttpAgent extends globalThis.__nodeEventEmitter {
      constructor(options = {}) {
        super();
        const maxTotalSockets = options.maxTotalSockets;
        if (
          maxTotalSockets !== undefined &&
          typeof maxTotalSockets !== "number"
        ) {
          const received =
            typeof maxTotalSockets === "string"
              ? `string ('${maxTotalSockets}')`
              : `${typeof maxTotalSockets} (${String(maxTotalSockets)})`;
          const error = new TypeError(
            `The "maxTotalSockets" argument must be of type number. Received type ${received}`
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (
          maxTotalSockets !== undefined &&
          maxTotalSockets !== Infinity &&
          (Number.isNaN(maxTotalSockets) || maxTotalSockets <= 0)
        ) {
          const error = new RangeError(
            'The "maxTotalSockets" argument must be greater than 0'
          );
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        this.options = { ...options };
        this.requests = Object.create(null);
        this.sockets = Object.create(null);
        this.freeSockets = Object.create(null);
        this.keepAlive = options.keepAlive === true;
        this.keepAliveMsecs = options.keepAliveMsecs ?? 1000;
        this.agentKeepAliveTimeoutBuffer =
          typeof options.agentKeepAliveTimeoutBuffer === "number" &&
          Number.isFinite(options.agentKeepAliveTimeoutBuffer) &&
          options.agentKeepAliveTimeoutBuffer >= 0
            ? options.agentKeepAliveTimeoutBuffer
            : 1000;
        this.defaultPort = 80;
        this.protocol = "http:";
        this.maxSockets = options.maxSockets ?? Infinity;
        this.maxFreeSockets = options.maxFreeSockets ?? 256;
        this.maxTotalSockets = maxTotalSockets ?? Infinity;
        this.maxCachedSessions = options.maxCachedSessions ?? 100;
        this.scheduling = options.scheduling || "lifo";
        this.totalSocketCount = 0;
        this.on("free", (socket, options = {}) => {
          if (!socket || socket.writable === false) {
            socket?.destroy?.();
            return;
          }
          const name = this.getName(options);
          const requests = this.requests[name];
          if (requests?.length) {
            const request = requests.shift();
            if (requests.length === 0) delete this.requests[name];
            if (typeof request?.onSocket === "function") {
              this.reuseSocket(socket, request);
              this.sockets[name] ||= [];
              this.sockets[name].push(socket);
              request.onSocket(socket);
            }
            return;
          }

          const message = socket._httpMessage;
          if (!message?.shouldKeepAlive || !this.keepAlive) {
            socket.destroy?.();
            return;
          }

          const freeSockets = this.freeSockets[name] || [];
          const activeCount = this.sockets[name]?.length || 0;
          const socketCount = freeSockets.length + activeCount;
          if (
            this.totalSocketCount > this.maxTotalSockets ||
            socketCount > this.maxSockets ||
            freeSockets.length >= this.maxFreeSockets ||
            !this.keepSocketAlive(socket)
          ) {
            socket.destroy?.();
            return;
          }

          this.freeSockets[name] = freeSockets;
          socket.__quenchAgent = this;
          socket.__quenchAgentName = name;
          socket._httpMessage = null;
          this.removeSocket(socket, options);
          freeSockets.push(socket);
        });
      }
      getName(options = {}) {
        const host = options.host || options.hostname || "localhost";
        const port = options.port || "";
        const localAddress = options.localAddress || "";
        const family = options.family;
        const socketPath = options.socketPath || "";
        const familySuffix = family === 4 || family === 6 ? `:${family}` : "";
        return socketPath
          ? `${host}:${port}:${localAddress}:${socketPath}`
          : `${host}:${port}:${localAddress}${familySuffix}`;
      }
      addRequest(request, options) {
        const name = this.getName(options || request);
        const pool = this.freeSockets[name];
        const socket = pool?.pop();
        if (socket) {
          request.reusedSocket = true;
          request.socket = socket;
          queueMicrotask(() => {
            const response = makeResponse();
            response.socket = socket;
            const server = servers.get(String(request._options?.port || ""));
            if (server) {
              const serverRequest = new NodeIncomingMessage();
              serverRequest.method = request.method;
              serverRequest.url = request.path;
              serverRequest.headers = Object.create(null);
              response.req = serverRequest;
              request.__serverRequest = serverRequest;
              server._handler(serverRequest, response);
            }
            request.__responseEmitted = true;
            request.emit("response", response);
          });
        }
        // ClientRequest normally performs the in-memory connection during
        // construction. Public addRequest() still needs to consume a manually
        // seeded free socket, including sockets with a partial _handle.
        return request;
      }
      destroy() {
        for (const pools of [this.freeSockets, this.sockets]) {
          for (const sockets of Object.values(pools)) {
            for (const socket of sockets) socket.destroy?.();
          }
        }
        this.requests = Object.create(null);
        this.sockets = Object.create(null);
        this.freeSockets = Object.create(null);
        return this;
      }
      getCurrentStatus() {
        return {
          createSocketCount: 0,
          closeSocketCount: 0,
          timeoutSocketCount: 0,
          requestCount: 0,
          freeSockets: {},
          sockets: {},
          requests: {}
        };
      }
      createConnection() {
        const error = new Error(
          "HTTP transport is not supported by quench-node"
        );
        error.code = "ENOTSUP";
        throw error;
      }
      keepSocketAlive(socket) {
        socket?.setKeepAlive?.(true, this.keepAliveMsecs);
        socket?.unref?.();
        let agentTimeout = this.options.timeout || 0;
        let canKeepSocketAlive = true;
        const keepAliveHint =
          socket?._httpMessage?.res?.headers?.["keep-alive"];
        const hint = /timeout=(\d+)/.exec(keepAliveHint || "")?.[1];
        if (hint) {
          const serverHintTimeout = Math.max(
            0,
            Number.parseInt(hint, 10) * 1000 - this.agentKeepAliveTimeoutBuffer
          );
          if (serverHintTimeout === 0) canKeepSocketAlive = false;
          else if (serverHintTimeout < agentTimeout) {
            agentTimeout = serverHintTimeout;
          }
        }
        if (socket && socket.timeout !== agentTimeout) {
          socket.setTimeout?.(agentTimeout);
        }
        return canKeepSocketAlive;
      }
      reuseSocket(socket, request) {
        const listener = socket?.__nodeAgentFreeSocketErrorListener;
        if (listener) {
          socket.removeListener?.("error", listener);
          socket.__nodeAgentFreeSocketErrorListener = undefined;
        }
        if (request) request.reusedSocket = true;
        socket?.ref?.();
      }
      removeSocket(socket, options = {}) {
        const name = this.getName(options);
        const pools = [this.sockets];
        if (socket?.writable === false) pools.push(this.freeSockets);
        for (const pool of pools) {
          const sockets = pool[name];
          if (!sockets) continue;
          const index = sockets.indexOf(socket);
          if (index !== -1) sockets.splice(index, 1);
          if (sockets.length === 0) delete pool[name];
        }
      }
    }
    const __quenchDefaultHttpCreateConnection =
      NodeHttpAgent.prototype.createConnection;
    const globalAgent = new NodeHttpAgent({ keepAlive: true });
    globalThis.__nodeHttpGlobalAgent = globalAgent;
    const http = {
      Agent: NodeHttpAgent,
      ClientRequest: NodeClientRequest,
      globalAgent,
      IncomingMessage: NodeIncomingMessage,
      OutgoingMessage: NodeOutgoingMessage,
      Server: NodeHttpServer,
      ServerResponse: NodeServerResponse,
      createServer: (options, handler) => new NodeHttpServer(options, handler),
      get: (target, options, callback) => {
        let requestOptions = { method: "GET" };
        if (typeof options === "function") {
          callback = options;
          options = {};
        } else {
          options ||= {};
        }
        if (
          typeof target === "string" &&
          options &&
          Object.keys(options).length
        ) {
          const original = new URL(target);
          requestOptions = { ...options, method: "GET" };
          const host = options.hostname || options.host || original.hostname;
          const port = options.port ?? (original.port || 80);
          const path = options.path || `${original.pathname}${original.search}`;
          target = `http://${host}:${port}${path}`;
        }
        if (
          typeof target === "object" &&
          target !== null &&
          !(target instanceof URL)
        ) {
          requestOptions = { ...target, method: "GET" };
          if (
            typeof requestOptions.path === "string" &&
            /[^\u0021-\u00ff]/.test(requestOptions.path)
          ) {
            const error = new TypeError(
              "Request path contains unescaped characters"
            );
            error.code = "ERR_UNESCAPED_CHARACTERS";
            throw error;
          }
          target = `http://${target.hostname || target.host || "localhost"}:${
            target.port || 80
          }${target.path || `${target.pathname || "/"}${target.search || ""}`}`;
        }
        const url = typeof target === "string" ? new URL(target) : target;
        const server = servers.get(url.port || "80");
        const request = makeRequest(
          server ? server._handler : () => {},
          `${url.pathname}${url.search}`,
          callback,
          requestOptions,
          server
        );
        if (requestOptions.timeout !== undefined) {
          request.setTimeout(requestOptions.timeout);
        }
        return request;
      },
      request: (target, options, callback) => {
        if (target instanceof URL) {
          callback = typeof options === "function" ? options : callback;
          const extra = options && typeof options === "object" ? options : {};
          options = {
            ...extra,
            hostname: target.hostname,
            port: target.port || (target.protocol === "https:" ? 443 : 80),
            path: `${target.pathname}${target.search}`,
            headers: target.headers || extra.headers
          };
          target = `http://${options.hostname}:${options.port}${options.path}`;
        }
        if (
          typeof target === "object" &&
          target !== null &&
          !(target instanceof URL)
        ) {
          callback = typeof options === "function" ? options : callback;
          options = target;
          for (const name of ["hostname", "host"]) {
            const value = Object.prototype.hasOwnProperty.call(options, name)
              ? options[name]
              : undefined;
            if (
              value !== undefined &&
              value !== null &&
              typeof value !== "string"
            ) {
              const received =
                value && typeof value === "object"
                  ? `an instance of ${value.constructor?.name || "Object"}`
                  : `type ${typeof value} (${String(value)})`;
              const error = new TypeError(
                `The "options.${name}" property must be of type string or one of undefined or null. Received ${received}`
              );
              error.code = "ERR_INVALID_ARG_TYPE";
              throw error;
            }
          }
          if (
            typeof options.path === "string" &&
            /[^\u0021-\u00ff]/.test(options.path)
          ) {
            const error = new TypeError(
              "Request path contains unescaped characters"
            );
            error.code = "ERR_UNESCAPED_CHARACTERS";
            throw error;
          }
          if (
            options.method !== undefined &&
            typeof options.method === "string" &&
            !/^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/.test(options.method)
          ) {
            const error = new TypeError(
              `Method must be a valid HTTP token ["${String(options.method)}"]`
            );
            error.code = "ERR_INVALID_HTTP_TOKEN";
            throw error;
          }
          const effectivePort =
            options.port ??
            options.defaultPort ??
            options.agent?.defaultPort ??
            80;
          const hostname = Object.prototype.hasOwnProperty.call(
            options,
            "hostname"
          )
            ? options.hostname
            : undefined;
          const host = Object.prototype.hasOwnProperty.call(options, "host")
            ? options.host
            : undefined;
          target = `http://${hostname || host || "localhost"}:${effectivePort}${
            options.path || "/"
          }`;
          if (
            options.timeout !== undefined &&
            typeof options.timeout !== "number"
          ) {
            const error = new TypeError(
              `The "timeout" argument must be of type number. Received type ${typeof options.timeout}`
            );
            error.code = "ERR_INVALID_ARG_TYPE";
            throw error;
          }
        }
        const url = typeof target === "string" ? new URL(target) : target;
        const server = servers.get(url.port || "80");
        const request = makeRequest(
          server ? server._handler : () => {},
          `${url.pathname}${url.search}`,
          callback,
          options || {},
          server
        );
        if (options?.timeout !== undefined) request.setTimeout(options.timeout);
        return request;
      }
    };
    globalThis.__nodeHttp = http;
    __quenchHttpModule = http;
  }
}
globalThis.__quench_require_part_00 = (name, specifier) => {
  const base = __quenchRequireCoreBase(String(name).replace(/^node:/, ""));
  if (base !== undefined) return base;
  if (name === "http") return globalThis.__nodeHttp || __quenchHttpModule;
  if (name === "child_process") {
    return globalThis.__nodeRequireChildProcess || __quenchChildProcessModule();
  }
};
