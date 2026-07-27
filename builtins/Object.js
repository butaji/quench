// Self-hosted Object builtins on top of __ops__
// Note: uses var (not const/let destructuring) due to a TDZ interaction between
// const destructuring and the binding names matching __ops__ property names.
// Once the const destructuring TDZ issue is fixed, switch to:
//   const { SameValue, ... } = __ops__;

var ops = __ops__;
var SameValue = ops.SameValue;
var ThrowTypeError = ops.ThrowTypeError;
var EnumerableOwnKeys = ops.EnumerableOwnKeys;
var OwnKeys = ops.OwnKeys;
var GetPrototypeOf = ops.GetPrototypeOf;
var IsExtensible = ops.IsExtensible;
var ToObject = ops.ToObject;

// Object.is (ES2025 §20.1.2.12)
Object.is = function ObjectIs(value1, value2) {
  return SameValue(value1, value2);
};

// Object.keys (ES2025 §20.1.2.17)
Object.keys = function ObjectKeys(O) {
  var obj = ToObject(O);
  return EnumerableOwnKeys(obj);
};
