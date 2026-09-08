"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const left = [[1, 2, 3], [4, 5, 6]];
const right = [[7, 8], [9, 10], [11, 12]];
function run(left, right) {
  let total = 0;
  for (let row = 0; row < 2; row++)
    for (let col = 0; col < 2; col++)
      for (let inner = 0; inner < 3; inner++)
        total += left[row][inner] * right[inner][col];
  return total;
}
function verify(result) {
  assert(result === 415, "rectangular matrix product checksum");
  return result;
}
return { run, verify, arguments: [left, right] };
