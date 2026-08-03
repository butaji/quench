// Self-hosted Iterator helpers on top of __ops__
var ops = __ops__;
var ThrowTypeError = ops.ThrowTypeError;
var IsCallable = ops.IsCallable;

// Save native implementations
var _nativeFrom = Iterator.__from;
var _nativeProtoMap = Iterator.prototype.__map;
var _nativeProtoFilter = Iterator.prototype.__filter;
var _nativeProtoTake = Iterator.prototype.__take;
var _nativeProtoDrop = Iterator.prototype.__drop;
var _nativeProtoFlatMap = Iterator.prototype.__flatMap;

// Iterator.from (ES2025 §27.1.3.2)
Iterator.from = function IteratorFrom(O) {
  return _nativeFrom(O);
};

// Iterator.prototype.map (ES2025 §27.1.4.4)
Iterator.prototype.map = function IteratorMap(mapper) {
  if (!IsCallable(mapper)) throw ThrowTypeError("mapper is not a function");
  return _nativeProtoMap.call(this, mapper);
};

// Iterator.prototype.filter (ES2025 §27.1.4.3)
Iterator.prototype.filter = function IteratorFilter(filterer) {
  if (!IsCallable(filterer)) throw ThrowTypeError("filterer is not a function");
  return _nativeProtoFilter.call(this, filterer);
};

Iterator.prototype.take = function(limit) { return _nativeProtoTake.call(this, limit); };
Iterator.prototype.drop = function(limit) { return _nativeProtoDrop.call(this, limit); };
Iterator.prototype.flatMap = function(mapper) {
  if (!IsCallable(mapper)) throw ThrowTypeError("mapper is not a function");
  return _nativeProtoFlatMap.call(this, mapper);
};
Iterator.prototype.reduce = function IteratorReduce(reducer, initialValue) {
  if (!IsCallable(reducer)) throw ThrowTypeError("reducer is not a function");
  var iterator = this;
  var step = iterator.next();
  var accumulator;
  if (arguments.length > 1) accumulator = initialValue;
  else {
    if (step.done) throw ThrowTypeError("Reduce of empty iterator with no initial value");
    accumulator = step.value;
    step = iterator.next();
  }
  while (!step.done) {
    accumulator = reducer(accumulator, step.value);
    step = iterator.next();
  }
  return accumulator;
};
Iterator.prototype.toArray = function IteratorToArray() {
  var result = [];
  var step = this.next();
  while (!step.done) {
    result.push(step.value);
    step = this.next();
  }
  return result;
};
Iterator.prototype.forEach = function IteratorForEach(fn) {
  if (!IsCallable(fn)) throw ThrowTypeError("fn is not a function");
  var step = this.next();
  while (!step.done) {
    fn(step.value);
    step = this.next();
  }
};
Iterator.prototype.some = function IteratorSome(fn) {
  if (!IsCallable(fn)) throw ThrowTypeError("fn is not a function");
  var index = 0;
  var step = this.next();
  while (!step.done) {
    if (fn(step.value, index++)) return true;
    step = this.next();
  }
  return false;
};
Iterator.prototype.every = function IteratorEvery(fn) {
  if (!IsCallable(fn)) throw ThrowTypeError("fn is not a function");
  var index = 0;
  var step = this.next();
  while (!step.done) {
    if (!fn(step.value, index++)) return false;
    step = this.next();
  }
  return true;
};
Iterator.prototype.find = function IteratorFind(fn) {
  if (!IsCallable(fn)) throw ThrowTypeError("fn is not a function");
  var index = 0;
  var step = this.next();
  while (!step.done) {
    if (fn(step.value, index++)) return step.value;
    step = this.next();
  }
  return undefined;
};
