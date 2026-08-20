// Minimal async_hooks factory (parses).
(function (deps) {
  'use strict';
  function AsyncResource(type) { this.type_ = type; }
  AsyncResource.prototype.runInAsyncScope = function (fn, thisArg) {
    return fn.apply(thisArg, Array.prototype.slice.call(arguments, 2));
  };
  AsyncResource.bind = function (fn, thisArg) {
    return function () {
      var r = new AsyncResource(fn.name || 'bound');
      return r.runInAsyncScope(fn, thisArg, arguments);
    };
  };
  return { AsyncResource: AsyncResource };
});
