"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };

function run() {
  let a = 7;
  let b = -9;
  for (let i = 4; i < 29; i++) {
    const next = 0x7fff & (b + a);
    a = b;
    b = (15 & i) ^ next;
  }
  return b;
}

function verify(result) {
  assert(result === 5946, "commuted recurrence operands preserve dataflow");
  return result;
}

return { run, verify };
