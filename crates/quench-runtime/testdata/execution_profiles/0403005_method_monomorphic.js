function Counter(value) {
  this.value = value;
}
Counter.prototype.increment = function increment(delta) {
  this.value = this.value + delta;
  return this.value;
};

var counter = new Counter(40);
counter.increment(1);
var result = counter.increment(1);
if (result !== 42) {
  throw new Error("monomorphic method mismatch");
}
return result;
