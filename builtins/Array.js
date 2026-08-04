// Self-hosted Array builtins on top of __ops__
var ops = __ops__;
var IsCallable = ops.IsCallable;
var IsArray = ops.IsArray;
var ToObject = ops.ToObject;
var ThrowTypeError = ops.ThrowTypeError;
var SameValueZero = ops.SameValueZero;
var HasProperty = ops.HasProperty;


function ToLength(value) {
  var n = Number(value);
  if (n !== n || n <= 0) return 0;
  if (n === Infinity) return 9007199254740991;
  return Math.min(Math.floor(n), 9007199254740991);
}

function ToIntegerOrInfinity(value) {
  var n = Number(value);
  if (n !== n || n === 0) return 0;
  if (n === Infinity || n === -Infinity) return n;
  return n < 0 ? Math.ceil(n) : Math.floor(n);
}

function CreateDataProperty(O, key, value) {
  Object.defineProperty(O, String(key), {
    value: value,
    writable: true,
    enumerable: true,
    configurable: true
  });
  return true;
}

// Array.isArray (ES2025 §23.1.2.3)
Array.isArray = function ArrayIsArray(arg) {
  return IsArray(arg);
};

Array.of = function ArrayOf() {
  var result = new this(arguments.length);
  for (var i = 0; i < arguments.length; i++) CreateDataProperty(result, i, arguments[i]);
  result.length = arguments.length;
  return result;
};

Array.from = function ArrayFrom(items) {
  if (items === null || items === undefined) throw ThrowTypeError("Array.from requires an object");
  var mapfn = arguments.length > 1 ? arguments[1] : undefined;
  var thisArg = arguments.length > 2 ? arguments[2] : undefined;
  if (mapfn !== undefined && typeof mapfn !== 'function') throw ThrowTypeError("mapfn is not a function");
  var mappedFromIterator = false;
  if (IsArray(items) && mapfn !== undefined) {
    var arrayResult = new Array(0);
    var arrayLength = ToLength(items.length);
    for (var arrayIndex = 0; arrayIndex < arrayLength; arrayIndex++) {
      CreateDataProperty(arrayResult, arrayIndex, mapfn.call(thisArg, items[arrayIndex], arrayIndex));
    }
    arrayResult.length = arrayLength;
    return arrayResult;
  }
  var values = [];
  var iteratorMethod = items[Symbol.iterator];
  if (typeof iteratorMethod === 'function') {
    mappedFromIterator = mapfn !== undefined;
    var iterator = iteratorMethod.call(items);
    var step;
    while (!(step = iterator.next()).done) {
      var value = step.value;
      if (mappedFromIterator) {
        try { value = mapfn.call(thisArg, value, values.length); }
        catch (error) { if (typeof iterator.return === 'function') iterator.return.call(iterator); throw error; }
      }
      values.push(value);
    }
  } else {
    var object = ToObject(items);
    var length = ToLength(object.length);
    for (var i = 0; i < length; i++) values.push(object[i]);
  }
  var result = Reflect.construct(Array, [values.length], this);
  for (var i = 0; i < values.length; i++) CreateDataProperty(result, i, mappedFromIterator ? values[i] : mapfn === undefined ? values[i] : mapfn.call(thisArg, values[i], i));
  return result;
};

Array.fromAsync = async function ArrayFromAsync(items, mapfn, thisArg) {
  if (items === null || items === undefined) throw ThrowTypeError("Array.fromAsync requires an object");
  if (mapfn !== undefined && typeof mapfn !== 'function') throw ThrowTypeError("mapfn is not a function");
  var values = [];
  var index = 0;
  if (!IsArray(items) && (items[Symbol.asyncIterator] !== undefined || items[Symbol.iterator] !== undefined)) {
    for await (var value of items) {
      values.push(mapfn === undefined ? value : await mapfn.call(thisArg, value, index, items));
      index++;
    }
  } else {
    var length = ToLength(items.length);
    for (var i = 0; i < length; i++) {
      var element = await Promise.resolve(items[i]);
      values.push(mapfn === undefined ? element : await mapfn.call(thisArg, element, i, items));
    }
  }
  var result = new this(values.length);
  for (var j = 0; j < values.length; j++) CreateDataProperty(result, j, values[j]);
  result.length = values.length;
  return result;
};

