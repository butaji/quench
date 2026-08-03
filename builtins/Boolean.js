// Self-hosted Boolean prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

function BooleanValue(value) {
  if (typeof value === 'boolean') return value;
  if (value !== null && typeof value === 'object' && typeof value._value === 'boolean') return value._value;
  throw ThrowTypeError("Boolean.prototype called on incompatible receiver");
}

// Boolean.prototype.toString (ES2025 §20.3.3.2)
Boolean.prototype.toString = function BooleanToString() {
  if (this === null || this === undefined) throw ThrowTypeError("Boolean.prototype.toString called on null or undefined");
  return BooleanValue(this) ? 'true' : 'false';
};

// Boolean.prototype.valueOf (ES2025 §20.3.3.3)
Boolean.prototype.valueOf = function BooleanValueOf() {
  if (this === null || this === undefined) throw ThrowTypeError("Boolean.prototype.valueOf called on null or undefined");
  return BooleanValue(this);
};
