// Self-hosted JSON methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

// Save native implementations
var _nativeStringify = JSON.stringify;
var _nativeParse = JSON.parse;

// JSON.stringify (ES2025 §25.5.2)
JSON.stringify = function JSONStringify(value, replacer, space) {
  return _nativeStringify(value, replacer, space);
};

// JSON.parse (ES2025 §25.5.1)
JSON.parse = function JSONParse(text, reviver) {
  return _nativeParse(text, reviver);
};
