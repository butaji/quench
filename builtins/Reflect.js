// Self-hosted Reflect methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;
var ToObject = ops.ToObject;
var HasProperty = ops.HasProperty;
var OwnKeys = ops.OwnKeys;
var GetOwnPropDesc = ops.GetOwnPropDesc;
var PreventExtensions = ops.PreventExtensions;
var IsExtensible = ops.IsExtensible;

var _nativeGet = Reflect.get;
var _nativeSet = Reflect.set;
var _nativeDeleteProperty = Reflect.deleteProperty;
var _nativeConstruct = Reflect.construct;
var _nativeDefineProperty = Reflect.defineProperty;

Reflect.get = function(target, propertyKey, receiver) {
  return _nativeGet(target, propertyKey, receiver);
};
Reflect.set = function(target, propertyKey, value, receiver) {
  return _nativeSet(target, propertyKey, value, receiver);
};
Reflect.has = function(target, propertyKey) {
  return HasProperty(ToObject(target), propertyKey);
};
Reflect.deleteProperty = function(target, propertyKey) {
  return _nativeDeleteProperty(target, propertyKey);
};
Reflect.construct = function(target, argumentsList, newTarget) {
  return _nativeConstruct(target, argumentsList, newTarget);
};
Reflect.defineProperty = function(target, propertyKey, attributes) {
  return _nativeDefineProperty(target, propertyKey, attributes);
};
Reflect.getPrototypeOf = function(target) {
  return ops.GetPrototypeOf(target);
};
Reflect.setPrototypeOf = function(target, proto) {
  return ops.SetPrototypeOf(target, proto);
};
Reflect.isExtensible = function(target) {
  return IsExtensible(target);
};
Reflect.preventExtensions = function(target) {
  PreventExtensions(target);
  return true;
};
Reflect.ownKeys = function(target) {
  return OwnKeys(ToObject(target));
};
Reflect.getOwnPropertyDescriptor = function(target, propertyKey) {
  return GetOwnPropDesc(ToObject(target), propertyKey);
};
