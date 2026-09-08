"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const values = new Int32Array(5);
function run(values) {
  let total = 1;
  for (let index = 2; index < 5; index++) {
    values[index] = 3 + (6 ^ index);
    total = values[index] * values[index] + total;
  }
  return total;
}
function verify(result) {
  assert(result === 139, "nonzero lane range uses additive adjustment");
  assert(values[0] === 0 && values[2] === 7 && values[4] === 5, "only selected lanes change");
  return result;
}
return { run, verify, arguments: [values] };
