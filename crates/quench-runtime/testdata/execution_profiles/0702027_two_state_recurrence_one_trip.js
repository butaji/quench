"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };

function run() {
  let a = 1;
  let b = 2;
  for (let i = 3; i < 4; i++) {
    const next = (a + b) & 255;
    a = b;
    b = next ^ (i & 7);
  }
  return b;
}

function verify(result) {
  assert(result === 0, "one-trip recurrence applies both masks");
  return result;
}

return { run, verify };