// Array.prototype.forEach (ES2025 §23.1.3.17)
Array.prototype.forEach = function ArrayForEach(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.forEach called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  for (var k = 0; k < len; k++) {
    if (HasProperty(O, k)) {
      callbackfn.call(thisArg, O[k], k, O);
    }
  }
  return undefined;
};

// Array.prototype.map (ES2025 §23.1.3.22)
Array.prototype.map = function ArrayMap(callbackfn /*, thisArg */) {
  "use strict";
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.map called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  var A = new Array(len);
  for (var k = 0; k < len; k++) {
    if (HasProperty(O, k)) {
      CreateDataProperty(A, k, callbackfn.call(thisArg, O[k], k, O));
    }
  }
  return A;
};

// Array.prototype.filter (ES2025 §23.1.3.12)
Array.prototype.filter = function ArrayFilter(callbackfn /*, thisArg */) {
  "use strict";
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.filter called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  var A = new Array(0);
  var to = 0;
  for (var k = 0; k < len; k++) {
    if (HasProperty(O, k)) {
      var kValue = O[k];
      if (callbackfn.call(thisArg, kValue, k, O)) {
        CreateDataProperty(A, to, kValue);
        to = to + 1;
      }
    }
  }
  return A;
};

// Array.prototype.reduce (ES2025 §23.1.3.28)
Array.prototype.reduce = function ArrayReduce(callbackfn /*, initialValue */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.reduce called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  if (len === 0 && arguments.length < 2) throw ThrowTypeError("Reduce of empty array with no initial value");
  var k = 0;
  var accumulator = arguments.length >= 2 ? arguments[1] : undefined;
  if (arguments.length < 2) {
    while (k < len && !HasProperty(O, k)) k++;
    accumulator = O[k++];
  }
  for (; k < len; k++) {
    if (HasProperty(O, k)) {
      accumulator = callbackfn.call(undefined, accumulator, O[k], k, O);
    }
  }
  return accumulator;
};

// Array.prototype.find (ES2025 §23.1.3.16)
Array.prototype.find = function ArrayFind(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.find called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  for (var k = 0; k < len; k++) {
    var kValue = O[k];
    if (callbackfn.call(thisArg, kValue, k, O)) return kValue;
  }
  return undefined;
};

// Array.prototype.some (ES2025 §23.1.3.30)
Array.prototype.some = function ArraySome(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.some called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  for (var k = 0; k < len; k++) {
    if (HasProperty(O, k)) {
      if (callbackfn.call(thisArg, O[k], k, O)) return true;
    }
  }
  return false;
};

// Array.prototype.every (ES2025 §23.1.3.10)
Array.prototype.every = function ArrayEvery(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.every called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  for (var k = 0; k < len; k++) {
    if (HasProperty(O, k)) {
      if (!callbackfn.call(thisArg, O[k], k, O)) return false;
    }
  }
  return true;
};

// Array.prototype.includes (ES2025 §23.1.3.18)
Array.prototype.includes = function ArrayIncludes(searchElement /*, fromIndex */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.includes called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (len === 0) return false;
  var n = arguments.length > 1 ? ToIntegerOrInfinity(arguments[1]) : 0;
  var k = Math.max(n >= 0 ? n : len + n, 0);
  for (; k < len; k++) {
    if (SameValueZero(O[k], searchElement)) return true;
  }
  return false;
};

// Array.prototype.indexOf (ES2025 §23.1.3.19)
Array.prototype.indexOf = function ArrayIndexOf(searchElement /*, fromIndex */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.indexOf called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (len === 0) return -1;
  var n = arguments.length > 1 ? ToIntegerOrInfinity(arguments[1]) : 0;
  var k = Math.max(n >= 0 ? n : len + n, 0);
  for (; k < len; k++) {
    if (HasProperty(O, k) && O[k] === searchElement) return k;
  }
  return -1;
};

// Array.prototype.join (ES2025 §23.1.3.20)
Array.prototype.join = function ArrayJoin(separator) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.join called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var sep = separator === undefined ? ',' : String(separator);
  if (len === 0) return '';
  var R = '';
  for (var k = 0; k < len; k++) {
    if (k > 0) R += sep;
    if (HasProperty(O, k)) {
      var elem = O[k];
      R += (elem === null || elem === undefined) ? '' : String(elem);
    }
  }
  return R;
};

