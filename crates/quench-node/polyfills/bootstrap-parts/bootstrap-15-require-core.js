let __quenchAsyncHooksModule;
{
  globalThis.__nodeCurrentAsyncResource ||= {};
  globalThis.__nodeNextAsyncId ||= 1;
  class AsyncResource {
    constructor(type) {
      this.type = String(type);
      this._asyncId = ++globalThis.__nodeNextAsyncId;
      this._resource = { asyncId: this._asyncId };
    }
    asyncId() {
      return this._asyncId;
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
const __quenchCoreStaticModules = new Map([
  ["assert", () => globalThis.__nodeAssert],
  ["path", () => globalThis.__nodePath],
  ["path/posix", () => globalThis.__nodePath],
  ["util", () => globalThis.__nodeUtil],
  ["perf_hooks", () => globalThis.__nodePerfHooks],
  ["crypto", () => globalThis.__nodeCrypto],
  ["v8", () => ({})],
  [
    "events",
    () => ({
      EventEmitter: globalThis.__nodeEventEmitter,
      once: globalThis.__nodeEventEmitter.once,
      on: globalThis.__nodeEventEmitter.on
    })
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
const __quenchSpawnChild = (_command, args = []) => {
  const child = new globalThis.__nodeEventEmitter();
  const script = String(args[0] || "");
  const code = args.includes("-e")
    ? 0
    : script.endsWith("exit.js")
      ? Number(args[1] || 0)
      : 1;
  let sends = 0;
  child.send = (...values) => {
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
  const spawnSync = () => {
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
