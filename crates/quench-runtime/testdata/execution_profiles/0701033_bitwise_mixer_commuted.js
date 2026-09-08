"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };

function run() {
  let bits = -987654321;
  for (let i = 2; i < 19; i++) {
    bits = Math.imul(668265263, (bits >>> 17) ^ bits) | 0;
  }
  return bits;
}

function verify(result) {
  assert(result === -197103355, "commuted mixer operands preserve dataflow");
  return result;
}

return { run, verify };