Array.prototype.toLocaleString = function ArrayToLocaleString(locales, options) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.toLocaleString called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var R = '';
  for (var k = 0; k < len; k++) {
    if (k > 0) R += ',';
    if (!HasProperty(O, k)) continue;
    var value = O[k];
    if (value === null || value === undefined) continue;
    var method = value.toLocaleString;
    if (!IsCallable(method)) throw ThrowTypeError("Array element toLocaleString is not callable");
    R += method.call(value);
  }
  return R;
};

Array.prototype.push = function ArrayPush() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.push called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  for (var i = 0; i < arguments.length; i++) O[len + i] = arguments[i];
  O.length = len + arguments.length;
  return O.length;
};

Array.prototype.pop = function ArrayPop() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.pop called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
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
  var len = ToLength(O.length);
  var relativeStart = start === undefined ? 0 : ToIntegerOrInfinity(start);
  var k = relativeStart >= 0 ? relativeStart : Math.max(len + relativeStart, 0);
  var relativeEnd = end === undefined ? len : ToIntegerOrInfinity(end);
  var final = relativeEnd >= 0 ? Math.min(relativeEnd, len) : Math.max(len + relativeEnd, 0);
  var count = Math.max(final - k, 0);
  var A = new Array(count);
  var n = 0;
  while (k < final) {
    if (HasProperty(O, k)) CreateDataProperty(A, n, O[k]);
    k++;
    n++;
  }
  return A;
};

// Array.prototype.concat (ES2025 §23.1.3.4)
Array.prototype.concat = function ArrayConcat() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.concat called on null or undefined");
  var O = ToObject(this);
  if (IsArray(O)) O.constructor;
  var A = new Array(0);
  var n = 0;
  var items = [O];
  for (var i = 0; i < arguments.length; i++) items.push(arguments[i]);
  for (var i = 0; i < items.length; i++) {
    var E = items[i];
    var spreadable = IsArray(E);
    if (E !== null && E !== undefined) {
      var spreadFlag = E[Symbol.isConcatSpreadable];
      if (spreadFlag !== undefined) spreadable = Boolean(spreadFlag);
    }
    if (spreadable) {
      var k = 0;
      var len = ToLength(E.length);
      while (k < len) {
        var element = E[k];
        if (element !== undefined || k in E) {
          CreateDataProperty(A, n, element);
        }
        n = n + 1;
        k++;
      }
    } else {
      CreateDataProperty(A, n, E);
      n = n + 1;
    }
  }
  A.length = n;
  return A;
};

// Array.prototype.reverse (ES2025 §23.1.3.27)
Array.prototype.reverse = function ArrayReverse() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.reverse called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var middle = Math.floor(len / 2);
  var lower = 0;
  while (lower !== middle) {
    var upper = len - lower - 1;
    var lowerExists = HasProperty(O, lower);
    var upperExists = HasProperty(O, upper);
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

// Array.prototype.fill (ES2025 §23.1.3.11)
Array.prototype.fill = function ArrayFill(value /*, start, end */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.fill called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var relativeStart = arguments.length > 1 ? ToIntegerOrInfinity(arguments[1]) : 0;
  var k = relativeStart >= 0 ? relativeStart : Math.max(len + relativeStart, 0);
  var relativeEnd = arguments.length > 2 && arguments[2] !== undefined ? ToIntegerOrInfinity(arguments[2]) : len;
  var final = relativeEnd >= 0 ? Math.min(relativeEnd, len) : Math.max(len + relativeEnd, 0);
  while (k < final) { O[k] = value; k++; }
  return O;
};

// Array.prototype.reduceRight (ES2025 §23.1.3.29)
Array.prototype.reduceRight = function ArrayReduceRight(callbackfn /*, initialValue */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.reduceRight called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  if (len === 0 && arguments.length < 2) throw ThrowTypeError("Reduce of empty array with no initial value");
  var k = len - 1;
  var accumulator = arguments.length >= 2 ? arguments[1] : undefined;
  if (arguments.length < 2) {
    while (k >= 0 && !HasProperty(O, k)) k--;
    accumulator = O[k--];
  }
  for (; k >= 0; k--) {
    if (HasProperty(O, k)) accumulator = callbackfn.call(undefined, accumulator, O[k], k, O);
  }
  return accumulator;
};

// Array.prototype.keys (ES2025 §23.1.3.21)
function ArrayIteratorNext() {
  var O = this._array;
  var index = this._index;
  if (this._done) return { value: undefined, done: true };
  var len = ToLength(O.length);
  if (index >= len) { this._done = true; return { value: undefined, done: true }; }
  this._index = index + 1;
  if (this._kind === 0) return { value: index, done: false };
  var value = O[index];
  if (this._kind === 1) return { value: value, done: false };
  return { value: [index, value], done: false };
}

function ArrayIterator(O, kind) {
  var iterator = { _array: O, _index: 0, _kind: kind, _done: false };
  iterator.next = ArrayIteratorNext;
  iterator[Symbol.iterator] = function() { return this; };
  return iterator;
}

Array.prototype.keys = function ArrayKeys() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.keys called on null or undefined");
  var O = ToObject(this);
  return ArrayIterator(O, 0);
};

