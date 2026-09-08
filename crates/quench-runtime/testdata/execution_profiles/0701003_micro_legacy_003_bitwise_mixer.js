"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };

function run() {
  let bits = -1640531525;
  for (let i = 0; i < 32; i++) {
    bits = Math.imul(bits ^ (bits >>> 13), 0x85ebca6b) | 0;
  }
  return bits >>> 0;
}

function verify(result) {
  assert(result === 3707827554, "unsigned mixer result");
  return result;
}

return { run, verify };
