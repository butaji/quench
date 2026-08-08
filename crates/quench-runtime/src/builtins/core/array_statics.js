// Self-hosted Array methods over the `__ops__` bridge.

// Array.isArray(arg) — §23.1.2.2: true iff `arg` is an Array exotic object.
Array.isArray = function isArray(arg) {
  return __ops__.isArray(arg);
};

// Array.prototype.indexOf(searchElement, fromIndex) — §23.1.3.14.
// Per spec the comparison is SameValueZero (so NaN matches NaN); the Rust
// implementation used strict equality and missed NaN. Prototype methods must
// be non-enumerable, so install with defineProperty.
Object.defineProperty(Array.prototype, 'indexOf', {
  value: function indexOf(searchElement, fromIndex) {
    var len = this.length;
    var start = fromIndex === undefined ? 0 : __ops__.toNumber(fromIndex);
    if (start !== start) start = 0;
    if (start < 0) start = Math.max(len + start, 0);
    for (var i = start; i < len; i++) {
      if (i in this && __ops__.sameValueZero(this[i], searchElement)) return i;
    }
    return -1;
  },
  writable: true,
  configurable: true
});