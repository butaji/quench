// Self-hosted Reflect methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;

var _nativeGet = Reflect.get;
var _nativeSet = Reflect.set;
var _nativeHas = Reflect.has;
var _nativeDeleteProperty = Reflect.deleteProperty;
var _nativeConstruct = Reflect.construct;
var _nativeDefineProperty = Reflect.defineProperty;
var _nativeGetPrototypeOf = Reflect.getPrototypeOf;
var _nativeSetPrototypeOf = Reflect.setPrototypeOf;
var _nativeIsExtensible = Reflect.isExtensible;
var _nativePreventExtensions = Reflect.preventExtensions;
var _nativeOwnKeys = Reflect.ownKeys;
var _nativeGetOwnPropertyDescriptor = Reflect.getOwnPropertyDescriptor;

Reflect.get = function(target, propertyKey, receiver) {
  return _nativeGet(target, propertyKey, receiver);
};
Reflect.set = function(target, propertyKey, value, receiver) {
  return _nativeSet(target, propertyKey, value, receiver);
};
Reflect.has = function(target, propertyKey) {
  return _nativeHas(target, propertyKey);
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
  return _nativeGetPrototypeOf(target);
};
Reflect.setPrototypeOf = function(target, proto) {
  return _nativeSetPrototypeOf(target, proto);
};
Reflect.isExtensible = function(target) {
  return _nativeIsExtensible(target);
};
Reflect.preventExtensions = function(target) {
  return _nativePreventExtensions(target);
};
Reflect.ownKeys = function(target) {
  return _nativeOwnKeys(target);
};
Reflect.getOwnPropertyDescriptor = function(target, propertyKey) {
  return _nativeGetOwnPropertyDescriptor(target, propertyKey);
};
