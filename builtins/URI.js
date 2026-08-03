// Self-hosted URI global methods over Rust UTF-8/percent primitives.
var _nativeEncodeURI = __encodeURI;
var _nativeEncodeURIComponent = __encodeURIComponent;
var _nativeDecodeURI = __decodeURI;
var _nativeDecodeURIComponent = __decodeURIComponent;

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
