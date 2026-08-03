// Self-hosted Map prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;
var IsCallable = ops.IsCallable;

// Save native implementations
var _nativeGet = Map.prototype.__get;
var _nativeSet = Map.prototype.__set;
var _nativeHas = Map.prototype.__has;
var _nativeDelete = Map.prototype.__delete;
var _nativeClear = Map.prototype.__clear;

Map.prototype.forEach = function MapForEach(callbackfn, thisArg) {
  if (this === null || this === undefined) throw ThrowTypeError("Map.prototype.forEach called on null or undefined");
  if (typeof callbackfn !== 'function') throw ThrowTypeError("callbackfn is not a function");
  var iterator = this.entries();
  var step;
  while (!(step = iterator.next()).done) {
    var entry = step.value;
    callbackfn.call(thisArg, entry[1], entry[0], this);
  }
  return undefined;
};

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

Map.groupBy = function MapGroupBy(items, callbackfn) {
  if (items === null || items === undefined) throw ThrowTypeError("Map.groupBy requires an iterable");
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var map = new Map();
  var iteratorMethod = items[Symbol.iterator];
  if (!IsCallable(iteratorMethod)) throw ThrowTypeError("Map.groupBy requires an iterable");
  var iterator = iteratorMethod.call(items);
  var index = 0;
  var step;
  while (!(step = iterator.next()).done) {
    var value = step.value;
    var key = callbackfn(value, index++);
    var group = map.get(key);
    if (group === undefined) { group = []; map.set(key, group); }
    group.push(value);
  }
  return map;
};
