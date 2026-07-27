// Self-hosted GeneratorFunction.prototype on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// GeneratorFunction.prototype inherits from Function.prototype.
// No spec-defined prototype methods require native forwarding;
// all generator iteration methods live on %GeneratorPrototype%.
// Set @@toStringTag per ES2025 §20.2.2.3.
// Note: simple assignment is used because Object.defineProperty with
// Symbol keys is not yet supported in this runtime.
GeneratorFunction.prototype[Symbol.toStringTag] = "GeneratorFunction";
