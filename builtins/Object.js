// Self-hosted Object builtins on top of __ops__
var ops = __ops__;
var SameValue = ops.SameValue;
var ThrowTypeError = ops.ThrowTypeError;
var EnumerableOwnKeys = ops.EnumerableOwnKeys;
var ToObject = ops.ToObject;
var GetProperty = ops.GetProperty;
var SetProperty = ops.SetProperty;
var IsExtensible = ops.IsExtensible;
var IsCallable = ops.IsCallable;
var HasProperty = ops.HasProperty;

// Object.is (ES2025 §20.1.2.12)
Object.is = function ObjectIs(value1, value2) {
  return SameValue(value1, value2);
};

// Object.keys (ES2025 §20.1.2.17)
Object.keys = function ObjectKeys(O) {
  return EnumerableOwnKeys(ToObject(O));
};

// Object.values (ES2025 §20.1.2.23)
Object.values = function ObjectValues(O) {
  var obj = ToObject(O);
  var keys = EnumerableOwnKeys(obj);
  var len = keys.length;
  var values = new Array(len);
  for (var i = 0; i < len; i++) {
    values[i] = obj[keys[i]];
  }
  return values;
};

// Object.entries (ES2025 §20.1.2.5)
Object.entries = function ObjectEntries(O) {
  var obj = ToObject(O);
  var keys = EnumerableOwnKeys(obj);
  var len = keys.length;
  var entries = new Array(len);
  for (var i = 0; i < len; i++) {
    entries[i] = [keys[i], obj[keys[i]]];
  }
  return entries;
};

// Object.assign (ES2025 §20.1.2.1)
Object.assign = function ObjectAssign(target, ...sources) {
  var to = ToObject(target);
  var len = sources.length;
  for (var i = 0; i < len; i++) {
    var nextSource = sources[i];
    if (nextSource === null || nextSource === undefined) continue;
    var from = ToObject(nextSource);
    var keys = EnumerableOwnKeys(from);
    var keysLen = keys.length;
    for (var j = 0; j < keysLen; j++) {
      var key = keys[j];
      to[key] = from[key];
    }
  }
  return to;
};

// Object.hasOwn (ES2025 §20.1.2.14)
Object.hasOwn = function ObjectHasOwn(O, P) {
  return HasProperty(ToObject(O), P);
};

// Object.isExtensible (ES2025 §20.1.2.16)
Object.isExtensible = function ObjectIsExtensible(O) {
  if (O === null || O === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  return IsExtensible(O);
};

// Object.fromEntries (ES2025 §20.1.2.8)
Object.fromEntries = function ObjectFromEntries(iterable) {
  if (iterable === null || iterable === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  var obj = {};
  var iterator = iterable[Symbol.iterator]();
  if (!iterator) throw ThrowTypeError("iterable is not iterable");
  var result;
  while (!(result = iterator.next()).done) {
    var entry = result.value;
    if (entry === null || entry === undefined) throw ThrowTypeError("Iterator value is not an object");
    obj[entry[0]] = entry[1];
  }
  return obj;
};
