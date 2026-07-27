// Self-hosted Set prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations
var _nativeAdd = Set.prototype.add;
var _nativeHas = Set.prototype.has;
var _nativeDelete = Set.prototype.delete;
var _nativeClear = Set.prototype.clear;

// Set.prototype.add (ES2025 §24.2.3.1)
Set.prototype.add = function SetAdd(value) {
  if (this === null || this === undefined) throw ThrowTypeError("Set.prototype.add called on null or undefined");
  _nativeAdd.call(this, value);
  return this;
};

// Set.prototype.has (ES2025 §24.2.3.7)
Set.prototype.has = function SetHas(value) {
  if (this === null || this === undefined) throw ThrowTypeError("Set.prototype.has called on null or undefined");
  return _nativeHas.call(this, value);
};

// Set.prototype.delete (ES2025 §24.2.3.4)
Set.prototype.delete = function SetDelete(value) {
  if (this === null || this === undefined) throw ThrowTypeError("Set.prototype.delete called on null or undefined");
  return _nativeDelete.call(this, value);
};

// Set.prototype.clear (ES2025 §24.2.3.2)
Set.prototype.clear = function SetClear() {
  if (this === null || this === undefined) throw ThrowTypeError("Set.prototype.clear called on null or undefined");
  return _nativeClear.call(this);
};
