function Counter(value) {
  this.value = value;
}

Counter.prototype.add = function (left, right) {
  return this.value + left + right;
};

const counter = new Counter(1);
let sum = 0;
for (let i = 0; i < 250000; i++) sum += counter.add(2, 3);
if (sum !== 1500000) throw new Error("method result");
