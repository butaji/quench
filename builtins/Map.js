// Self-hosted Map prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations
var _nativeGet = Map.prototype.get;
var _nativeSet = Map.prototype.set;
var _nativeHas = Map.prototype.has;
var _nativeDelete = Map.prototype.delete;
var _nativeClear = Map.prototype.clear;

// Map.prototype.get (ES2025 §24.1.3.6)
Map.prototype.get = function MapGet(key) {
  if (this === null || this === undefined) throw ThrowTypeError("Map.prototype.get called on null or undefined");
  return _nativeGet.call(this, key);
};

// Map.prototype.set (ES2025 §24.1.3.9)
Map.prototype.set = function MapSet(key, value) {
  if (this === null || this === undefined) throw ThrowTypeError("Map.prototype.set called on null or undefined");
  _nativeSet.call(this, key, value);
  return this;
};

// Map.prototype.has (ES2025 §24.1.3.7)
Map.prototype.has = function MapHas(key) {
  if (this === null || this === undefined) throw ThrowTypeError("Map.prototype.has called on null or undefined");
  return _nativeHas.call(this, key);
};

// Map.prototype.delete (ES2025 §24.1.3.3)
Map.prototype.delete = function MapDelete(key) {
  if (this === null || this === undefined) throw ThrowTypeError("Map.prototype.delete called on null or undefined");
  return _nativeDelete.call(this, key);
};

// Map.prototype.clear (ES2025 §24.1.3.2)
Map.prototype.clear = function MapClear() {
  if (this === null || this === undefined) throw ThrowTypeError("Map.prototype.clear called on null or undefined");
  return _nativeClear.call(this);
};
