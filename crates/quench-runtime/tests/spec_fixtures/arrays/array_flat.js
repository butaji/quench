// spec: ECMA-262 sec-array.prototype.flat
// expect: value: [1, 2, 3, 4]
// tags: arrays, flat

const arr = [1, [2, [3]], 4];
const flattened = arr.flat(2);
flattened;
