// Self-hosted Object builtins on top of __ops__
var ops = __ops__;
var SameValue = ops.SameValue;
var ThrowTypeError = ops.ThrowTypeError;
var EnumerableOwnKeys = ops.EnumerableOwnKeys;
var ToObject = ops.ToObject;
var IsExtensible = ops.IsExtensible;
var IsCallable = ops.IsCallable;
var HasProperty = ops.HasProperty;
var HasOwnProperty = ops.HasOwnProperty;
var GetPrototypeOf = ops.GetPrototypeOf;
var SetPrototypeOf = ops.SetPrototypeOf;
var PreventExtensions = ops.PreventExtensions;
var SealObject = ops.SealObject;
var FreezeObject = ops.FreezeObject;
var IsSealedObject = ops.IsSealedObject;
var IsFrozenObject = ops.IsFrozenObject;
var DefineProp = ops.DefineProp;
var GetOwnPropDesc = ops.GetOwnPropDesc;
var OwnKeys = ops.OwnKeys;
var CreateObject = ops.CreateObject;

// Object.getOwnPropertyDescriptor (ES2025 §20.1.2.7)
Object.getOwnPropertyDescriptor = function ObjectGetOwnPropertyDescriptor(O, P) {
  if (O === null || O === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  return GetOwnPropDesc(O, P);
};

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
  for (var i = 0; i < len; i++) values[i] = obj[keys[i]];
  return values;
};

// Object.entries (ES2025 §20.1.2.5)
Object.entries = function ObjectEntries(O) {
  var obj = ToObject(O);
  var keys = EnumerableOwnKeys(obj);
  var len = keys.length;
  var entries = new Array(len);
  for (var i = 0; i < len; i++) entries[i] = [keys[i], obj[keys[i]]];
  return entries;
};

Object.assign = function ObjectAssign(target) {
  if (target === null || target === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  var to = ToObject(target);
  for (var i = 1; i < arguments.length; i++) {
    var source = arguments[i];
    if (source === null || source === undefined) continue;
    var from = ToObject(source);
    var keys = EnumerableOwnKeys(from);
    for (var j = 0; j < keys.length; j++) to[keys[j]] = from[keys[j]];
  }
  return to;
};

// Object.hasOwn (ES2025 §20.1.2.14)
Object.hasOwn = function ObjectHasOwn(O, P) {
  return HasOwnProperty(ToObject(O), P);
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
  var it = iterable[Symbol.iterator]();
  while (true) {
    var result = it.next();
    if (result.done) break;
    var entry = result.value;
    obj[entry[0]] = entry[1];
  }
  return obj;
};

// Object.getPrototypeOf (ES2025 §20.1.2.10)
Object.getPrototypeOf = function ObjectGetPrototypeOf(O) {
  if (O === null || O === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  return GetPrototypeOf(O);
};

// Object.setPrototypeOf (ES2025 §20.1.2.18)
Object.setPrototypeOf = function ObjectSetPrototypeOf(O, proto) {
  if (O === null || O === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  if (proto !== null && typeof proto !== 'object') throw ThrowTypeError("proto must be an object or null");
  SetPrototypeOf(O, proto);
  return O;
};

// Object.preventExtensions (ES2025 §20.1.2.15)
Object.preventExtensions = function ObjectPreventExtensions(O) {
  if (O === null || O === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  PreventExtensions(O);
  return O;
};

// Object.seal (ES2025 §20.1.2.19)
Object.seal = function ObjectSeal(O) {
  if (O === null || O === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  SealObject(O);
  return O;
};

// Object.freeze (ES2025 §20.1.2.9)
Object.freeze = function ObjectFreeze(O) {
  if (O === null || O === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  FreezeObject(O);
  return O;
};

// Object.isSealed (ES2025 §20.1.2.20)
Object.isSealed = function ObjectIsSealed(O) {
  if (O === null || O === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  return IsSealedObject(O);
};

// Object.isFrozen (ES2025 §20.1.2.13)
Object.isFrozen = function ObjectIsFrozen(O) {
  if (O === null || O === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  return IsFrozenObject(O);
};

// Object.getOwnPropertyDescriptors (ES2025 §20.1.2.6)
Object.getOwnPropertyDescriptors = function ObjectGetOwnPropertyDescriptors(O) {
  if (O === null || O === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  var obj = ToObject(O);
  var keys = OwnKeys(obj);
  var descriptors = {};
  for (var i = 0; i < keys.length; i++) {
    var key = keys[i];
    var desc = GetOwnPropDesc(obj, key);
    if (desc !== undefined) descriptors[key] = desc;
  }
  return descriptors;
};

// Object.getOwnPropertyNames (ES2025 §20.1.2.11)
Object.getOwnPropertyNames = function ObjectGetOwnPropertyNames(O) {
  if (O === null || O === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  return OwnKeys(ToObject(O));
};

Object.getOwnPropertySymbols = function ObjectGetOwnPropertySymbols(O) {
  if (O === null || O === undefined) throw ThrowTypeError("Cannot convert undefined or null to object");
  var keys = OwnKeys(ToObject(O));
  var symbols = [];
  for (var i = 0; i < keys.length; i++) if (typeof keys[i] === 'symbol') symbols.push(keys[i]);
  return symbols;
};

// Object.create (ES2025 §20.1.2.2)
Object.create = function ObjectCreate(proto, properties) {
  if (proto !== null && typeof proto !== 'object') throw ThrowTypeError("Object prototype may only be an Object or null");
  var obj = CreateObject(proto);
  if (properties !== undefined) Object.defineProperties(obj, properties);
  return obj;
};
