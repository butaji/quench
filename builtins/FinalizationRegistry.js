// Placeholder: FinalizationRegistry
//
// Full implementation requires a native Rust FinalizationRegistry backing
// (gc-observation hook + cleanup callback queue + job integration).
// Until then, this stub satisfies the global name so that test262 harness
// dependencies that reference `FinalizationRegistry` do not throw ReferenceError.
//
// See: ECMA-262 §27.2 — FinalizationRegistry Objects

var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;
var DefineProp = ops.DefineProp;

// Stub constructor — no-op until native backing lands.
var FinalizationRegistry = function FinalizationRegistry(cleanupCallback) {};
FinalizationRegistry.prototype.constructor = FinalizationRegistry;

DefineProp(FinalizationRegistry.prototype, "cleanupSome", {
  value: function (callback) {
    if (this === null || this === undefined) throw ThrowTypeError("FinalizationRegistry.prototype.cleanupSome called on null or undefined");
  },
  writable: true,
  enumerable: false,
  configurable: true
});
DefineProp(FinalizationRegistry.prototype, "register", {
  value: function (target, heldValue, unregisterToken = undefined) {
    if (this === null || this === undefined) throw ThrowTypeError("FinalizationRegistry.prototype.register called on null or undefined");
  },
  writable: true,
  enumerable: false,
  configurable: true
});
DefineProp(FinalizationRegistry.prototype, "unregister", {
  value: function (unregisterToken = undefined) {
    if (this === null || this === undefined) throw ThrowTypeError("FinalizationRegistry.prototype.unregister called on null or undefined");
    return false;
  },
  writable: true,
  enumerable: false,
  configurable: true
});
