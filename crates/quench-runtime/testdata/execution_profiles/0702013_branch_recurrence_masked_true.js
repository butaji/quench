"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let score = 9;
  let state = 17;
  for (let i = 3; i < 257; i++) {
    state = (1664525 * state + 1013904223) >>> 0;
    if (4 === (state & 7)) score = (i & 15) + score;
    else score -= i;
  }
  return score;
}
function verify(result) {
  assert(result === -28596, "masked true recurrence result");
  return result;
}
return { run, verify };
