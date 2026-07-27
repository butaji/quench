// Self-hosted DisposableStack builtins on top of __ops__
//
// %DisposableStackPrototype% provides explicit resource management
// per ES2025 §27.5.3. Methods on the prototype:
//   - dispose()      — disposes all resources on the stack (reverse order)
//   - use(value)     — adds a synchronous disposable resource to the stack
//   - adopt(value, onDispose) — adds a value with a custom disposal function
//   - defer(onDispose) — adds a disposal function alone
//   - move()         — moves resources to a new DisposableStack
//   - disposed (getter) — returns whether the stack has been disposed
//
// Requires Rust-side implementation:
//   - DisposableStack constructor function
//   - %DisposableStackPrototype% with internal [[DisposableState]] slot
//   - Native dispose/use/adopt/defer/move methods
//   - SuppressedError error type
//   - Symbol.dispose well-known symbol
//
// Once those exist, this file saves the native methods and wraps them
// with null/undefined checks, following the standard pattern:
//
//   var ops = __ops__;
//   var ThrowTypeError = ops.ThrowTypeError;
//   var _nativeDispose = DisposableStack.prototype.dispose;
//
//   DisposableStack.prototype.dispose = function DisposableStackDispose() {
//     if (this === null || this === undefined)
//       throw ThrowTypeError("DisposableStack.prototype.dispose called on null or undefined");
//     return _nativeDispose.apply(this, arguments);
//   };
