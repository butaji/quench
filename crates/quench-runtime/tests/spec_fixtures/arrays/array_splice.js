// spec: ECMA-262 sec-array.prototype.splice
// expect: value: [2, 3]
// tags: arrays, splice

const arr = [1, 2, 3, 4, 5];
const removed = arr.splice(1, 2);
removed;
