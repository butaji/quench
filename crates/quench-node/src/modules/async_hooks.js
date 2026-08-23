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
  function AsyncLocalStorage(options) {
    options = options || {}; this._store = undefined;
    this._defaultValue = options.defaultValue; this._disabled = false; this._exitDepth = 0;
  }
  AsyncLocalStorage.prototype.run = function (store, fn) {
    var args = Array.prototype.slice.call(arguments, 2);
    if (typeof fn !== 'function') throw new TypeError('The "callback" argument must be of type function');
    if (this._disabled) return fn.apply(undefined, args);
    var previous = this._store; this._store = store;
    try { return fn.apply(undefined, args); } finally { this._store = previous; }
  };
  AsyncLocalStorage.prototype.enterWith = function (store) { if (!this._disabled && this._exitDepth === 0) this._store = store; };
  AsyncLocalStorage.prototype.disable = function () { this._disabled = true; this._store = undefined; };
  AsyncLocalStorage.prototype.getStore = function () {
    return this._disabled ? undefined : (this._exitDepth > 0 ? this._defaultValue : (this._store === undefined ? this._defaultValue : this._store));
  };
  AsyncLocalStorage.prototype.exit = function (fn) {
    if (typeof fn !== 'function') throw new TypeError('The "callback" argument must be of type function');
    var previous = this._store; this._exitDepth += 1;
    try { return fn(); } finally { this._exitDepth -= 1; this._store = previous; }
  };
  AsyncLocalStorage.prototype.bind = function (fn) {
    var storage = this;
    var captured = storage._store;
    return function () {
      var previous = storage._store;
      if (!storage._disabled) storage._store = captured;
      try { return fn.apply(undefined, arguments); }
      finally { storage._store = previous; }
    };
  };
  AsyncLocalStorage.prototype.snapshot = function () {
    var storage = this;
    var captured = storage._store;
    return function (fn) {
      var args = Array.prototype.slice.call(arguments, 1);
      var previous = storage._store;
      if (!storage._disabled) storage._store = captured;
      try { return fn.apply(undefined, args); }
      finally { storage._store = previous; }
    };
  };
  AsyncLocalStorage.bind = function (fn) {
    var storage = this;
    var captured = storage._store;
    return function () {
      var previous = storage._store;
      if (!storage._disabled) storage._store = captured;
      try { return fn.apply(undefined, arguments); }
      finally { storage._store = previous; }
    };
  };
  AsyncLocalStorage.snapshot = function () {
    var storage = this;
    var captured = storage._store;
    return function (fn) {
      var args = Array.prototype.slice.call(arguments, 1);
      var previous = storage._store;
      if (!storage._disabled) storage._store = captured;
      try { return fn.apply(undefined, args); }
      finally { storage._store = previous; }
    };
  };
  return { AsyncResource: AsyncResource, createHook: createHook,
    executionAsyncId: function () { return current; }, triggerAsyncId: function () { return current; },
    enabledHooksExist: function () { return false; }, AsyncLocalStorage: AsyncLocalStorage };
});
