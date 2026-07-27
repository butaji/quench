// Self-hosted Array builtins on top of __ops__
var ops = __ops__;
var IsCallable = ops.IsCallable;
var IsArray = ops.IsArray;
var ToObject = ops.ToObject;
var ThrowTypeError = ops.ThrowTypeError;
var SameValueZero = ops.SameValueZero;

// Array.isArray (ES2025 §23.1.2.3)
Array.isArray = function ArrayIsArray(arg) {
  return IsArray(arg);
};

// Array.prototype.forEach (ES2025 §23.1.3.17)
Array.prototype.forEach = function ArrayForEach(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.forEach called on null or undefined");
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var O = ToObject(this);
  var len = O.length >>> 0;
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  for (var k = 0; k < len; k++) {
    if (k in O) {
      callbackfn.call(thisArg, O[k], k, O);
    }
  }
  return undefined;
};

// Array.prototype.map (ES2025 §23.1.3.22)
Array.prototype.map = function ArrayMap(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.map called on null or undefined");
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var O = ToObject(this);
  var len = O.length >>> 0;
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  var A = new Array(len);
  for (var k = 0; k < len; k++) {
    if (k in O) {
      A[k] = callbackfn.call(thisArg, O[k], k, O);
    }
  }
  return A;
};

// Array.prototype.filter (ES2025 §23.1.3.12)
Array.prototype.filter = function ArrayFilter(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.filter called on null or undefined");
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var O = ToObject(this);
  var len = O.length >>> 0;
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  var A = new Array(0);
  var to = 0;
  for (var k = 0; k < len; k++) {
    if (k in O) {
      var kValue = O[k];
      if (callbackfn.call(thisArg, kValue, k, O)) {
        A[to++] = kValue;
      }
    }
  }
  return A;
};

// Array.prototype.reduce (ES2025 §23.1.3.28)
Array.prototype.reduce = function ArrayReduce(callbackfn /*, initialValue */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.reduce called on null or undefined");
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var O = ToObject(this);
  var len = O.length >>> 0;
  if (len === 0 && arguments.length < 2) throw ThrowTypeError("Reduce of empty array with no initial value");
  var k = 0;
  var accumulator = arguments.length >= 2 ? arguments[1] : undefined;
  if (arguments.length < 2) {
    while (k < len && !(k in O)) k++;
    accumulator = O[k++];
  }
  for (; k < len; k++) {
    if (k in O) {
      accumulator = callbackfn.call(undefined, accumulator, O[k], k, O);
    }
  }
  return accumulator;
};

// Array.prototype.find (ES2025 §23.1.3.16)
Array.prototype.find = function ArrayFind(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.find called on null or undefined");
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var O = ToObject(this);
  var len = O.length >>> 0;
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  for (var k = 0; k < len; k++) {
    if (k in O) {
      var kValue = O[k];
      if (callbackfn.call(thisArg, kValue, k, O)) return kValue;
    }
  }
  return undefined;
};

// Array.prototype.some (ES2025 §23.1.3.30)
Array.prototype.some = function ArraySome(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.some called on null or undefined");
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var O = ToObject(this);
  var len = O.length >>> 0;
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  for (var k = 0; k < len; k++) {
    if (k in O) {
      if (callbackfn.call(thisArg, O[k], k, O)) return true;
    }
  }
  return false;
};

// Array.prototype.every (ES2025 §23.1.3.10)
Array.prototype.every = function ArrayEvery(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.every called on null or undefined");
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var O = ToObject(this);
  var len = O.length >>> 0;
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  for (var k = 0; k < len; k++) {
    if (k in O) {
      if (!callbackfn.call(thisArg, O[k], k, O)) return false;
    }
  }
  return true;
};

// Array.prototype.includes (ES2025 §23.1.3.18)
Array.prototype.includes = function ArrayIncludes(searchElement /*, fromIndex */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.includes called on null or undefined");
  var O = ToObject(this);
  var len = O.length >>> 0;
  if (len === 0) return false;
  var n = arguments.length > 1 ? arguments[1] : 0;
  var k = Math.max(n >= 0 ? n : len + n, 0);
  for (; k < len; k++) {
    if (k in O && SameValueZero(O[k], searchElement)) return true;
  }
  return false;
};

// Array.prototype.indexOf (ES2025 §23.1.3.19)
Array.prototype.indexOf = function ArrayIndexOf(searchElement /*, fromIndex */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.indexOf called on null or undefined");
  var O = ToObject(this);
  var len = O.length >>> 0;
  if (len === 0) return -1;
  var n = arguments.length > 1 ? arguments[1] : 0;
  var k = Math.max(n >= 0 ? n : len + n, 0);
  for (; k < len; k++) {
    if (k in O && O[k] === searchElement) return k;
  }
  return -1;
};

// Array.prototype.join (ES2025 §23.1.3.20)
Array.prototype.join = function ArrayJoin(separator) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.join called on null or undefined");
  var O = ToObject(this);
  var len = O.length >>> 0;
  var sep = separator === undefined ? ',' : String(separator);
  if (len === 0) return '';
  var R = '';
  for (var k = 0; k < len; k++) {
    if (k > 0) R += sep;
    if (k in O) {
      var elem = O[k];
      R += (elem === null || elem === undefined) ? '' : String(elem);
    }
  }
  return R;
};
