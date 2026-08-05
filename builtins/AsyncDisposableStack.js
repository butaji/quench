var ThrowTypeError = __ops__.ThrowTypeError;
var DefineProp = __ops__.DefineProp;

function AsyncDisposableStack() {
  if (!(this instanceof AsyncDisposableStack)) throw ThrowTypeError("AsyncDisposableStack requires new");
  Object.defineProperty(this, "__resources", { value: [], writable: true });
  Object.defineProperty(this, "__disposed", { value: false, writable: true });
}

AsyncDisposableStack.prototype.use = function AsyncDisposableStackUse(value) {
  if (this.__disposed) throw new ReferenceError("AsyncDisposableStack is already disposed");
  if (value == null) return value;
  var method = value[Symbol.asyncDispose] || value[Symbol.dispose];
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

DefineProp(AsyncDisposableStack.prototype, "adopt", {
  value: AsyncDisposableStack.prototype.adopt,
  writable: true,
  enumerable: false,
  configurable: true
});
DefineProp(AsyncDisposableStack.prototype, "defer", {
  value: AsyncDisposableStack.prototype.defer,
  writable: true,
  enumerable: false,
  configurable: true
});
DefineProp(AsyncDisposableStack.prototype, "disposeAsync", {
  value: AsyncDisposableStack.prototype.disposeAsync,
  writable: true,
  enumerable: false,
  configurable: true
});
DefineProp(AsyncDisposableStack.prototype, "use", {
  value: AsyncDisposableStack.prototype.use,
  writable: true,
  enumerable: false,
  configurable: true
});
DefineProp(AsyncDisposableStack.prototype, "move", {
  value: AsyncDisposableStack.prototype.move,
  writable: true,
  enumerable: false,
  configurable: true
});
DefineProp(AsyncDisposableStack.prototype, Symbol.toStringTag, {
  value: "AsyncDisposableStack",
  writable: false,
  enumerable: false,
  configurable: true
});
DefineProp(AsyncDisposableStack.prototype, Symbol.asyncDispose, {
  value: AsyncDisposableStack.prototype.disposeAsync,
  writable: true,
  enumerable: false,
  configurable: true
});
DefineProp(AsyncDisposableStack.prototype, "disposed", {
  get: function() { return this.__disposed; },
  enumerable: false,
  configurable: true
});
DefineProp(AsyncDisposableStack, "prototype", {
  value: AsyncDisposableStack.prototype,
  writable: false,
  enumerable: false,
  configurable: false
});
DefineProp(AsyncDisposableStack, "length", {
  value: 0,
  writable: false,
  enumerable: false,
  configurable: true
});
DefineProp(AsyncDisposableStack, "name", {
  value: "AsyncDisposableStack",
  writable: false,
  enumerable: false,
  configurable: true
});
