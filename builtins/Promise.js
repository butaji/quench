// Self-hosted Promise prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

var _nativeThen = Promise.prototype.then;
var _nativeResolve = Promise.__resolve;
var _nativeReject = Promise.__reject;
var _nativeAll = Promise.__all;
var _nativeRace = Promise.__race;

Promise.resolve = function PromiseResolve(value) {
  return _nativeResolve.call(this, value);
};

Promise.reject = function PromiseReject(reason) {
  return _nativeReject.call(this, reason);
};

Promise.all = function PromiseAll(iterable) {
  return _nativeAll.call(this, iterable);
};

Promise.race = function PromiseRace(iterable) {
  return _nativeRace.call(this, iterable);
};

// Promise.prototype.then (ES2025 §27.2.5.4)
Promise.prototype.then = function PromiseThen(onFulfilled, onRejected) {
  if (this === null || this === undefined) throw ThrowTypeError("Promise.prototype.then called on null or undefined");
  return _nativeThen.call(this, onFulfilled, onRejected);
};

// Promise.prototype.catch (ES2025 §27.2.5.2)
Promise.prototype.catch = function PromiseCatch(onRejected) {
  if (this === null || this === undefined) throw ThrowTypeError("Promise.prototype.catch called on null or undefined");
  return _nativeThen.call(this, undefined, onRejected);
};

// Promise.prototype.finally (ES2025 §27.2.5.3)
Promise.prototype.finally = function PromiseFinally(onFinally) {
  if (this === null || this === undefined) throw ThrowTypeError("Promise.prototype.finally called on null or undefined");
  var isCallable = typeof onFinally === 'function';
  var thenFinally = function(value) {
    var result = isCallable ? onFinally() : onFinally;
    return Promise.resolve(result).then(function() { return value; });
  };
  var catchFinally = function(reason) {
    var result = isCallable ? onFinally() : onFinally;
    return Promise.resolve(result).then(function() { throw reason; });
  };
  return _nativeThen.call(this, thenFinally, catchFinally);
};
