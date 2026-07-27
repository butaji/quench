// Self-hosted AsyncFunction prototype properties on top of __ops__
// %AsyncFunctionPrototype% inherits call/apply/bind/toString from
// Function.prototype.  The only spec‑required own property is @@toStringTag.

var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// ES2025 §20.2.4: %AsyncFunctionPrototype%[@@toStringTag] = "AsyncFunction"
// TODO: Use Object.defineProperty for spec-correct non-writable/non-enumerable
// flags once symbol-keyed defineProperty is fixed for plain objects.
AsyncFunction.prototype[Symbol.toStringTag] = "AsyncFunction";
