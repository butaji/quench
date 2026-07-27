// Self-hosted %AsyncIteratorPrototype% on top of __ops__
//
// NOTE: %AsyncIteratorPrototype% requires native Rust implementation in the
// realm before the JS self-hosted layer can fully wire it. The native realm
// must:
//   1. Create %AsyncIteratorPrototype% and %AsyncFromSyncIteratorPrototype%
//      as intrinsic objects on the realm.
//   2. Wire async generator prototypes to inherit from
//      %AsyncIteratorPrototype%.
//   3. Wire %AsyncFromSyncIteratorPrototype% for the CreateAsyncFromSyncIterator
//      abstract operation.
//
// Once the native realm creates the prototype, this file will wrap its
// [Symbol.asyncIterator]() method. The method returns `this` per
// ES2025 §27.1.3.1.

var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;
