"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };

function run() {
  let bits = 123456789;
  for (let i = 9; i < 9; i++) {
    bits = Math.imul(bits ^ (bits >>> 7), 31) | 0;
  }
  return bits;
}

function verify(result) {
  assert(result === 123456789, "zero-trip mixer preserves state");
  return result;
}

return { run, verify };
