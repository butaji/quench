"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };

function run() {
  let a = -5;
  let b = 12;
  for (let i = 7; i < 7; i++) {
    const next = (a + b) & 255;
    a = b;
    b = next ^ (i & 7);
  }
  return b;
}

function verify(result) {
  assert(result === 12, "zero-trip recurrence preserves the second state");
  return result;
}

return { run, verify };
