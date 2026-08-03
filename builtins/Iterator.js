// Self-hosted Iterator helpers on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;
var IsCallable = ops.IsCallable;

// Save native implementations
var _nativeFrom = Iterator.__from;
var _nativeProtoMap = Iterator.prototype.__map;
var _nativeProtoFilter = Iterator.prototype.__filter;
var _nativeProtoTake = Iterator.prototype.__take;
var _nativeProtoDrop = Iterator.prototype.__drop;
var _nativeProtoFlatMap = Iterator.prototype.__flatMap;
var _nativeProtoReduce = Iterator.prototype.__reduce;
var _nativeProtoToArray = Iterator.prototype.__toArray;
var _nativeProtoForEach = Iterator.prototype.__forEach;
var _nativeProtoSome = Iterator.prototype.__some;
var _nativeProtoEvery = Iterator.prototype.__every;
var _nativeProtoFind = Iterator.prototype.__find;

// Iterator.from (ES2025 §27.1.3.2)
Iterator.from = function IteratorFrom(O) {
  return _nativeFrom(O);
};

// Iterator.prototype.map (ES2025 §27.1.4.4)
Iterator.prototype.map = function IteratorMap(mapper) {
  if (!IsCallable(mapper)) throw ThrowTypeError("mapper is not a function");
  return _nativeProtoMap.call(this, mapper);
};

// Iterator.prototype.filter (ES2025 §27.1.4.3)
Iterator.prototype.filter = function IteratorFilter(filterer) {
  if (!IsCallable(filterer)) throw ThrowTypeError("filterer is not a function");
  return _nativeProtoFilter.call(this, filterer);
};

Iterator.prototype.take = function(limit) { return _nativeProtoTake.call(this, limit); };
Iterator.prototype.drop = function(limit) { return _nativeProtoDrop.call(this, limit); };
Iterator.prototype.flatMap = function(mapper) {
  if (!IsCallable(mapper)) throw ThrowTypeError("mapper is not a function");
  return _nativeProtoFlatMap.call(this, mapper);
};
Iterator.prototype.reduce = function(reducer, initialValue) {
  if (!IsCallable(reducer)) throw ThrowTypeError("reducer is not a function");
  return _nativeProtoReduce.call(this, reducer, initialValue);
};
Iterator.prototype.toArray = function() { return _nativeProtoToArray.call(this); };
Iterator.prototype.forEach = function(fn) {
  if (!IsCallable(fn)) throw ThrowTypeError("fn is not a function");
  return _nativeProtoForEach.call(this, fn);
};
Iterator.prototype.some = function(fn) {
  if (!IsCallable(fn)) throw ThrowTypeError("fn is not a function");
  return _nativeProtoSome.call(this, fn);
};
Iterator.prototype.every = function(fn) {
  if (!IsCallable(fn)) throw ThrowTypeError("fn is not a function");
  return _nativeProtoEvery.call(this, fn);
};
Iterator.prototype.find = function(fn) {
  if (!IsCallable(fn)) throw ThrowTypeError("fn is not a function");
  return _nativeProtoFind.call(this, fn);
};
