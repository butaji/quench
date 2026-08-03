// Self-hosted RegExp prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations
var _nativeTest = RegExp.prototype.__test;

// RegExp.prototype.test (ES2025 §22.2.5.15)
RegExp.prototype.test = function RegExpTest(S) {
  if (this === null || this === undefined) throw ThrowTypeError("RegExp.prototype.test called on null or undefined");
  return _nativeTest.call(this, S);
};

// RegExp.prototype.toString (ES2025 §22.2.5.16)
RegExp.prototype.toString = function RegExpToString() {
  if (this === null || this === undefined) throw ThrowTypeError("RegExp.prototype.toString called on null or undefined");
  var source = String(this.source);
  var flags = String(this.flags);
  return "/" + (source === "" ? "(?:)" : source) + "/" + flags;
};
