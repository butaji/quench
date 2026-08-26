const Direction = { FORWARD: 1 };
const Rank = {
  min(left, right) {
    return left.value > right.value ? left : right;
  },
};

function Link(direction, left, right, rank) {
  this.direction = direction;
  this.left = left;
  this.right = right;
  this.rank = rank;
}
Link.prototype.input = function () {
  return this.direction == Direction.FORWARD ? this.left : this.right;
};
Link.prototype.output = function () {
  return this.direction == Direction.FORWARD ? this.right : this.left;
};
Link.prototype.execute = function () {
  this.output().value = this.input().value;
};
Link.prototype.recalculate = function () {
  var input = this.input(), output = this.output();
  output.rank = Rank.min(this.rank, input.rank);
  output.live = input.live;
  if (output.live) this.execute();
};

const strong = { value: 1 };
const weak = { value: 2 };
const left = { value: 7, rank: weak, live: true };
const right = { value: 0, rank: strong, live: false };
const link = new Link(Direction.FORWARD, left, right, strong);
for (let i = 0; i < 200000; i++) link.recalculate();
if (right.value !== 7 || right.rank !== weak || !right.live) {
  throw new Error("recalculate kernel lost derived state");
}
