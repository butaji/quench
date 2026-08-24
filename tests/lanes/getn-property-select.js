const Direction = { FORWARD: 1 };
const selector = {
  direction: Direction.FORWARD,
  first: { value: 3 },
  second: { value: 5 },
  input() {
    return this.direction == Direction.FORWARD ? this.first : this.second;
  },
};

let total = 0;
for (let i = 0; i < 250000; i++) total += selector.input().value;
if (total !== 750000) throw new Error("property select result");
