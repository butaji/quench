// Self-hosted Function prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

var _nativeBind = Function.prototype.__bind;
var _nativeToString = Function.prototype.__toString;

// Function.prototype.bind (ES2025 §20.2.3.3)
Function.prototype.bind = function FunctionBind(thisArg) {
  if (typeof this !== 'function') throw ThrowTypeError("bind called on non-function");
  var args = [thisArg];
  for (var i = 1; i < arguments.length; i++) args.push(arguments[i]);
  return _nativeBind.apply(this, args);
};

// Function.prototype.toString (ES2025 §20.2.3.7)
Function.prototype.toString = function FunctionToString() {
  if (typeof this !== 'function') throw ThrowTypeError("toString called on non-function");
  return this.__toString();
};
