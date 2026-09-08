"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };

function run() {
  let a = -2147483648;
  let b = 2147483647;
  for (let i = 5; i < 23; i++) {
    const next = (a + b) & -1;
    a = b;
    b = next ^ (i & 255);
  }
  return b;
}

function verify(result) {
  assert(result === 2147474425, "signed recurrence preserves ToInt32 behavior");
  return result;
}

return { run, verify };
