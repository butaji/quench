// spec: ECMA-262 sec-array.prototype.some
// expect: value: true
// tags: arrays, some

const arr = [1, 2, 3, 4, 5];
const hasEven = arr.some(x => x % 2 === 0);
hasEven;
