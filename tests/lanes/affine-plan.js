var Mode = { FORWARD: 1, BACKWARD: -1 };

function Cell(value) {
  this.value = value;
}

function Transform(left, right, factor, bias) {
  this.left = left;
  this.right = right;
  this.factor = factor;
  this.bias = bias;
  this.mode = Mode.FORWARD;
}

Transform.prototype.apply = function () {
  if (this.mode == Mode.FORWARD) {
    this.right.value = this.left.value * this.factor.value + this.bias.value;
  } else {
    this.left.value = (this.right.value - this.bias.value) / this.factor.value;
  }
};

function Collection(value) {
  this.values = [value];
}

Collection.prototype.size = function () {
  return this.values.length;
};

Collection.prototype.at = function (index) {
  return this.values[index];
};

Collection.prototype.run = function () {
  for (var index = 0; index < this.size(); index++) {
    var value = this.at(index);
    value.apply();
  }
};

var left = new Cell(3);
var right = new Cell(0);
var transform = new Transform(left, right, new Cell(4), new Cell(2));
var collection = new Collection(transform);
for (var iteration = 0; iteration < 20000; iteration++) collection.run();
if (right.value !== 14) throw new Error("affine plan");
