"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let value = -7;
  for (let index = 3; index < 10; index++) {
    value = (index + value * -5 + 9) | 0;
  }
  return value >>> 0;
}
function verify(result) {
  assert(result === 700958, "nonzero counted recurrence result");
  return result;
}
return { run, verify };
