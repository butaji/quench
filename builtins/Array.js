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

// Array.prototype.push (ES2025 §23.1.3.24)
Array.prototype.push = function ArrayPush() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.push called on null or undefined");
  var O = ToObject(this);
  var len = O.length >>> 0;
  var argCount = arguments.length;
  for (var n = 0; n < argCount; n++) {
    O[len + n] = arguments[n];
  }
  var newLen = len + argCount;
  O.length = newLen;
  return newLen;
};

// Array.prototype.pop (ES2025 §23.1.3.23)
Array.prototype.pop = function ArrayPop() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.pop called on null or undefined");
  var O = ToObject(this);
  var len = O.length >>> 0;
  if (len === 0) {
    O.length = 0;
    return undefined;
  }
  len--;
  var element = O[len];
  delete O[len];
  O.length = len;
  return element;
};

// Array.prototype.slice (ES2025 §23.1.3.29)
Array.prototype.slice = function ArraySlice(start, end) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.slice called on null or undefined");
  var O = ToObject(this);
  var len = O.length >>> 0;
  var relativeStart = start === undefined ? 0 : start;
  var k = relativeStart >= 0 ? relativeStart : Math.max(len + relativeStart, 0);
  var relativeEnd = end === undefined ? len : end;
  var final = relativeEnd >= 0 ? Math.min(relativeEnd, len) : Math.max(len + relativeEnd, 0);
  var count = Math.max(final - k, 0);
  var A = new Array(count);
  var n = 0;
  while (k < final) {
    if (k in O) A[n] = O[k];
    k++;
    n++;
  }
  return A;
};

// Array.prototype.concat (ES2025 §23.1.3.4)
Array.prototype.concat = function ArrayConcat() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.concat called on null or undefined");
  var O = ToObject(this);
  var A = new Array(0);
  var n = 0;
  var items = [O];
  for (var i = 0; i < arguments.length; i++) items.push(arguments[i]);
  for (var i = 0; i < items.length; i++) {
    var E = items[i];
    var spreadable = IsArray(E);
    if (spreadable) {
      var k = 0;
      var len = E.length >>> 0;
      while (k < len) {
        if (k in E) A[n++] = E[k];
        k++;
      }
    } else {
      A[n++] = E;
    }
  }
  return A;
};

// Array.prototype.reverse (ES2025 §23.1.3.27)
Array.prototype.reverse = function ArrayReverse() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.reverse called on null or undefined");
  var O = ToObject(this);
  var len = O.length >>> 0;
  var middle = Math.floor(len / 2);
  var lower = 0;
  while (lower !== middle) {
    var upper = len - lower - 1;
    var lowerExists = lower in O;
    var upperExists = upper in O;
    var lowerValue = lowerExists ? O[lower] : undefined;
    var upperValue = upperExists ? O[upper] : undefined;
    if (lowerExists && upperExists) {
      O[lower] = upperValue;
      O[upper] = lowerValue;
    } else if (lowerExists && !upperExists) {
      O[upper] = lowerValue;
      delete O[lower];
    } else if (!lowerExists && upperExists) {
      O[lower] = upperValue;
      delete O[upper];
    }
    lower++;
  }
  return O;
};