// Array.prototype.values (ES2025 §23.1.3.33)
Array.prototype.values = function ArrayValues() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.values called on null or undefined");
  var O = ToObject(this);
  return ArrayIterator(O, 1);
};

// Array.prototype.entries (ES2025 §23.1.3.7)
Array.prototype.entries = function ArrayEntries() {
  "use strict";
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.entries called on null or undefined");
  var O = ToObject(this);
  return ArrayIterator(O, 2);
};

Array.prototype[Symbol.iterator] = Array.prototype.values;

// Array.prototype.findIndex (ES2025 §23.1.3.15)
Array.prototype.findIndex = function ArrayFindIndex(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.findIndex called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  for (var k = 0; k < len; k++) {
    if (callbackfn.call(thisArg, O[k], k, O)) return k;
  }
  return -1;
};

Array.prototype.copyWithin = function ArrayCopyWithin(target, start, end) {
  "use strict";
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.copyWithin called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var relativeTarget = ToIntegerOrInfinity(target);
  var relativeStart = ToIntegerOrInfinity(start);
  var to = relativeTarget >= 0 ? Math.min(relativeTarget, len) : Math.max(len + relativeTarget, 0);
  var from = relativeStart >= 0 ? Math.min(relativeStart, len) : Math.max(len + relativeStart, 0);
  var relativeEnd = end === undefined ? len : ToIntegerOrInfinity(end);
  var final = relativeEnd >= 0 ? Math.min(relativeEnd, len) : Math.max(len + relativeEnd, 0);
  var count = Math.min(final - from, len - to);
  var direction = 1;
  if (from < to && to < from + count) { from += count - 1; to += count - 1; direction = -1; }
  while (count > 0) {
    if (HasProperty(O, from)) O[to] = O[from];
    else if (!delete O[to]) throw ThrowTypeError("Cannot delete array property");
    from += direction; to += direction; count--;
  }
  return O;
};

Array.prototype.sort = function ArraySort(comparefn) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.sort called on null or undefined");
  if (comparefn !== undefined && !IsCallable(comparefn)) throw ThrowTypeError("comparefn is not a function");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var values = [];
  for (var i = 0; i < len; i++) if (HasProperty(O, i)) values.push(O[i]);
  for (var i = 1; i < values.length; i++) {
    var value = values[i];
    var j = i - 1;
    while (j >= 0) {
      var order;
      if (values[j] === undefined) order = value !== undefined;
      else if (value === undefined) order = false;
      else order = comparefn === undefined ? String(values[j]) > String(value) : comparefn(values[j], value) > 0;
      if (!order) break;
      values[j + 1] = values[j]; j--;
    }
    values[j + 1] = value;
  }
  for (var i = 0; i < len; i++) { if (i < values.length) O[i] = values[i]; else delete O[i]; }
  return O;
};

Array.prototype.toString = function ArrayToString() {
  var O = ToObject(this);
  var join = O.join;
  return IsCallable(join) ? join.call(O) : Object.prototype.toString.call(O);
};

Array.prototype.at = function ArrayAt(index) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.at called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var relativeIndex = ToIntegerOrInfinity(index);
  var k = relativeIndex >= 0 ? relativeIndex : len + relativeIndex;
  return k < 0 || k >= len ? undefined : O[k];
};

