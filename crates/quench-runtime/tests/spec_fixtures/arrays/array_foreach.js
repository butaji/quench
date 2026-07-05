// spec: ECMA-262 sec-array.prototype.forEach
// expect: value: "1-2-3"
// tags: arrays, forEach

const arr = [1, 2, 3];
let result = "";
arr.forEach(x => { result += x + "-"; });
result;
