// spec: ECMA-262 sec-array.prototype.filter
// expect: value: [2, 4]
// tags: arrays, filter

const arr = [1, 2, 3, 4, 5];
const filtered = arr.filter(x => x % 2 === 0);
filtered;
