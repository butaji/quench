// Self-hosted Date prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations
var _nativeToString = Date.prototype.__toString;
var _nativeValueOf = Date.prototype.__valueOf;
var _nativeToISOString = Date.prototype.toISOString;
var _nativeToJSON = Date.prototype.toJSON;

// Date.prototype.toString (ES2025 §21.4.4.38)
Date.prototype.toString = function DateToString() {
  if (this === null || this === undefined) throw ThrowTypeError("Date.prototype.toString called on null or undefined");
  return _nativeToString.call(this);
};

// Date.prototype.toISOString (ES2025 §21.4.4.41)
Date.prototype.toISOString = function DateToISOString() {
  if (this === null || this === undefined) throw ThrowTypeError("Date.prototype.toISOString called on null or undefined");
  return _nativeToISOString.call(this);
};

// Date.prototype.toJSON (ES2025 §21.4.4.42)
Date.prototype.toJSON = function DateToJSON(key) {
  if (this === null || this === undefined) throw ThrowTypeError("Date.prototype.toJSON called on null or undefined");
  return _nativeToJSON.call(this, key);
};

Date.prototype.valueOf = function DateValueOf() {
  if (this === null || this === undefined) throw ThrowTypeError("Date.prototype.valueOf called on null or undefined");
  return _nativeValueOf.call(this);
};
