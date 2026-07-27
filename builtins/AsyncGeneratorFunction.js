// Self-hosted AsyncGeneratorFunction on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native Function.prototype.toString for forwarding
var _nativeToString = Function.prototype.toString;

// AsyncGeneratorFunction.prototype.toString (ES2025 §27.7.2.2)
AsyncGeneratorFunction.prototype.toString = function AsyncGeneratorToString() {
  if (typeof this !== 'function') throw ThrowTypeError("AsyncGeneratorFunction.prototype.toString called on non-function");
  return _nativeToString.call(this);
};

// %AsyncGeneratorFunctionPrototype%[@@toStringTag] = "AsyncGeneratorFunction" (ES2025 §27.7.2.6)
Object.defineProperty(AsyncGeneratorFunction.prototype, Symbol.toStringTag, {
  value: "AsyncGeneratorFunction",
  writable: false,
  enumerable: false,
  configurable: true
});
