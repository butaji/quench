"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let value = -9;
  for (let index = 5; index < 2; index++) {
    value = (value * 13 + index + 4) | 0;
  }
  return value;
}
function verify(result) {
  assert(result === -9, "empty recurrence preserves its initial value");
  return result;
}
return { run, verify };
