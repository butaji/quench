var ThrowTypeError = __ops__.ThrowTypeError;

function DisposableStack() {
  if (!(this instanceof DisposableStack)) throw ThrowTypeError("DisposableStack requires new");
  Object.defineProperty(this, "__resources", { value: [], writable: true });
  Object.defineProperty(this, "__disposed", { value: false, writable: true });
}

DisposableStack.prototype.use = function DisposableStackUse(value) {
  if (this.__disposed) throw new ReferenceError("DisposableStack is already disposed");
  var method = value == null ? undefined : value[Symbol.dispose];
  if (typeof method !== 'function') throw new TypeError("Object is not disposable");
  this.__resources.push(function() { method.call(value); });
  return value;
};

DisposableStack.prototype.adopt = function DisposableStackAdopt(value, onDispose) {
  if (this.__disposed) throw new ReferenceError("DisposableStack is already disposed");
  if (typeof onDispose !== 'function') throw new TypeError("onDispose is not callable");
  this.__resources.push(function() { onDispose(value); });
  return value;
};

DisposableStack.prototype.defer = function DisposableStackDefer(onDispose) {
  if (this.__disposed) throw new ReferenceError("DisposableStack is already disposed");
  if (typeof onDispose !== 'function') throw new TypeError("onDispose is not callable");
  this.__resources.push(onDispose);
};

DisposableStack.prototype.dispose = function DisposableStackDispose() {
  if (this.__disposed) return undefined;
  this.__disposed = true;
  var completion;
  while (this.__resources.length) {
    try { this.__resources.pop()(); }
    catch (error) { completion = completion === undefined ? error : new SuppressedError(error, completion); }
  }
  if (completion !== undefined) throw completion;
};

DisposableStack.prototype.move = function DisposableStackMove() {
  if (this.__disposed) throw new ReferenceError("DisposableStack is already disposed");
  var moved = new DisposableStack();
  moved.__resources = this.__resources;
  this.__resources = [];
  this.__disposed = true;
  return moved;
};

Object.defineProperty(DisposableStack.prototype, "disposed", {
  get: function() { return this.__disposed; }, enumerable: false, configurable: true
});
Object.defineProperty(DisposableStack.prototype, Symbol.dispose, {
  value: DisposableStack.prototype.dispose, writable: true, configurable: true
});
