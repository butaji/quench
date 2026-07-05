// spec: ECMA-262 sec-array.prototype.find
// expect: value: 3
// tags: arrays, find

const arr = [1, 2, 3, 4, 5];
const found = arr.find(x => x > 2);
found;
