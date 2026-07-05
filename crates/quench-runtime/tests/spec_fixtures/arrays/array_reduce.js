// spec: ECMA-262 sec-array.prototype.reduce
// expect: value: 15
// tags: arrays, reduce

const arr = [1, 2, 3, 4, 5];
const sum = arr.reduce((acc, x) => acc + x, 0);
sum;
