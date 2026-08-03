// Self-hosted Symbol prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;
var _symbolToString = ops.SymbolToString;
var _symbolValueOf = ops.SymbolValueOf;
var _symbolDescription = Symbol.prototype.__description;

// Symbol.prototype.toString (ES2025 §20.4.3.3)
Symbol.prototype.toString = function SymbolToString() {
  if (this === null || this === undefined) throw ThrowTypeError("Symbol.prototype.toString called on null or undefined");
  return _symbolToString(this);
};

// Symbol.prototype.valueOf (ES2025 §20.4.3.4)
Symbol.prototype.valueOf = function SymbolValueOf() {
  if (this === null || this === undefined) throw ThrowTypeError("Symbol.prototype.valueOf called on null or undefined");
  return _symbolValueOf(this);
};

Symbol.prototype.description = function SymbolDescription() {
  if (this === null || this === undefined) throw ThrowTypeError("Symbol.prototype.description called on null or undefined");
  return _symbolDescription.call(this);
};

// Symbol.prototype[Symbol.toPrimitive] (ES2025 §20.4.3.2)
Symbol.prototype[Symbol.toPrimitive] = function SymbolToPrimitive(hint) {
  if (this === null || this === undefined) throw ThrowTypeError("Symbol.prototype[@@toPrimitive] called on null or undefined");
  return _symbolValueOf(this);
};
