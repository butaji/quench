// Self-hosted WeakSet prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

var _nativeAdd = WeakSet.prototype.__add;
var _nativeHas = WeakSet.prototype.__has;
var _nativeDelete = WeakSet.prototype.__delete;

WeakSet.prototype.add = function WeakSetAdd(value) {
  if (this === null || this === undefined) throw ThrowTypeError("WeakSet.prototype.add called on null or undefined");
  _nativeAdd.call(this, value);
  return this;
};

WeakSet.prototype.has = function WeakSetHas(value) {
  if (this === null || this === undefined) throw ThrowTypeError("WeakSet.prototype.has called on null or undefined");
  return _nativeHas.call(this, value);
};

WeakSet.prototype.delete = function WeakSetDelete(value) {
  if (this === null || this === undefined) throw ThrowTypeError("WeakSet.prototype.delete called on null or undefined");
  return _nativeDelete.call(this, value);
};
