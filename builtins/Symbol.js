// Self-hosted Symbol prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations
var _nativeToString = Symbol.prototype.toString;
var _nativeValueOf = Symbol.prototype.valueOf;

// Symbol.prototype.toString (ES2025 §20.4.3.3)
Symbol.prototype.toString = function SymbolToString() {
  if (this === null || this === undefined) throw ThrowTypeError("Symbol.prototype.toString called on null or undefined");
  return _nativeToString.call(this);
};

// Symbol.prototype.valueOf (ES2025 §20.4.3.4)
Symbol.prototype.valueOf = function SymbolValueOf() {
  if (this === null || this === undefined) throw ThrowTypeError("Symbol.prototype.valueOf called on null or undefined");
  return _nativeValueOf.call(this);
};

// Symbol.prototype[Symbol.toPrimitive] (ES2025 §20.4.3.2)
Symbol.prototype[Symbol.toPrimitive] = function SymbolToPrimitive(hint) {
  if (this === null || this === undefined) throw ThrowTypeError("Symbol.prototype[@@toPrimitive] called on null or undefined");
  return _nativeValueOf.call(this);
};
