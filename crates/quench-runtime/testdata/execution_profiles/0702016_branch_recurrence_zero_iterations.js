"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let score = -99;
  let state = 4000000000;
  for (let i = 7; i < 7; i++) {
    state = (state * 3 + 5) >>> 0;
    if ((state & 7) === 1) score += i;
    else score -= i & 3;
  }
  return score;
}
function verify(result) {
  assert(result === -99, "zero-iteration recurrence preserves initial state");
  return result;
}
return { run, verify };
