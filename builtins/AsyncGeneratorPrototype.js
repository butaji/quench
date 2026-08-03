// Self-hosted AsyncGeneratorPrototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Get %AsyncGeneratorPrototype% via AsyncGeneratorFunction.prototype.prototype
var AsyncGeneratorProto = AsyncGeneratorFunction.prototype.prototype;

// Save native implementations
var _nativeNext = AsyncGeneratorProto.__next;
var _nativeReturn = AsyncGeneratorProto.__return;
var _nativeThrow = AsyncGeneratorProto.__throw;

// AsyncGeneratorPrototype.next (ES2025 §27.6.1.2.1)
AsyncGeneratorProto.next = function AsyncGeneratorNext(value) {
  if (this === null || this === undefined) throw ThrowTypeError("AsyncGenerator.prototype.next called on null or undefined");
  return _nativeNext.apply(this, arguments);
};

// AsyncGeneratorPrototype.return (ES2025 §27.6.1.2.2)
AsyncGeneratorProto.return = function AsyncGeneratorReturn(value) {
  if (this === null || this === undefined) throw ThrowTypeError("AsyncGenerator.prototype.return called on null or undefined");
  return _nativeReturn.apply(this, arguments);
};

// AsyncGeneratorPrototype.throw (ES2025 §27.6.1.2.3)
AsyncGeneratorProto.throw = function AsyncGeneratorThrow(exception) {
  if (this === null || this === undefined) throw ThrowTypeError("AsyncGenerator.prototype.throw called on null or undefined");
  return _nativeThrow.apply(this, arguments);
};
