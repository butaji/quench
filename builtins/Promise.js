// Self-hosted Promise prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

var _nativeThen = Promise.prototype.then;
var _nativeCatch = Promise.prototype.catch;
var _nativeFinally = Promise.prototype.finally;

// Promise.prototype.then (ES2025 §27.2.5.4)
Promise.prototype.then = function PromiseThen(onFulfilled, onRejected) {
  if (this === null || this === undefined) throw ThrowTypeError("Promise.prototype.then called on null or undefined");
  return _nativeThen.call(this, onFulfilled, onRejected);
};

// Promise.prototype.catch (ES2025 §27.2.5.2)
Promise.prototype.catch = function PromiseCatch(onRejected) {
  if (this === null || this === undefined) throw ThrowTypeError("Promise.prototype.catch called on null or undefined");
  return _nativeCatch.call(this, onRejected);
};

// Promise.prototype.finally (ES2025 §27.2.5.3)
Promise.prototype.finally = function PromiseFinally(onFinally) {
  if (this === null || this === undefined) throw ThrowTypeError("Promise.prototype.finally called on null or undefined");
  return _nativeFinally.call(this, onFinally);
};
