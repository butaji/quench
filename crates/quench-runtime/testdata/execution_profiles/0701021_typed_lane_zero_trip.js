"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const values = new Int32Array(0);
function run(values) {
  let total = 3.5;
  for (let index = 0; index < 0; index++) {
    values[index] = (index ^ 17) - 4;
    total += values[index] * values[index];
  }
  return total;
}
function verify(result) {
  assert(result === 3.5 && values.length === 0, "zero-trip lane loop preserves state");
  return result;
}
return { run, verify, arguments: [values] };
