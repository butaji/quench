// Self-hosted URI global methods over Rust UTF-8/percent primitives.
var _nativeEncodeURI = __encodeURI;
var _nativeEncodeURIComponent = __encodeURIComponent;
var _nativeDecodeURI = __decodeURI;
var _nativeDecodeURIComponent = __decodeURIComponent;
var _nativeParseInt = __parseInt;
var _nativeParseFloat = __parseFloat;
var _nativeIsNaN = __isNaN;
var _nativeIsFinite = __isFinite;

encodeURI = function encodeURI(uri) {
  return _nativeEncodeURI.apply(this, arguments);
};
encodeURIComponent = function encodeURIComponent(uriComponent) {
  return _nativeEncodeURIComponent.apply(this, arguments);
};
decodeURI = function decodeURI(uri) {
  return _nativeDecodeURI.apply(this, arguments);
};
decodeURIComponent = function decodeURIComponent(uriComponent) {
  return _nativeDecodeURIComponent.apply(this, arguments);
};
parseInt = function parseInt(string, radix) {
  return _nativeParseInt.apply(this, arguments);
};
parseFloat = function parseFloat(string) {
  return _nativeParseFloat.apply(this, arguments);
};
isNaN = function isNaN(value) {
  return _nativeIsNaN.apply(this, arguments);
};
isFinite = function isFinite(value) {
  return _nativeIsFinite.apply(this, arguments);
};
