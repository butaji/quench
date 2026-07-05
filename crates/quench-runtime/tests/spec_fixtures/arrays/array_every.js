// spec: ECMA-262 sec-array.prototype.every
// expect: value: true
// tags: arrays, every

const arr = [2, 4, 6, 8, 10];
const allEven = arr.every(x => x % 2 === 0);
allEven;
