// Self-hosted TypedArray prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;
var IsCallable = ops.IsCallable;

var _nativeFilter = TypedArray.prototype.filter;
var _nativeMap = TypedArray.prototype.map;
var _nativeForEach = TypedArray.prototype.forEach;
var _nativeReduce = TypedArray.prototype.reduce;
var _nativeSlice = TypedArray.prototype.slice;
var _nativeSubarray = TypedArray.prototype.subarray;
var _nativeFill = TypedArray.prototype.__fill;
var _nativeValues = TypedArray.prototype.__values;
var _nativeKeys = TypedArray.prototype.__keys;
var _nativeIterator = TypedArray.prototype[Symbol.iterator];

TypedArray.prototype.fill = function(value, start, end) {
  return _nativeFill.call(this, value, start, end);
};
TypedArray.prototype.values = function() {
  return _nativeValues.call(this);
};
TypedArray.prototype.keys = function() {
  return _nativeKeys.call(this);
};
TypedArray.prototype[Symbol.iterator] = function() {
  return _nativeIterator.call(this);
};

TypedArray.prototype.filter = function(callbackfn, thisArg) {
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  return _nativeFilter.call(this, callbackfn, thisArg);
};
TypedArray.prototype.map = function(callbackfn, thisArg) {
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  return _nativeMap.call(this, callbackfn, thisArg);
};
TypedArray.prototype.forEach = function(callbackfn, thisArg) {
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  return _nativeForEach.call(this, callbackfn, thisArg);
};
TypedArray.prototype.reduce = function(callbackfn, initialValue) {
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  return _nativeReduce.call(this, callbackfn, initialValue);
};
TypedArray.prototype.slice = function(start, end) {
  return _nativeSlice.call(this, start, end);
};
TypedArray.prototype.subarray = function(begin, end) {
  return _nativeSubarray.call(this, begin, end);
};
