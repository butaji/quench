// Self-hosted Atomics builtins on top of __ops__
//
// Atomics provides static methods for atomic operations on SharedArrayBuffer
// and typed array views. Per the spec (ECMAScript §25.1), Atomics is a
// namespace object whose methods are all static — no constructor, no prototype.
//
// Requires native Rust implementation in the realm:
//   1. SharedArrayBuffer backing store with atomic load/store/op primitives.
//   2. Each Atomics method as a NativeFunction on the Atomics object:
//      - Atomics.add, Atomics.and, Atomics.compareExchange, Atomics.exchange
//      - Atomics.load, Atomics.or, Atomics.store, Atomics.sub, Atomics.xor
//      - Atomics.isLockFree, Atomics.wait, Atomics.notify, Atomics.waitAsync
//
// Once the native methods exist on Atomics, this file wraps each with
// null/undefined checks, following the standard pattern:
//
//   var ops = __ops__;
//   var ThrowTypeError = ops.ThrowTypeError;
//
//   var _nativeAdd = Atomics.add;
//   Atomics.add = function AtomicsAdd(typedArray, index, value) {
//     if (this === null || this === undefined)
//       throw ThrowTypeError("Atomics.add called on null/undefined");
//     return _nativeAdd.apply(this, arguments);
//   };
//
//   (repeat for each method)
//
