var _String = String;

String.prototype.includes = function StringIncludes(searchString, position) {
  var string = _String(this);
  var search = _String(searchString);
  var start = position === undefined ? 0 : Math.max(position, 0);
  return string.indexOf(search, start) !== -1;
};

String.prototype.startsWith = function StringStartsWith(searchString, position) {
  var string = _String(this);
  var search = _String(searchString);
  var start = position === undefined ? 0 : Math.max(position, 0);
  return string.slice(start, start + search.length) === search;
};

String.prototype.endsWith = function StringEndsWith(searchString, endPosition) {
  var string = _String(this);
  var search = _String(searchString);
  var end = endPosition === undefined ? string.length : Math.min(endPosition, string.length);
  return string.slice(end - search.length, end) === search;
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
