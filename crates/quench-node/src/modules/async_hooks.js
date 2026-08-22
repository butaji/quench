// Synchronous async_hooks compatibility.
(function (deps) {
  'use strict';
  var current = 0;
  function AsyncResource(type) { this.type_ = type; this.id_ = ++current; this.triggerId_ = current - 1; }
  AsyncResource.prototype.asyncId = function () { return this.id_; };
  AsyncResource.prototype.triggerAsyncId = function () { return this.triggerId_; };
  AsyncResource.prototype.emitDestroy = function () { return this; };
  AsyncResource.prototype.runInAsyncScope = function (fn, thisArg) {
    var args = Array.prototype.slice.call(arguments, 2), prev = current;
    current = this.id_; try { return fn.apply(thisArg, args); } finally { current = prev; }
  };
  AsyncResource.bind = function (fn, thisArg) {
    return function () { return fn.apply(thisArg, arguments); };
  };
  AsyncResource.prototype.bind = function (fn, thisArg) {
    var resource = this;
    return function () { return resource.runInAsyncScope(fn, thisArg || this, ...arguments); };
  };
  function createHook(opts) {
    opts = opts || {}; var enabled = false;
    return { enable: function () { enabled = true; return this; }, disable: function () { enabled = false; return this; }, _enabled: function () { return enabled; } };
  }
  function AsyncLocalStorage() {
    this._stack = [];
    this._defaultValue = undefined;
  }
  AsyncLocalStorage.prototype.run = function (store, fn) {
    if (typeof fn !== 'function') {
      throw new TypeError('The "callback" argument must be of type function');
    }
    this._stack.push(store);
    try { return fn(); }
    finally { this._stack.pop(); }
  };
  AsyncLocalStorage.prototype.getStore = function () {
    return this._stack.length ? this._stack[this._stack.length - 1] : this._defaultValue;
  };
  return { AsyncResource: AsyncResource, createHook: createHook,
    executionAsyncId: function () { return current; }, triggerAsyncId: function () { return current; },
    AsyncLocalStorage: AsyncLocalStorage };
});
