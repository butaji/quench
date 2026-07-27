// Self-hosted Error prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Error.prototype.toString (ES2025 §20.5.3.5)
Error.prototype.toString = function ErrorToString() {
  if (this === null || this === undefined) throw ThrowTypeError("Error.prototype.toString called on null or undefined");
  var obj = this;
  var name = obj.name !== undefined ? String(obj.name) : 'Error';
  var msg = obj.message !== undefined ? String(obj.message) : '';
  return msg ? name + ': ' + msg : name;
};
