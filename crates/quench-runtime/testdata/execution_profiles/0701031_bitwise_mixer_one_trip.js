"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };

function run() {
  let bits = -1;
  for (let i = 5; i < 6; i++) {
    bits = Math.imul(bits ^ (bits >>> 31), 3) | 0;
  }
  return bits;
}

function verify(result) {
  assert(result === -6, "one-trip mixer keeps signed i32 result");
  return result;
}

return { run, verify };
