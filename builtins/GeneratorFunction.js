// Self-hosted GeneratorFunction.prototype on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;
var DefineProp = ops.DefineProp;

DefineProp(GeneratorFunction.prototype, Symbol.toStringTag, {
  value: "GeneratorFunction",
  writable: false,
  enumerable: false,
  configurable: true
});
DefineProp(GeneratorFunction.prototype, "constructor", {
  value: GeneratorFunction,
  writable: false,
  enumerable: false,
  configurable: true
});
DefineProp(GeneratorFunction.prototype, "prototype", {
  value: GeneratorFunction.prototype,
  writable: false,
  enumerable: false,
  configurable: true
});
