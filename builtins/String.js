var _String = String;
var _charAt = String.prototype.__charAt;
var _charCodeAt = String.prototype.__charCodeAt;
var _codePointAt = String.prototype.__codePointAt;
var _at = String.prototype.__at;
var _includes = String.prototype.__includes;
var _startsWith = String.prototype.__startsWith;
var _endsWith = String.prototype.__endsWith;
var _indexOf = String.prototype.__indexOf;
var _lastIndexOf = String.prototype.__lastIndexOf;

String.prototype.includes = function StringIncludes(searchString, position) {
  return _includes.call(this, searchString, position);
};

String.prototype.startsWith = function StringStartsWith(searchString, position) {
  return _startsWith.call(this, searchString, position);
};

String.prototype.endsWith = function StringEndsWith(searchString, endPosition) {
  return _endsWith.call(this, searchString, endPosition);
};

String.prototype.indexOf = function StringIndexOf(searchString, position) {
  return _indexOf.call(this, searchString, position);
};

String.prototype.lastIndexOf = function StringLastIndexOf(searchString, position) {
  return _lastIndexOf.call(this, searchString, position);
};

String.prototype.charAt = function StringCharAt(position) {
  return _charAt.call(this, position);
};

String.prototype.charCodeAt = function StringCharCodeAt(position) {
  return _charCodeAt.call(this, position);
};

String.prototype.codePointAt = function StringCodePointAt(position) {
  return _codePointAt.call(this, position);
};

String.prototype.at = function StringAt(index) {
  return _at.call(this, index);
};

String.prototype.repeat = function StringRepeat(count) {
  var string = _String(this);
  var n = Math.floor(count);
  if (n < 0 || n === Infinity) throw new RangeError("Invalid count value");
  var result = "";
  for (var i = 0; i < n; i++) result += string;
  return result;
};

String.prototype.padStart = function StringPadStart(maxLength, fillString) {
  var string = _String(this);
  var target = Math.floor(maxLength);
  if (target <= string.length || target === Infinity) return string;
  var fill = fillString === undefined ? " " : _String(fillString);
  if (fill.length === 0) return string;
  var padding = "";
  while (padding.length < target - string.length) padding += fill;
  return padding.slice(0, target - string.length) + string;
};

String.prototype.padEnd = function StringPadEnd(maxLength, fillString) {
  var string = _String(this);
  var target = Math.floor(maxLength);
  if (target <= string.length || target === Infinity) return string;
  var fill = fillString === undefined ? " " : _String(fillString);
  if (fill.length === 0) return string;
  var padding = "";
  while (padding.length < target - string.length) padding += fill;
  return string + padding.slice(0, target - string.length);
};

String.prototype.trim = function StringTrim() {
  return _String(this).replace(/^\s+|\s+$/g, "");
};

String.prototype.trimStart = function StringTrimStart() {
  return _String(this).replace(/^\s+/, "");
};

String.prototype.trimEnd = function StringTrimEnd() {
  return _String(this).replace(/\s+$/, "");
};

String.prototype.trimLeft = String.prototype.trimStart;
String.prototype.trimRight = String.prototype.trimEnd;
