"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const left = [
  [9, 10, 11, 12],
  [10, 11, 12, 13],
  [11, 12, 13, 14],
  [12, 13, 14, 15]
];
const right = [
  [1, 0, 0, 0],
  [0, 1, 0, 0],
  [0, 0, 1, 0],
  [0, 0, 0, 1]
];
function run(left, right) {
  let total = 0;
  for (let row = 0; row < 4; row++) {
    for (let col = 0; col < 4; col++) {
      for (let inner = 0; inner < 4; inner++) {
        total += left[row][inner] * right[inner][col];
      }
    }
  }
  return total;
}
function verify(result) {
  assert(result === 192, "square matrix product checksum");
  return result;
}
return { run, verify, arguments: [left, right] };
