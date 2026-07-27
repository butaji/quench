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
