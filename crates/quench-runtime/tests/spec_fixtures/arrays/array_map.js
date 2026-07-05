// spec: ECMA-262 sec-array.prototype.map
// expect: value: [2, 4, 6]
// tags: arrays, map

const arr = [1, 2, 3];
const mapped = arr.map(x => x * 2);
mapped;
