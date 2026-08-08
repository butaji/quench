// Self-hosted Array methods over the `__ops__` bridge.

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

// Array.prototype.includes(searchElement, fromIndex) — §23.1.3.3.
// Same loop as indexOf but returns a boolean; SameValueZero so NaN matches.
Object.defineProperty(Array.prototype, 'includes', {
  value: function includes(searchElement, fromIndex) {
    var len = this.length;
    var start = fromIndex === undefined ? 0 : __ops__.toNumber(fromIndex);
    if (start !== start) start = 0;
    if (start < 0) start = Math.max(len + start, 0);
    for (var i = start; i < len; i++) {
      if (i in this && __ops__.sameValueZero(this[i], searchElement)) return true;
    }
    return false;
  },
  writable: true,
  configurable: true
});

// Callback methods iterate the array, skipping holes (`i in this`), and call
// the predicate/iterator with (value, index, array). The callback's `this`
// is thisArg (or undefined -> globalThis for sloppy functions), matching the
// ES spec. §23.1.3

function requireCallable(cb, name) {
  if (typeof cb !== 'function') {
    throw __ops__.throwTypeError(name + ': callback is not a function');
  }
}

Object.defineProperty(Array.prototype, 'forEach', {
  value: function forEach(callback, thisArg) {
    requireCallable(callback, 'forEach');
    var len = this.length;
    for (var i = 0; i < len; i++) {
      if (i in this) callback.call(thisArg, this[i], i, this);
    }
  },
  writable: true,
  configurable: true
});

Object.defineProperty(Array.prototype, 'map', {
  value: function map(callback, thisArg) {
    requireCallable(callback, 'map');
    var len = this.length;
    var result = new Array(len);
    for (var i = 0; i < len; i++) {
      if (i in this) result[i] = callback.call(thisArg, this[i], i, this);
    }
    return result;
  },
  writable: true,
  configurable: true
});

Object.defineProperty(Array.prototype, 'filter', {
  value: function filter(callback, thisArg) {
    requireCallable(callback, 'filter');
    var len = this.length;
    var result = [];
    for (var i = 0; i < len; i++) {
      if (i in this && callback.call(thisArg, this[i], i, this)) result.push(this[i]);
    }
    return result;
  },
  writable: true,
  configurable: true
});

Object.defineProperty(Array.prototype, 'every', {
  value: function every(callback, thisArg) {
    requireCallable(callback, 'every');
    var len = this.length;
    for (var i = 0; i < len; i++) {
      if (i in this && !callback.call(thisArg, this[i], i, this)) return false;
    }
    return true;
  },
  writable: true,
  configurable: true
});

Object.defineProperty(Array.prototype, 'some', {
  value: function some(callback, thisArg) {
    requireCallable(callback, 'some');
    var len = this.length;
    for (var i = 0; i < len; i++) {
      if (i in this && callback.call(thisArg, this[i], i, this)) return true;
    }
    return false;
  },
  writable: true,
  configurable: true
});

Object.defineProperty(Array.prototype, 'reduce', {
  value: function reduce(callback /*, initialValue*/) {
    requireCallable(callback, 'reduce');
    var len = this.length;
    var i = 0;
    var accumulator;
    if (arguments.length > 1) {
      accumulator = arguments[1];
    } else {
      while (i < len && !(i in this)) i++;
      if (i >= len) {
        throw __ops__.throwTypeError('Reduce of empty array with no initial value');
      }
      accumulator = this[i];
      i++;
    }
    for (; i < len; i++) {
      if (i in this) accumulator = callback.call(undefined, accumulator, this[i], i, this);
    }
    return accumulator;
  },
  writable: true,
  configurable: true
});

Object.defineProperty(Array.prototype, 'find', {
  value: function find(predicate, thisArg) {
    requireCallable(predicate, 'find');
    var len = this.length;
    for (var i = 0; i < len; i++) {
      if (i in this && predicate.call(thisArg, this[i], i, this)) return this[i];
    }
    return undefined;
  },
  writable: true,
  configurable: true
});

Object.defineProperty(Array.prototype, 'findLast', {
  value: function findLast(predicate, thisArg) {
    requireCallable(predicate, 'findLast');
    var len = this.length;
    for (var i = len - 1; i >= 0; i--) {
      if (i in this && predicate.call(thisArg, this[i], i, this)) return this[i];
    }
    return undefined;
  },
  writable: true,
  configurable: true
});

Object.defineProperty(Array.prototype, 'findLastIndex', {
  value: function findLastIndex(predicate, thisArg) {
    requireCallable(predicate, 'findLastIndex');
    var len = this.length;
    for (var i = len - 1; i >= 0; i--) {
      if (i in this && predicate.call(thisArg, this[i], i, this)) return i;
    }
    return -1;
  },
  writable: true,
  configurable: true
});