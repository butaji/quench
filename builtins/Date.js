// Self-hosted Date prototype methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations
var _nativeToString = Date.prototype.__toString;
var _nativeValueOf = Date.prototype.__valueOf;
// Date.prototype.toString (ES2025 §21.4.4.38)
Date.prototype.toString = function DateToString() {
  if (this === null || this === undefined) throw ThrowTypeError("Date.prototype.toString called on null or undefined");
  return _nativeToString.call(this);
};

// Date.prototype.toISOString (ES2025 §21.4.4.41)
Date.prototype.toISOString = function DateToISOString() {
  if (this === null || this === undefined) throw ThrowTypeError("Date.prototype.toISOString called on null or undefined");
  var time = this.getTime();
  if (time !== time || time === Infinity || time === -Infinity) throw new RangeError("Invalid time value");
  var year = this.getUTCFullYear();
  var month = this.getUTCMonth() + 1;
  var day = this.getUTCDate();
  var hour = this.getUTCHours();
  var minute = this.getUTCMinutes();
  var second = this.getUTCSeconds();
  var millis = this.getUTCMilliseconds();
  function pad(value, width) {
    var text = String(value);
    while (text.length < width) text = '0' + text;
    return text;
  }
  var yearText = year >= 0 && year <= 9999 ? pad(year, 4) : (year < 0 ? '-' + pad(-year, 6) : '+' + pad(year, 6));
  return yearText + '-' + pad(month, 2) + '-' + pad(day, 2) + 'T' +
    pad(hour, 2) + ':' + pad(minute, 2) + ':' + pad(second, 2) + '.' + pad(millis, 3) + 'Z';
};

// Date.prototype.toJSON (ES2025 §21.4.4.42)
Date.prototype.toJSON = function DateToJSON(key) {
  if (this === null || this === undefined) throw ThrowTypeError("Date.prototype.toJSON called on null or undefined");
  var primitive = Number(this);
  if (primitive !== primitive || primitive === Infinity || primitive === -Infinity) return null;
  return this.toISOString();
};

Date.prototype.valueOf = function DateValueOf() {
  if (this === null || this === undefined) throw ThrowTypeError("Date.prototype.valueOf called on null or undefined");
  return _nativeValueOf.call(this);
};
