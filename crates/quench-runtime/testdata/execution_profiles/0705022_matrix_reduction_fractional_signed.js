"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const left = [[-1.5, 2], [3.25, -4], [0.5, 8]];
const right = [[2], [-0.5]];
function run(left, right) {
  let total = 1.25;
  for (let row = 0; row < 3; row++) {
    for (let col = 0; col < 1; col++) {
      for (let inner = 0; inner < 2; inner++) {
        total = total + left[row][inner] * right[inner][col];
      }
    }
  }
  return total;
}
function verify(result) {
  assert(result === 2.75, "signed fractional matrix product checksum");
  return result;
}
return { run, verify, arguments: [left, right] };
