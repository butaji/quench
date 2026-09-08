"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let value = -2;
  for (let index = 0; index < 3; index++) {
    value = (value * -3 + index + -2) | 0;
  }
  return value;
}
function verify(result) {
  assert(result === 39, "signed recurrence result");
  return result;
}
return { run, verify };
