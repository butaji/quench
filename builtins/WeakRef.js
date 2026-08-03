// Self-hosted WeakRef prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

var _nativeDeref = WeakRef.prototype.__deref;

WeakRef.prototype.deref = function WeakRefDeref() {
  if (this === null || this === undefined) throw ThrowTypeError("WeakRef.prototype.deref called on null or undefined");
  return _nativeDeref.call(this);
};
