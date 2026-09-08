"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const left = [[-3]];
const right = [[2.5]];
function run(left, right) {
  let total = 4;
  for (let row = 0; row < 1; row++)
    for (let column = 0; column < 1; column++)
      for (let inner = 0; inner < 1; inner++)
        total = left[row][inner] * right[inner][column] + total;
  return total;
}
function verify(result) {
  assert(result === -3.5, "one-cell reduction accepts commuted accumulator addition");
  return result;
}
return { run, verify, arguments: [left, right] };
