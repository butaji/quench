//! Polyfill: `core-head`

pub const JS: &str = quench_js_check::checked_js!(r#"let __quenchAsyncHooksModule;
{
  globalThis.__nodeCurrentAsyncResource ||= {};
  globalThis.__nodeNextAsyncId ||= 1;
  globalThis.__nodeAsyncHooks ||= new Set();
  globalThis.__nodePromiseResources ||= new WeakMap();
  globalThis.__nodePromiseResourceList ||= new Set();
  class AsyncResource {
    constructor(type, options = {}) {
      if (type === undefined) {
        throw Object.assign(new TypeError('The "type" argument must be specified'), { code: "ERR_INVALID_ARG_TYPE" });
      }
      if (String(type).length === 0) {
        throw Object.assign(new TypeError("Invalid asyncId type"), { code: "ERR_ASYNC_TYPE" });
      }
      if (
        typeof options === "number" &&
        (!Number.isInteger(options) || options < 0)
      ) {
        throw Object.assign(new RangeError("Invalid asyncId"), { code: "ERR_INVALID_ASYNC_ID" });
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
        throw Object.assign(new TypeError("The callback argument must be of type function"), { code: "ERR_INVALID_ARG_TYPE" });
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
        throw Object.assign(new TypeError("The callback argument must be of type function"), { code: "ERR_INVALID_ARG_TYPE" });
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
        throw Object.assign(new TypeError('The "fn" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
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
        throw Object.assign(new TypeError('The "fn" argument must be of type function'), { code: "ERR_INVALID_ARG_TYPE" });
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
      for (const name of "init before after destroy promiseResolve".split(
        " "
      )) {
        if (
          callbacks[name] !== undefined &&
          typeof callbacks[name] !== "function"
        ) {
          throw Object.assign(new TypeError(`hook.${name} must be a function`), { code: "ERR_ASYNC_CALLBACK" });
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
"#);
