"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let total = 123;
  for (let a = 4; a < 9; a++)
    for (let b = 2; b < 7; b++)
      for (let c = 6; c < 6; c++) total += (a ^ b ^ c) & 3;
  return total;
}
function verify(result) {
  assert(result === 123, "empty nested dimension preserves accumulator");
  return result;
}
return { run, verify };
