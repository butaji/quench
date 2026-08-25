function Box(value) {
  this.value = value;
}
Box.prototype.read = function () {
  return this.value;
};
const object = new Box(1);
let sum = 0;
for (let i = 0; i < 250000; i++) {
  object.value += 1;
  sum += object.value;
}
if (sum !== 31250375000) throw new Error("property result");