Array.prototype.lastIndexOf = function ArrayLastIndexOf(searchElement, fromIndex) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.lastIndexOf called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (len === 0) return -1;
  var relativeFrom = fromIndex === undefined ? len - 1 : ToIntegerOrInfinity(fromIndex);
  var k = relativeFrom >= 0 ? Math.min(relativeFrom, len - 1) : len + relativeFrom;
  for (; k >= 0; k--) if (HasProperty(O, k) && O[k] === searchElement) return k;
  return -1;
};

Array.prototype.toSorted = function ArrayToSorted(comparefn) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.toSorted called on null or undefined");
  var result = Array.prototype.slice.call(this);
  return result.sort(comparefn);
};

// Array.prototype.splice (ES2025 §23.1.3.30)
Array.prototype.splice = function ArraySplice(start, deleteCount /*, ...items */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.splice called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var relativeStart = ToIntegerOrInfinity(start);
  var actualStart = relativeStart < 0 ? Math.max(len + relativeStart, 0) : Math.min(relativeStart, len);
  var actualDeleteCount;
  if (arguments.length === 1) {
    actualDeleteCount = len - actualStart;
  } else {
    actualDeleteCount = Math.min(Math.max(ToIntegerOrInfinity(deleteCount), 0), len - actualStart);
  }
  var itemCount = Math.max(arguments.length - 2, 0);
  // Gather deleted
  var removed = new Array(actualDeleteCount);
  for (var i = 0; i < actualDeleteCount; i++) {
    if (HasProperty(O, actualStart + i)) CreateDataProperty(removed, i, O[actualStart + i]);
  }
  // Move elements
  if (itemCount < actualDeleteCount) {
    for (var i = actualStart; i < len - actualDeleteCount; i++) {
      var from = i + actualDeleteCount;
      var to = i + itemCount;
      if (HasProperty(O, from)) O[to] = O[from]; else delete O[to];
    }
    for (var i = len - 1; i >= len - (actualDeleteCount - itemCount); i--) delete O[i];
  } else if (itemCount > actualDeleteCount) {
    for (var i = len - 1; i >= actualStart; i--) {
      var from = i;
      var to = i + (itemCount - actualDeleteCount);
      if (HasProperty(O, from)) O[to] = O[from]; else delete O[to];
    }
  }
  // Insert new items
  for (var i = 0; i < itemCount; i++) O[actualStart + i] = arguments[i + 2];
  O.length = len - actualDeleteCount + itemCount;
  return removed;
};

// Array.prototype.unshift (ES2025 §23.1.3.32)
Array.prototype.unshift = function ArrayUnshift() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.unshift called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var argCount = arguments.length;
  if (argCount > 0) {
    for (var k = len - 1; k >= 0; k--) {
    if (HasProperty(O, k)) O[k + argCount] = O[k]; else delete O[k + argCount];
    }
    for (var j = 0; j < argCount; j++) O[j] = arguments[j];
  }
  var newLen = len + argCount;
  O.length = newLen;
  return newLen;
};

// Array.prototype.shift (ES2025 §23.1.3.29)
Array.prototype.shift = function ArrayShift() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.shift called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (len === 0) { O.length = 0; return undefined; }
  var first = O[0];
  for (var k = 1; k < len; k++) {
    if (HasProperty(O, k)) O[k - 1] = O[k]; else delete O[k - 1];
  }
  delete O[len - 1];
  O.length = len - 1;
  return first;
};

// Array.prototype.flat (ES2025 §23.1.3.13)
Array.prototype.flat = function ArrayFlat() {
  "use strict";
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.flat called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var depth = arguments.length === 0 ? undefined : arguments[0];
  var d = depth === undefined ? 1 : ToIntegerOrInfinity(depth);
  var result = [];
  var resultIndex = 0;
  for (var i = 0; i < len; i++) {
    if (HasProperty(O, i)) {
      if (d > 0 && IsArray(O[i])) {
        var sub = O[i].flat(d - 1);
        for (var j = 0; j < sub.length; j++) CreateDataProperty(result, resultIndex++, sub[j]);
      } else {
        CreateDataProperty(result, resultIndex++, O[i]);
      }
    }
  }
  return result;
};

