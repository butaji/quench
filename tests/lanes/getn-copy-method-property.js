const Direction = { FORWARD: 1 };
const constraint = {
  direction: Direction.FORWARD,
  first: { value: 3 },
  second: { value: 5 },
  input() {
    return this.direction == Direction.FORWARD ? this.first : this.second;
  },
  output() {
    return this.direction == Direction.FORWARD ? this.second : this.first;
  },
  execute() {
    this.output().value = this.input().value;
  },
};

for (let i = 0; i < 250000; i++) constraint.execute();
if (constraint.second.value !== 3) throw new Error("copy method property result");
