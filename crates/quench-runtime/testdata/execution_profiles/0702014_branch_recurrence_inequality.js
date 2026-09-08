"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let score = -4;
  let state = 123;
  for (let i = 11; i < 311; i++) {
    state = (1 + 22695477 * state) >>> 0;
    if (5 !== (15 & state)) score -= i & 3;
    else score += i;
  }
  return score;
}
function verify(result) {
  assert(result === -454, "inequality recurrence result");
  return result;
}
return { run, verify };