// Array.prototype.flatMap (ES2025 §23.1.3.14)
Array.prototype.flatMap = function ArrayFlatMap(callbackfn /*, thisArg */) {
  "use strict";
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.flatMap called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  var result = [];
  var resultIndex = 0;
  for (var i = 0; i < len; i++) {
    if (HasProperty(O, i)) {
      var val = callbackfn.call(thisArg, O[i], i, O);
      if (IsArray(val)) { for (var j = 0; j < val.length; j++) CreateDataProperty(result, resultIndex++, val[j]); }
      else { CreateDataProperty(result, resultIndex++, val); }
    }
  }
  return result;
};

// Array.prototype.toReversed (ES2023 §23.1.3.35)
Array.prototype.toReversed = function ArrayToReversed() {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.toReversed called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var A = new Array(len);
  for (var i = 0; i < len; i++) {
    var from = len - 1 - i;
    CreateDataProperty(A, i, O[from]);
  }
  return A;
};

// Array.prototype.toSpliced (ES2023 §23.1.3.37)
Array.prototype.toSpliced = function ArrayToSpliced(start, deleteCount /*, ...items */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.toSpliced called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var relativeStart = ToIntegerOrInfinity(start);
  var actualStart = relativeStart < 0 ? Math.max(len + relativeStart, 0) : Math.min(relativeStart, len);
  var actualDeleteCount;
  if (arguments.length === 1) actualDeleteCount = len - actualStart;
  else actualDeleteCount = Math.min(Math.max(ToIntegerOrInfinity(deleteCount), 0), len - actualStart);
  var itemCount = Math.max(arguments.length - 2, 0);
  var newLen = len - actualDeleteCount + itemCount;
  var A = new Array(newLen);
  for (var i = 0; i < actualStart; i++) CreateDataProperty(A, i, O[i]);
  for (var i = 0; i < itemCount; i++) CreateDataProperty(A, actualStart + i, arguments[i + 2]);
  for (var i = actualStart + actualDeleteCount; i < len; i++) {
    CreateDataProperty(A, i - actualDeleteCount + itemCount, O[i]);
  }
  return A;
};

// Array.prototype.with (ES2023 §23.1.3.39)
Array.prototype.with = function ArrayWith(index, value) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.with called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  var relativeIndex = ToIntegerOrInfinity(index);
  var actualIndex = relativeIndex < 0 ? len + relativeIndex : relativeIndex;
  if (actualIndex < 0 || actualIndex >= len) throw new RangeError("Array.prototype.with index out of range");
  var A = new Array(len);
  for (var i = 0; i < len; i++) {
    CreateDataProperty(A, i, O[i]);
  }
  CreateDataProperty(A, actualIndex, value);
  return A;
};

// Array.prototype.findLast (ES2023 §23.1.3.14)
Array.prototype.findLast = function ArrayFindLast(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.findLast called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  for (var k = len - 1; k >= 0; k--) {
    if (callbackfn.call(thisArg, O[k], k, O)) return O[k];
  }
  return undefined;
};

// Array.prototype.findLastIndex (ES2023 §23.1.3.15)
Array.prototype.findLastIndex = function ArrayFindLastIndex(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.findLastIndex called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  for (var k = len - 1; k >= 0; k--) {
    if (callbackfn.call(thisArg, O[k], k, O)) return k;
  }
  return -1;
};

// Array.prototype.group (ES2025 §23.1.3.18)
Array.prototype.group = function ArrayGroup(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.group called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  var groups = {};
  for (var k = 0; k < len; k++) {
    if (HasProperty(O, k)) {
      var key = callbackfn.call(thisArg, O[k], k, O);
      if (groups[key] === undefined) groups[key] = [];
      groups[key].push(O[k]);
    }
  }
  return groups;
};

// Array.prototype.groupToMap (ES2025 §23.1.3.19)
Array.prototype.groupToMap = function ArrayGroupToMap(callbackfn /*, thisArg */) {
  if (this === null || this === undefined) throw ThrowTypeError("Array.prototype.groupToMap called on null or undefined");
  var O = ToObject(this);
  var len = ToLength(O.length);
  if (!IsCallable(callbackfn)) throw ThrowTypeError("callbackfn is not a function");
  var thisArg = arguments.length > 1 ? arguments[1] : undefined;
  var map = new Map();
  for (var k = 0; k < len; k++) {
    if (HasProperty(O, k)) {
      var key = callbackfn.call(thisArg, O[k], k, O);
      var items = map.get(key);
      if (items === undefined) { items = []; map.set(key, items); }
      items.push(O[k]);
    }
  }
  return map;
};
