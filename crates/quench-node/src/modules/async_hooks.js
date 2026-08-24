"use strict";

const hooks = globalThis.__quenchAsyncHooks || [];
Object.defineProperty(globalThis, "__quenchAsyncHooks", { configurable: true, value: hooks });
let nextId = globalThis.__quenchAsyncId || 1;
Object.defineProperty(globalThis, "__quenchAsyncId", { configurable: true, writable: true, value: nextId });

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

class AsyncResource {
  constructor(type, options = {}) {
    if (type === undefined) {
      throw Object.assign(new TypeError('The "type" argument must be specified'), { code: "ERR_INVALID_ARG_TYPE" });
    }
    this.type = String(type);
    this._asyncId = ++nextId;
    this._triggerAsyncId = typeof options === "number" ? options : (options.triggerAsyncId || 0);
    this._resource = { asyncId: this._asyncId };
  }
  asyncId() { return this._asyncId; }
  triggerAsyncId() { return this._triggerAsyncId; }
  runInAsyncScope(callback, thisArg, ...args) {
    if (typeof callback !== "function") throw new TypeError("The callback argument must be of type function");
    return callback.apply(thisArg, args);
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
    try { return callback(...args); } finally { this.store = previous; }
  }
  enterWith(store) { this.store = store; }
  exit(callback, ...args) {
    const previous = this.store;
    this.store = undefined;
    try { return callback(...args); } finally { this.store = previous; }
  }
}

module.exports = {
  createHook,
  AsyncResource,
  AsyncLocalStorage,
  executionAsyncId: () => 1,
  triggerAsyncId: () => 0,
  executionAsyncResource: () => ({})
};
