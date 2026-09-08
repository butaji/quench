"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let score = 100;
  let state = 9;
  for (let i = 0; i < 512; i++) {
    state = (state * 214013 + 2531011) >>> 0;
    if ((state & 31) === 16) score += i;
    else score -= i & 1;
  }
  return score;
}
function verify(result) {
  assert(result === 3812, "wide-mask recurrence result");
  return result;
}
return { run, verify };
