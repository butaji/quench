// Synchronous async_hooks compatibility.
(function (deps) {
  'use strict';
  var current = 0, stores = {};
  function AsyncResource(type) { this.type_ = type; this.id_ = ++current; }
  AsyncResource.prototype.runInAsyncScope = function (fn, thisArg) {
    var args = Array.prototype.slice.call(arguments, 2), prev = current;
    current = this.id_; try { return fn.apply(thisArg, args); } finally { current = prev; }
  };
  AsyncResource.bind = function (fn, thisArg) { return function () { return fn.apply(thisArg, arguments); }; };
  function createHook(opts) {
    opts = opts || {}; var enabled = false;
    return { enable: function () { enabled = true; return this; }, disable: function () { enabled = false; return this; }, _enabled: function () { return enabled; } };
  }
  function AsyncLocalStorage() { this.store = undefined; }
  AsyncLocalStorage.prototype.run = function (store, fn) {
    var id = current, old = stores[id]; stores[id] = store;
    try { return fn(); } finally { stores[id] = old; }
  };
  AsyncLocalStorage.prototype.getStore = function () { return stores[current]; };
  return { AsyncResource: AsyncResource, createHook: createHook,
    executionAsyncId: function () { return current; }, triggerAsyncId: function () { return current; },
    AsyncLocalStorage: AsyncLocalStorage };
});
