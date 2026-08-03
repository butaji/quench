// Self-hosted public host bindings over Rust engine integration points.
var _nativeSetTimeout = __setTimeout;
var _nativeSetInterval = __setInterval;
var _nativeClearTimeout = __clearTimeout;
var _nativeClearInterval = __clearInterval;

setTimeout = function setTimeout(callback, delay) {
  return _nativeSetTimeout.apply(this, arguments);
};
setInterval = function setInterval(callback, delay) {
  return _nativeSetInterval.apply(this, arguments);
};
clearTimeout = function clearTimeout(id) {
  return _nativeClearTimeout.apply(this, arguments);
};
clearInterval = function clearInterval(id) {
  return _nativeClearInterval.apply(this, arguments);
};
