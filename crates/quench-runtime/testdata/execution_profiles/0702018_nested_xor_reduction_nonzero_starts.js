"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let total = 17;
  for (let a = 2; a < 7; a++)
    for (let b = 4; b < 10; b++)
      for (let c = 1; c < 5; c++) total += (a ^ b ^ c) & 7;
  return total;
}
function verify(result) {
  assert(result === 429, "nonzero-start nested xor reduction");
  return result;
}
return { run, verify };
