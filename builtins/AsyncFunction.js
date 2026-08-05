// Self-hosted AsyncFunction prototype properties on top of __ops__
// %AsyncFunctionPrototype% inherits call/apply/bind/toString from
// Function.prototype.  The only spec‑required own property is @@toStringTag.

var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;
var DefineProp = ops.DefineProp;

DefineProp(AsyncFunction.prototype, Symbol.toStringTag, {
  value: "AsyncFunction",
  writable: false,
  enumerable: false,
  configurable: true
});
DefineProp(AsyncFunction.prototype, "constructor", {
  value: AsyncFunction,
  writable: false,
  enumerable: false,
  configurable: true
});
