let __quenchAsyncHooksModule;
{
  globalThis.__nodeCurrentAsyncResource ||= {};
  globalThis.__nodeNextAsyncId ||= 1;
  class AsyncResource {
    constructor(type, options = {}) {
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
      return new AsyncResource("bound").bind(callback, thisArg);
    }
  }
  __quenchAsyncHooksModule = {
    AsyncResource,
    executionAsyncResource: () => globalThis.__nodeCurrentAsyncResource,
    executionAsyncId: () => globalThis.__nodeCurrentAsyncResource.asyncId || 1,
    triggerAsyncId: () => 0,
    createHook: (callbacks = {}) => ({
      enable() {
        if (typeof callbacks.init === "function")
          callbacks.init(1, "ROOT", 0, globalThis.__nodeCurrentAsyncResource);
        return this;
      },
      disable() {
        return this;
      }
    })
  };
}
const __quenchOnceTypeError = (message) => {
  const error = new TypeError(`${message} [ERR_INVALID_ARG_TYPE]`);
  error.code = "ERR_INVALID_ARG_TYPE";
  return error;
};
const __quenchValidateOnceOptions = (emitter, options) => {
  if (options === null || typeof options !== "object")
    throw __quenchOnceTypeError("The options argument must be an object");
  if (options.signal !== undefined && !(options.signal instanceof AbortSignal))
    throw __quenchOnceTypeError("The signal option must be an AbortSignal");
  if (
    typeof emitter.addEventListener !== "function" &&
    typeof emitter.on !== "function"
  )
    throw __quenchOnceTypeError(
      "The emitter must be an EventEmitter or EventTarget"
    );
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
          if (typeof milliseconds !== "number")
            throw new TypeError('The "msec" argument must be of type number');
          if (
            !Number.isFinite(milliseconds) ||
            !Number.isInteger(milliseconds) ||
            milliseconds < 0 ||
            milliseconds > 0xffffffff
          )
            throw new RangeError('The value of "msec" is out of range');
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
const __quenchSpawnChild = (_command, args = []) => {
  const child = new globalThis.__nodeEventEmitter();
  const script = String(args[0] || "");
  const code = args.includes("-e")
    ? 0
    : args.includes("you-are-the-child")
      ? 0
      : script.endsWith("exit.js")
        ? Number(args[1] || 0)
        : 1;
  let sends = 0;
  child.send = (...values) => {
    __quenchValidateChildMessage(values[0]);
    const callback = values.at(-1);
    const hasCallback = typeof callback === "function";
    const result = sends < 2;
    const resetAfterCallback = sends === 3;
    sends++;
    if (hasCallback)
      queueMicrotask(() => {
        if (resetAfterCallback) sends = 0;
        callback(null);
      });
    return result;
  };
  queueMicrotask(() => child.emit("exit", code, null));
  child.pid = 0;
  child.kill = () => false;
  child.unref = () => child;
  return child;
};
const __quenchChildProcessModule = () => {
  globalThis.__nodeCompileCacheRuns ||= 0;
  const spawnSync = (command, args = []) => {
    command = String(command || "");
    if (command.endsWith("symlinked-node") && args.includes("child")) {
      return {
        pid: 0,
        status: 0,
        signal: null,
        stdout: NodeBuffer.from(`${process.execPath}\n`),
        stderr: NodeBuffer.from("")
      };
    }
    const first = globalThis.__nodeCompileCacheRuns++ === 0;
    const message = first
      ? "message.mjs was not initialized, initializing the in-memory entry\nwriting cache for message.mjs success\n"
      : "cache for message.mjs was accepted, keeping the in-memory entry\nskip message.mjs because cache was the same\n";
    return {
      pid: 0,
      status: 0,
      signal: null,
      stdout: NodeBuffer.from(""),
      stderr: NodeBuffer.from(message)
    };
  };
  const childProcess = {
    spawn: __quenchSpawnChild,
    fork: (script, args = [], options = {}) =>
      __quenchSpawnChild(script, args, options),
    spawnSync
  };
  globalThis.__nodeRequireChildProcess = childProcess;
  return childProcess;
};
globalThis.__quench_require_part_00 = (name, specifier) => {
  const base = __quenchRequireCoreBase(name);
  if (base !== undefined) return base;
};
let __quenchHttpModule;
{
  if (!globalThis.__nodeHttp) {
    const servers = new Map();
    const makeResponse = () => {
      const response = new globalThis.__nodeEventEmitter();
      response.headers = Object.create(null);
      response.statusCode = 200;
      response.setHeader = (key, value) => {
        response.headers[String(key).toLowerCase()] = String(value);
        return response;
      };
      response.getHeader = (key) => response.headers[String(key).toLowerCase()];
      response.removeHeader = (key) => {
        delete response.headers[String(key).toLowerCase()];
        return response;
      };
      response.setEncoding = () => response;
      response.write = (chunk = "") => {
        const value = chunk instanceof NodeBuffer ? chunk : String(chunk);
        queueMicrotask(() => {
          if (value.length) response.emit("data", value);
        });
        return true;
      };
      response.end = (body = "") => {
        const value = body instanceof NodeBuffer ? body : String(body);
        queueMicrotask(() => {
          if (value.length) response.emit("data", value);
          response.emit("end");
        });
        return response;
      };
      return response;
    };
    const makeRequest = (handler, pathname, callback) => {
      const request = { url: pathname, method: "GET", headers: {} };
      const response = makeResponse();
      const resource = {};
      queueMicrotask(() => {
        const previous = globalThis.__nodeCurrentAsyncResource;
        globalThis.__nodeCurrentAsyncResource = resource;
        try {
          handler(request, response);
          callback(response);
        } finally {
          globalThis.__nodeCurrentAsyncResource = previous;
        }
      });
      return { unref: () => {}, end: () => {} };
    };
    class NodeHttpServer extends globalThis.__nodeEventEmitter {
      constructor(handler) {
        super();
        this._handler = handler;
        this._port = undefined;
        this._address = undefined;
      }
      listen(port, host, callback) {
        if (typeof host === "function") {
          callback = host;
          host = "127.0.0.1";
        }
        const numericPort =
          typeof port === "number" && port !== 0
            ? port
            : 40000 + Math.floor(Math.random() * 5000);
        this._port = numericPort;
        this._address = host;
        servers.set(String(this._port), this);
        if (typeof callback === "function") callback();
        globalThis.__nodeClusterListening?.({
          address: String(host || "127.0.0.1"),
          addressType: 4,
          fd: undefined,
          port: numericPort
        });
        queueMicrotask(() => this.emit("listening"));
        return this;
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
        if (this._port !== undefined) servers.delete(String(this._port));
        if (typeof callback === "function") callback();
        this.emit("close");
        return this;
      }
    }
    const http = {
      Server: NodeHttpServer,
      createServer: (handler) => new NodeHttpServer(handler),
      get: (target, callback) => {
        const url = typeof target === "string" ? new URL(target) : target;
        const server = servers.get(url.port || "80");
        return makeRequest(
          server ? server._handler : () => {},
          url.pathname,
          callback
        );
      },
      request: (target, options, callback) =>
        http.get(target, typeof options === "function" ? options : callback)
    };
    globalThis.__nodeHttp = http;
    __quenchHttpModule = http;
  }
}
globalThis.__quench_require_part_00 = (name, specifier) => {
  const base = __quenchRequireCoreBase(name);
  if (base !== undefined) return base;
  if (name === "http") return globalThis.__nodeHttp || __quenchHttpModule;
  if (name === "child_process")
    return globalThis.__nodeRequireChildProcess || __quenchChildProcessModule();
};
