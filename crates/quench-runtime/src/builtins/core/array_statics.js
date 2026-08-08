// Self-hosted Array static methods over the `__ops__` bridge.

// Array.isArray(arg) — §23.1.2.2: true iff `arg` is an Array exotic object.
Array.isArray = function isArray(arg) {
  return __ops__.isArray(arg);
};