"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const values = new Int32Array(4);
function run(values) {
  let total = 0;
  for (let index = 0; index < 4; index++) {
    values[index] = (index ^ 3) - 8;
    total += values[index] * values[index];
  }
  return total;
}
function verify(result) {
  assert(result === 174, "signed lanes square in Number order");
  assert(values[0] === -5 && values[3] === -8, "signed stores use ToInt32 values");
  return result;
}
return { run, verify, arguments: [values] };
