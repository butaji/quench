var _String = String;
var _charAt = String.prototype.__charAt;
var _charCodeAt = String.prototype.__charCodeAt;
var _codePointAt = String.prototype.__codePointAt;
var _at = String.prototype.__at;
var _slice = String.prototype.__slice;
var _indexOf = String.prototype.__indexOf;
var _lastIndexOf = String.prototype.__lastIndexOf;
var _toUpperCase = String.prototype.__toUpperCase;
var _toLowerCase = String.prototype.__toLowerCase;
var _repeat = String.prototype.__repeat;
var _toString = String.prototype.__toString;
var _valueOf = String.prototype.__valueOf;
var _match = String.prototype.__match;
var _search = String.prototype.__search;
var _replace = String.prototype.__replace;
var _regexSplit = String.prototype.__regexSplit;
var _replaceAll = String.prototype.__replaceAll;
var _fromCharCode = String.__fromCharCode;
var _fromCodePoint = String.__fromCodePoint;

function ToLength(value) {
  var n = Number(value);
  if (n !== n || n <= 0) return 0;
  if (n === Infinity) return 9007199254740991;
  return Math.min(Math.floor(n), 9007199254740991);
}

String.fromCharCode = function StringFromCharCode() {
  return _fromCharCode.apply(this, arguments);
};

String.fromCodePoint = function StringFromCodePoint() {
  return _fromCodePoint.apply(this, arguments);
};

String.raw = function StringRaw(template) {
  var cooked = Object(template);
  var raw = Object(cooked.raw);
  var len = ToLength(raw.length);
  if (len === 0) return "";
  var result = "";
  for (var i = 0; i < len; i++) {
    result += String(raw[i]);
    if (i + 1 < len && i + 1 < arguments.length) result += String(arguments[i + 1]);
  }
  return result;
};

String.prototype.includes = function StringIncludes(searchString, position) {
  if (searchString instanceof RegExp) throw new TypeError('First argument must not be a RegExp');
  var string = this + '';
  var search = searchString + '';
  var start = Number(position);
  start = start !== start || start < 0 ? 0 : Math.min(Math.floor(start), string.length);
  return _indexOf.call(string, search, start) !== -1;
};

String.prototype.startsWith = function StringStartsWith(searchString, position) {
  if (searchString instanceof RegExp) throw new TypeError('First argument must not be a RegExp');
  var string = this + '';
  var search = searchString + '';
  var start = Number(position);
  start = start !== start || start < 0 ? 0 : Math.min(Math.floor(start), string.length);
  return string.slice(start, start + search.length) === search;
};

String.prototype.endsWith = function StringEndsWith(searchString, endPosition) {
  if (searchString instanceof RegExp) throw new TypeError('First argument must not be a RegExp');
  var string = this + '';
  var search = searchString + '';
  var end = endPosition === undefined ? string.length : Number(endPosition);
  end = end !== end || end < 0 ? 0 : Math.min(Math.floor(end), string.length);
  var start = Math.max(end - search.length, 0);
  return string.slice(start, end) === search;
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
  var result = this + '';
  for (var i = 0; i < arguments.length; i++) result += String(arguments[i]);
  return result;
};

String.prototype.substring = function StringSubstring(start, end) {
  var string = this + '';
  var length = string.length;
  var first = Number(start);
  var from = first !== first || first < 0 ? 0 : first === Infinity ? length : Math.floor(first);
  var last = end === undefined ? length : Number(end);
  var to = last !== last || last < 0 ? 0 : last === Infinity ? length : Math.floor(last);
  if (from > to) {
    var swap = from;
    from = to;
    to = swap;
  }
  return _slice.call(string, from, to);
};

String.prototype.slice = function StringSlice(start, end) {
  var string = this + '';
  var length = string.length;
  var first = Number(start);
  var from = first !== first || first === 0 ? 0 : first === Infinity ? length : first === -Infinity ? 0 : first < 0 ? Math.max(length + Math.ceil(first), 0) : Math.min(Math.floor(first), length);
  var last = end === undefined ? length : Number(end);
  var to = last !== last || last === 0 ? 0 : last === Infinity ? length : last === -Infinity ? 0 : last < 0 ? Math.max(length + Math.ceil(last), 0) : Math.min(Math.floor(last), length);
  if (to < from) return '';
  return _slice.call(string, from, to);
};

