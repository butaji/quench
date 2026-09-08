"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let score = 10;
  let state = 4294967295;
  for (let i = 5; i < 6; i++) {
    state = (state * 1 + 1) >>> 0;
    if ((state & 255) === 0) score += i & 3;
    else score -= i;
  }
  return score;
}
function verify(result) {
  assert(result === 11, "one-iteration recurrence preserves ToUint32 wrap");
  return result;
}
return { run, verify };
