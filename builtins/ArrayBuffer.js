// Self-hosted ArrayBuffer prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations
var _nativeSlice = ArrayBuffer.prototype.__slice;

// ArrayBuffer.prototype.slice (ES2025 §25.1.5.4)
ArrayBuffer.prototype.slice = function ArrayBufferSlice(begin, end) {
  if (this === null || this === undefined) throw ThrowTypeError("ArrayBuffer.prototype.slice called on null or undefined");
  return _nativeSlice.apply(this, arguments);
};