String.prototype.split = function StringSplit(separator, limit) {
  var string = this + '';
  var max = limit === undefined ? 4294967295 : Number(limit) >>> 0;
  if (max === 0) return [];
  if (separator instanceof RegExp) return _regexSplit.call(string, separator, max);
  if (separator === undefined) return [string];
  var delimiter = separator + '';
  if (delimiter === '') {
    var units = [];
    for (var i = 0; i < string.length && units.length < max; i++) units.push(string.charAt(i));
    return units;
  }
  var result = [];
  var start = 0;
  while (result.length + 1 < max) {
    var next = _indexOf.call(string, delimiter, start);
    if (next < 0) break;
    result.push(string.slice(start, next));
    start = next + delimiter.length;
  }
  result.push(string.slice(start));
  return result;
};

String.prototype.toString = function StringToString() {
  return _toString.call(this);
};

String.prototype.valueOf = function StringValueOf() {
  return _valueOf.call(this);
};

String.prototype.isWellFormed = function StringIsWellFormed() {
  var string = _String(this);
  for (var i = 0; i < string.length; i++) {
    var code = string.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff) {
      var next = i + 1 < string.length ? string.charCodeAt(i + 1) : 0;
      if (next < 0xdc00 || next > 0xdfff) return false;
      i++;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return false;
    }
  }
  return true;
};

String.prototype.toWellFormed = function StringToWellFormed() {
  var string = _String(this);
  var result = "";
  for (var i = 0; i < string.length; i++) {
    var code = string.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff) {
      var next = i + 1 < string.length ? string.charCodeAt(i + 1) : 0;
      if (next >= 0xdc00 && next <= 0xdfff) {
        result += string.charAt(i) + string.charAt(i + 1);
        i++;
      } else {
        result += "\ufffd";
      }
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      result += "\ufffd";
    } else {
      result += string.charAt(i);
    }
  }
  return result;
};

String.prototype.match = function StringMatch(regexp) {
  return _match.call(this, regexp);
};

String.prototype.search = function StringSearch(regexp) {
  return _search.call(this, regexp);
};

String.prototype.replace = function StringReplace(searchValue, replaceValue) {
  if (searchValue instanceof RegExp) return _replace.call(this, searchValue, replaceValue);
  var string = this + '';
  var search = searchValue + '';
  var index = _indexOf.call(string, search, 0);
  if (index < 0) return string;
  return string.slice(0, index) + StringReplacement(search, replaceValue, index, string) + string.slice(index + search.length);
};

String.prototype.replaceAll = function StringReplaceAll(searchValue, replaceValue) {
  if (searchValue instanceof RegExp) return _replaceAll.call(this, searchValue, replaceValue);
  var string = this + '';
  var search = searchValue + '';
  if (search.length === 0) {
    var emptyResult = '';
    for (var emptyIndex = 0; emptyIndex <= string.length; emptyIndex++) {
      emptyResult += StringReplacement(search, replaceValue, emptyIndex, string);
      if (emptyIndex < string.length) emptyResult += string.charAt(emptyIndex);
    }
    return emptyResult;
  }
  var result = '';
  var start = 0;
  while (true) {
    var index = _indexOf.call(string, search, start);
    if (index < 0) return result + string.slice(start);
    result += string.slice(start, index) + StringReplacement(search, replaceValue, index, string);
    start = index + search.length;
  }
};

function StringReplacement(search, replacement, index, string) {
  if (typeof replacement === 'function') return replacement(search, index, string);
  var text = replacement + '';
  var result = '';
  for (var i = 0; i < text.length; i++) {
    if (text.charAt(i) !== '$' || i + 1 === text.length) { result += text.charAt(i); continue; }
    var token = text.charAt(++i);
    if (token === '$') result += '$';
    else if (token === '&') result += search;
    else if (token === '`') result += string.slice(0, index);
    else if (token === "'") result += string.slice(index + search.length);
    else result += '$' + token;
  }
  return result;
}

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
