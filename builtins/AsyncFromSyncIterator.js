// Self-hosted AsyncFromSyncIterator builtins on top of __ops__
//
// %AsyncFromSyncIteratorPrototype% wraps a synchronous iterator into an
// async iterator, forwarding each .next() call and awaiting the result.
//
// Requires Rust-side implementation:
//   - %AsyncIteratorPrototype% (no-arg @@asyncIterator method)
//   - %AsyncFromSyncIteratorPrototype% with .next(), .return(), .throw()
//   - Symbol.asyncIterator well-known symbol
//
// Once those exist, this file saves the native constructor/function and
// wraps it with null/undefined checks, following the standard pattern:
//
//   var ops = __ops__;
//   var ThrowTypeError = ops.ThrowTypeError;
//   var _nativeAsyncFromSyncIterator = AsyncFromSyncIterator;
//
//   AsyncFromSyncIterator = function AsyncFromSyncIterator(syncIteratorRecord) {
//     if (this === null || this === undefined)
//       throw ThrowTypeError("AsyncFromSyncIterator called on null/undefined");
//     return _nativeAsyncFromSyncIterator.apply(this, arguments);
//   };
