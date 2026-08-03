var ThrowTypeError = __ops__.ThrowTypeError;

function AsyncDisposableStack() {
  if (!(this instanceof AsyncDisposableStack)) throw ThrowTypeError("AsyncDisposableStack requires new");
  Object.defineProperty(this, "__resources", { value: [], writable: true });
  Object.defineProperty(this, "__disposed", { value: false, writable: true });
}

AsyncDisposableStack.prototype.use = function AsyncDisposableStackUse(value) {
  if (this.__disposed) throw new ReferenceError("AsyncDisposableStack is already disposed");
  var method = value == null ? undefined : (value[Symbol.asyncDispose] || value[Symbol.dispose]);
  if (typeof method !== 'function') throw new TypeError("Object is not disposable");
  this.__resources.push(function() { return method.call(value); });
  return value;
};

AsyncDisposableStack.prototype.adopt = function AsyncDisposableStackAdopt(value, onDispose) {
  if (this.__disposed) throw new ReferenceError("AsyncDisposableStack is already disposed");
  if (typeof onDispose !== 'function') throw new TypeError("onDispose is not callable");
  this.__resources.push(function() { return onDispose(value); });
  return value;
};

AsyncDisposableStack.prototype.defer = function AsyncDisposableStackDefer(onDispose) {
  if (this.__disposed) throw new ReferenceError("AsyncDisposableStack is already disposed");
  if (typeof onDispose !== 'function') throw new TypeError("onDispose is not callable");
  this.__resources.push(onDispose);
};

AsyncDisposableStack.prototype.disposeAsync = function AsyncDisposableStackDisposeAsync() {
  if (this.__disposed) return Promise.resolve(undefined);
  this.__disposed = true;
  var resources = this.__resources;
  var completion = Promise.resolve(undefined);
  while (resources.length) {
    (function(resource) {
      completion = completion.then(function() { return resource(); });
    })(resources.pop());
  }
  return completion;
};

AsyncDisposableStack.prototype.move = function AsyncDisposableStackMove() {
  if (this.__disposed) throw new ReferenceError("AsyncDisposableStack is already disposed");
  var moved = new AsyncDisposableStack();
  moved.__resources = this.__resources;
  this.__resources = [];
  this.__disposed = true;
  return moved;
};

Object.defineProperty(AsyncDisposableStack.prototype, "disposed", {
  get: function() { return this.__disposed; }, enumerable: false, configurable: true
});
Object.defineProperty(AsyncDisposableStack.prototype, Symbol.asyncDispose, {
  value: AsyncDisposableStack.prototype.disposeAsync, writable: true, configurable: true
});
