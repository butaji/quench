// Self-hosted WeakMap prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

var WeakMapNativeGet = WeakMap.prototype.__get;
var WeakMapNativeSet = WeakMap.prototype.__set;
var WeakMapNativeHas = WeakMap.prototype.__has;
var WeakMapNativeDelete = WeakMap.prototype.__delete;

WeakMap.prototype.get = function WeakMapGet(key) {
  if (this === null || this === undefined) throw ThrowTypeError("WeakMap.prototype.get called on null or undefined");
  return WeakMapNativeGet.call(this, key);
};

WeakMap.prototype.set = function WeakMapSet(key, value) {
  if (this === null || this === undefined) throw ThrowTypeError("WeakMap.prototype.set called on null or undefined");
  WeakMapNativeSet.call(this, key, value);
  return this;
};

WeakMap.prototype.has = function WeakMapHas(key) {
  if (this === null || this === undefined) throw ThrowTypeError("WeakMap.prototype.has called on null or undefined");
  return WeakMapNativeHas.call(this, key);
};

WeakMap.prototype.delete = function WeakMapDelete(key) {
  if (this === null || this === undefined) throw ThrowTypeError("WeakMap.prototype.delete called on null or undefined");
  return WeakMapNativeDelete.call(this, key);
};
