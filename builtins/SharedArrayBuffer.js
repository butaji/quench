// Self-hosted SharedArrayBuffer prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native slice from ArrayBuffer.prototype (same algorithm per ES2025 §25.2.3.4)
var _nativeSlice = ArrayBuffer.prototype.slice;

// SharedArrayBuffer.prototype.slice (ES2025 §25.2.3.4)
SharedArrayBuffer.prototype.slice = function SharedArrayBufferSlice(start, end) {
  if (this === null || this === undefined) throw ThrowTypeError("SharedArrayBuffer.prototype.slice called on null or undefined");
  return _nativeSlice.call(this, start, end);
};

// SharedArrayBuffer.prototype[@@toStringTag] (ES2025 §25.2.3.5)
Object.defineProperty(SharedArrayBuffer.prototype, Symbol.toStringTag, {
  value: "SharedArrayBuffer",
  writable: false,
  enumerable: false,
  configurable: true,
});
