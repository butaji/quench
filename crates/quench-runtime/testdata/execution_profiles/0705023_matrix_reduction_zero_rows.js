"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const left = [];
const right = [[2, 3], [5, 7]];
function run(left, right) {
  let total = -0.5;
  for (let row = 0; row < 0; row++)
    for (let column = 0; column < 2; column++)
      for (let inner = 0; inner < 2; inner++)
        total += left[row][inner] * right[inner][column];
  return total;
}
function verify(result) {
  assert(result === -0.5, "zero-row reduction preserves the initial value");
  return result;
}
return { run, verify, arguments: [left, right] };
