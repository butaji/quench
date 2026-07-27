// Placeholder: AsyncDisposableStack constructor and prototype.
// Requires native Rust implementation of the disposable resource stack
// and Symbol.asyncDispose well-known symbol before the real self-hosted
// methods can be wired via __ops__.

var AsyncDisposableStack = function AsyncDisposableStack() {};
AsyncDisposableStack.prototype = Object.create(Object.prototype);
AsyncDisposableStack.prototype.constructor = AsyncDisposableStack;
AsyncDisposableStack.prototype.disposeAsync = function() {
  throw new Error("AsyncDisposableStack.prototype[Symbol.asyncDispose] not yet implemented");
};
