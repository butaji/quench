function Pair(car, cdr) {
  this.car = car;
  this.cdr = cdr;
}

const tail = new Pair(2, null);
const head = new Pair(1, tail);
let pair = head;
let sum = 0;
for (let i = 0; i < 25000000; i++) {
  sum += pair.car;
  pair = pair.cdr;
  if (pair === null) pair = head;
}
if (sum !== 37500000) throw new Error("pair walk result");
