// Self-hosted Boolean prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Boolean.prototype.toString (ES2025 §20.3.3.2)
Boolean.prototype.toString = function BooleanToString() {
  if (this === null || this === undefined) throw ThrowTypeError("Boolean.prototype.toString called on null or undefined");
  return this === false ? 'false' : 'true';
};

// Boolean.prototype.valueOf (ES2025 §20.3.3.3)
Boolean.prototype.valueOf = function BooleanValueOf() {
  if (this === null || this === undefined) throw ThrowTypeError("Boolean.prototype.valueOf called on null or undefined");
  return this === false ? false : true;
};
