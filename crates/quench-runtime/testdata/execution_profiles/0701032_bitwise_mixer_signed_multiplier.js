"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };

function run() {
  let bits = 123456789;
  for (let i = 4; i < 27; i++) {
    bits = Math.imul(bits ^ (bits >>> 5), -1640531527) | 0;
  }
  return bits;
}

function verify(result) {
  assert(result === -823047495, "signed multiplier wraps exactly");
  return result;
}

return { run, verify };
