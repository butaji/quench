// Self-hosted Reflect methods on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;
var ToObject = ops.ToObject;
var HasProperty = ops.HasProperty;
var OwnKeys = ops.OwnKeys;
var GetOwnPropDesc = ops.GetOwnPropDesc;
var PreventExtensions = ops.PreventExtensions;
var IsExtensible = ops.IsExtensible;

Reflect.has = function(target, propertyKey) {
  return HasProperty(ToObject(target), propertyKey);
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
  if (target === null || (typeof target !== 'object' && typeof target !== 'function')) {
    throw ThrowTypeError("Reflect.ownKeys target must be an object");
  }
  return OwnKeys(target);
};
Reflect.getOwnPropertyDescriptor = function(target, propertyKey) {
  return GetOwnPropDesc(ToObject(target), propertyKey);
};
