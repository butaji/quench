// Self-hosted GeneratorPrototype methods on top of __ops__
// %GeneratorPrototype% is GeneratorFunction.prototype.prototype
var GeneratorPrototype = GeneratorFunction.prototype.prototype;
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations
var _nativeNext = GeneratorPrototype.__next;
var _nativeReturn = GeneratorPrototype.__return;
var _nativeThrow = GeneratorPrototype.__throw;
var _nativeIterator = GeneratorPrototype[Symbol.iterator];

// GeneratorPrototype.next (ES2025 §27.4.1.2)
GeneratorPrototype.next = function GeneratorNext(value) {
  if (this === null || this === undefined) throw ThrowTypeError("Generator.prototype.next called on null or undefined");
  return _nativeNext.call(this, value);
};

// GeneratorPrototype.return (ES2025 §27.4.1.3)
GeneratorPrototype.return = function GeneratorReturn(value) {
  if (this === null || this === undefined) throw ThrowTypeError("Generator.prototype.return called on null or undefined");
  return _nativeReturn.call(this, value);
};

// GeneratorPrototype.throw (ES2025 §27.4.1.4)
GeneratorPrototype.throw = function GeneratorThrow(exception) {
  if (this === null || this === undefined) throw ThrowTypeError("Generator.prototype.throw called on null or undefined");
  return _nativeThrow.call(this, exception);
};

// GeneratorPrototype[@@iterator] (ES2025 §27.4.1.5)
GeneratorPrototype[Symbol.iterator] = function GeneratorIterator() {
  if (this === null || this === undefined) throw ThrowTypeError("Generator.prototype[Symbol.iterator] called on null or undefined");
  return _nativeIterator.call(this);
};
