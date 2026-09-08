"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const values = new Int32Array(1);
function run(values) {
  let total = 0;
  for (let index = 0; index < 1; index++) {
    values[index] = (index ^ 7) - 2;
    total += values[index] * values[index];
  }
  return total;
}
function verify(result) {
  assert(result === 25 && values[0] === 5, "one typed lane commits once");
  return result;
}
return { run, verify, arguments: [values] };
