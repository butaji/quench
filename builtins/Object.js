// Self-hosted Object builtins on top of __ops__
var ops = __ops__;
var SameValue = ops.SameValue;
var ThrowTypeError = ops.ThrowTypeError;
var EnumerableOwnKeys = ops.EnumerableOwnKeys;
var ToObject = ops.ToObject;

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
