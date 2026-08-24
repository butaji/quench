"use strict";

const hooks = globalThis.__quenchAsyncHooks || [];
Object.defineProperty(globalThis, "__quenchAsyncHooks", { configurable: true, value: hooks });
if (!globalThis.__nodeCurrentAsyncResource) {
  Object.defineProperty(globalThis, "__nodeCurrentAsyncResource", {
    configurable: true,
    writable: true,
    value: { asyncId: 1, triggerAsyncId: 0 }
  });
}
Object.defineProperty(globalThis, "\0quench:process_next_tick_init", {
  configurable: true,
  value: () => {
  const resource = { asyncId: ++globalThis.__quenchAsyncId, triggerAsyncId: 1 };
  for (const hook of hooks) {
    if (hook.callbacks && typeof hook.callbacks.init === "function") {
      hook.callbacks.init(resource.asyncId, "TickObject", 1, resource);
    }
  }
  return resource;
  }
});
let nextId = globalThis.__quenchAsyncId || 1;
Object.defineProperty(globalThis, "__quenchAsyncId", { configurable: true, writable: true, value: nextId });
Object.defineProperty(globalThis, "__quenchAsyncInit", {
  configurable: true,
  value(type, resource) {
    const asyncId = ++globalThis.__quenchAsyncId;
    resource.asyncId = asyncId;
    resource.triggerAsyncId = 1;
    for (const hook of hooks) {
      if (hook.callbacks && typeof hook.callbacks.init === "function") {
        hook.callbacks.init.call(hook, asyncId, type, 1, resource);
      }
    }
    return resource;
  }
});
Object.defineProperty(globalThis, "__quenchAsyncDestroy", {
  configurable: true,
  value(resource) {
    for (const hook of hooks) {
      if (hook.callbacks && typeof hook.callbacks.destroy === "function") {
        hook.callbacks.destroy.call(hook, resource.asyncId);
      }
    }
  }
});


function createHook(callbacks = {}) {
  for (const name of ["init", "before", "after", "destroy", "promiseResolve"]) {
    if (callbacks[name] !== undefined && typeof callbacks[name] !== "function") {
      throw Object.assign(new TypeError(`hook.${name} must be a function`), { code: "ERR_ASYNC_CALLBACK" });
    }
  }
  const hook = {
    callbacks,
    enabled: false,
    enable() {
      if (!this.enabled) {
        this.enabled = true;
        hooks.push(this);
      }
      return this;
    },
    disable() {
      this.enabled = false;
      const index = hooks.indexOf(this);
      if (index >= 0) hooks.splice(index, 1);
      return this;
    }
  };
  return hook;
}

if (!globalThis.__quenchPromiseHooksPatched) {
  const NativePromise = globalThis.Promise;
  const NativePromisePrototype = NativePromise.prototype;
  const emitPromiseInit = (resource) => {
    resource.asyncId = ++globalThis.__quenchAsyncId;
    resource.triggerAsyncId = 1;
    for (const hook of hooks) {
      if (hook.callbacks && typeof hook.callbacks.init === "function") {
        hook.callbacks.init(resource.asyncId, "PROMISE", 1, resource);
      }
    }
  };
  const PromiseWithHooks = function (executor) {
    const resource = {};
    emitPromiseInit(resource);
    return new NativePromise(executor);
  };
  PromiseWithHooks.prototype = NativePromisePrototype;
  Object.setPrototypeOf(PromiseWithHooks, NativePromise);
  globalThis.Promise = PromiseWithHooks;
  Object.defineProperty(globalThis, "__quenchPromiseHooksPatched", {
    configurable: true,
    value: true,
  });
}

class AsyncResource {
  constructor(type, options = {}) {
    if (type === undefined) {
      throw Object.assign(new TypeError('The "type" argument must be specified'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    if (String(type).length === 0) {
      throw Object.assign(new TypeError("Invalid asyncId type"), { code: "ERR_ASYNC_TYPE" });
    }
    if (typeof options === "number" && (!Number.isInteger(options) || options < 0)) {
      throw Object.assign(new RangeError("Invalid asyncId"), { code: "ERR_INVALID_ASYNC_ID" });
    }
    this.type = String(type);
    this._asyncId = ++nextId;
    this._triggerAsyncId = typeof options === "number" ? options : (options.triggerAsyncId || 0);
    this._resource = { asyncId: this._asyncId, triggerAsyncId: this._triggerAsyncId };
  }
  asyncId() { return this._asyncId; }
  triggerAsyncId() { return this._triggerAsyncId; }
  runInAsyncScope(callback, thisArg, ...args) {
    if (typeof callback !== "function") throw new TypeError("The callback argument must be of type function");
    const previous = globalThis.__nodeCurrentAsyncResource;
    globalThis.__nodeCurrentAsyncResource = this._resource;
    try { return callback.apply(thisArg, args); }
    finally { globalThis.__nodeCurrentAsyncResource = previous; }
  }
  bind(callback, thisArg) { return (...args) => this.runInAsyncScope(callback, thisArg, ...args); }
  emitDestroy() { return this; }
  static bind(callback, thisArg) { return (...args) => callback.apply(thisArg, args); }
}

class AsyncLocalStorage {
  constructor(options = {}) { this.defaultValue = options.defaultValue; this.store = undefined; }
  disable() { this.store = undefined; }
  getStore() { return this.store === undefined ? this.defaultValue : this.store; }
  run(store, callback, ...args) {
    const previous = this.store;
    this.store = store;
    try { return callback(...args); }
    finally { this.store = previous; }
  }
  enterWith(store) { this.store = store; }
  exit(callback, ...args) {
    const previous = this.store;
    this.store = undefined;
    try { return callback(...args); }
    finally { this.store = previous; }
  }
  withScope(store) {
    const previous = this.store;
    this.store = store;
    let disposed = false;
    const restore = () => {
      if (disposed) return;
      disposed = true;
      this.store = previous;
    };
    return { dispose: restore, [Symbol.dispose]: restore };
  }
}

module.exports = {
  createHook,
  AsyncResource,
  AsyncLocalStorage,
  executionAsyncId: () => globalThis.__nodeCurrentAsyncResource?.asyncId || 1,
  triggerAsyncId: () => globalThis.__nodeCurrentAsyncResource?.triggerAsyncId || 0,
  executionAsyncResource: () => globalThis.__nodeCurrentAsyncResource
};
