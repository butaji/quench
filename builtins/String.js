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
var _toUpperCase = String.prototype.__toUpperCase;
var _toLowerCase = String.prototype.__toLowerCase;
var _concat = String.prototype.__concat;
var _repeat = String.prototype.__repeat;
var _substring = String.prototype.__substring;
var _slice = String.prototype.__slice;
var _split = String.prototype.__split;
var _toString = String.prototype.__toString;
var _valueOf = String.prototype.__valueOf;
var _match = String.prototype.__match;
var _search = String.prototype.__search;
var _replace = String.prototype.__replace;
var _regexSplit = String.prototype.__regexSplit;
var _replaceAll = String.prototype.__replaceAll;

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

String.prototype.toUpperCase = function StringToUpperCase() {
  return _toUpperCase.call(this);
};

String.prototype.toLowerCase = function StringToLowerCase() {
  return _toLowerCase.call(this);
};

String.prototype.toLocaleUpperCase = String.prototype.toUpperCase;
String.prototype.toLocaleLowerCase = String.prototype.toLowerCase;

String.prototype.concat = function StringConcat() {
  return _concat.apply(this, arguments);
};

String.prototype.substring = function StringSubstring(start, end) {
  return _substring.call(this, start, end);
};

String.prototype.slice = function StringSlice(start, end) {
  return _slice.call(this, start, end);
};

String.prototype.split = function StringSplit(separator, limit) {
  if (separator instanceof RegExp) return _regexSplit.call(this, separator, limit);
  return _split.call(this, separator, limit);
};

String.prototype.toString = function StringToString() {
  return _toString.call(this);
};

String.prototype.valueOf = function StringValueOf() {
  return _valueOf.call(this);
};

String.prototype.match = function StringMatch(regexp) {
  return _match.call(this, regexp);
};

String.prototype.search = function StringSearch(regexp) {
  return _search.call(this, regexp);
};

String.prototype.replace = function StringReplace(searchValue, replaceValue) {
  return _replace.call(this, searchValue, replaceValue);
};

String.prototype.replaceAll = function StringReplaceAll(searchValue, replaceValue) {
  return _replaceAll.call(this, searchValue, replaceValue);
};

String.prototype.repeat = function StringRepeat(count) {
  return _repeat.call(this, count);
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
