"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };

function run() {
  let a = 10;
  let b = 11;
  for (let i = 0; i < 32; i++) {
    const next = (a + b) & 0xffff;
    a = b;
    b = next ^ (i & 31);
  }
  return b;
}

function verify(result) {
  assert(result === 21453, "masked two-state recurrence result");
  return result;
}

return { run, verify };
