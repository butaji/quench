// Self-hosted Array builtins on top of __ops__
var ops = __ops__;
var IsCallable = ops.IsCallable;
var IsArray = ops.IsArray;
var ToObject = ops.ToObject;
var ThrowTypeError = ops.ThrowTypeError;

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
