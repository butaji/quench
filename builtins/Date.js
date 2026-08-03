// Self-hosted Date prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations
var _nativeToString = Date.prototype.__toString;
var _nativeValueOf = Date.prototype.__valueOf;
var _nativeNow = Date.__now;
var _nativeParse = Date.__parse;
var _nativeUTC = Date.__UTC;

Date.now = function DateNow() {
  return _nativeNow.call(this);
};

Date.parse = function DateParse(string) {
  return _nativeParse.call(this, string);
};

Date.UTC = function DateUTC() {
  return _nativeUTC.apply(this, arguments);
};
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

Date.prototype.getTime = function() { return Date.prototype.__getTime.call(this); };
Date.prototype.getTimezoneOffset = function() { return Date.prototype.__getTimezoneOffset.call(this); };
Date.prototype.getFullYear = function() { return Date.prototype.__getFullYear.call(this); };
Date.prototype.getMonth = function() { return Date.prototype.__getMonth.call(this); };
Date.prototype.getDate = function() { return Date.prototype.__getDate.call(this); };
Date.prototype.getUTCFullYear = function() { return Date.prototype.__getUTCFullYear.call(this); };
Date.prototype.getUTCMonth = function() { return Date.prototype.__getUTCMonth.call(this); };
Date.prototype.getUTCDate = function() { return Date.prototype.__getUTCDate.call(this); };

Date.prototype.getDay = function() { return Date.prototype.__getDay.call(this); };
Date.prototype.getHours = function() { return Date.prototype.__getHours.call(this); };
Date.prototype.getMilliseconds = function() { return Date.prototype.__getMilliseconds.call(this); };
Date.prototype.getMinutes = function() { return Date.prototype.__getMinutes.call(this); };
Date.prototype.getSeconds = function() { return Date.prototype.__getSeconds.call(this); };
Date.prototype.getUTCDay = function() { return Date.prototype.__getUTCDay.call(this); };
Date.prototype.getUTCHours = function() { return Date.prototype.__getUTCHours.call(this); };
Date.prototype.getUTCMilliseconds = function() { return Date.prototype.__getUTCMilliseconds.call(this); };
Date.prototype.getUTCMinutes = function() { return Date.prototype.__getUTCMinutes.call(this); };
Date.prototype.getUTCSeconds = function() { return Date.prototype.__getUTCSeconds.call(this); };
Date.prototype.setDate = function() { return Date.prototype.__setDate.apply(this, arguments); };
Date.prototype.setFullYear = function() { return Date.prototype.__setFullYear.apply(this, arguments); };
Date.prototype.setHours = function() { return Date.prototype.__setHours.apply(this, arguments); };
Date.prototype.setMilliseconds = function() { return Date.prototype.__setMilliseconds.apply(this, arguments); };
Date.prototype.setMinutes = function() { return Date.prototype.__setMinutes.apply(this, arguments); };
Date.prototype.setMonth = function() { return Date.prototype.__setMonth.apply(this, arguments); };
Date.prototype.setSeconds = function() { return Date.prototype.__setSeconds.apply(this, arguments); };
Date.prototype.setTime = function() { return Date.prototype.__setTime.apply(this, arguments); };
Date.prototype.setUTCDate = function() { return Date.prototype.__setUTCDate.apply(this, arguments); };
Date.prototype.setUTCFullYear = function() { return Date.prototype.__setUTCFullYear.apply(this, arguments); };
Date.prototype.setUTCHours = function() { return Date.prototype.__setUTCHours.apply(this, arguments); };
Date.prototype.setUTCMilliseconds = function() { return Date.prototype.__setUTCMilliseconds.apply(this, arguments); };
Date.prototype.setUTCMinutes = function() { return Date.prototype.__setUTCMinutes.apply(this, arguments); };
Date.prototype.setUTCMonth = function() { return Date.prototype.__setUTCMonth.apply(this, arguments); };
Date.prototype.setUTCSeconds = function() { return Date.prototype.__setUTCSeconds.apply(this, arguments); };
Date.prototype.toLocaleString = function() { return Date.prototype.__toLocaleString.apply(this, arguments); };
Date.prototype.toUTCString = function() { return Date.prototype.__toUTCString.apply(this, arguments); };
