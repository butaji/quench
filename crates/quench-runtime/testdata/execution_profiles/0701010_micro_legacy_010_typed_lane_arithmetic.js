"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const values = new Int32Array(8);
function run(values) {
  let total = 0;
  for (let index = 0; index < 8; index++) {
    values[index] = (index ^ 279) - 9;
    total += values[index] * values[index];
  }
  return total;
}
function verify(result) {
  assert(result === 568220, "typed lanes use exact int32 writes and ordered Number sum");
  assert(values[0] === 270 && values[7] === 263, "typed lane stores remain observable");
  return result;
}
return { run, verify, arguments: [values] };
