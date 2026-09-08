"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let total = 11;
  for (let i = 5; i < 6; i++) {
    switch ((8 - i) % 4) {
      case 3: total = 7 ^ total; break;
      case 2: total = 4 + total; break;
      case 1: total -= 3 & i; break;
      default: total = (total << 2) | 1;
    }
  }
  return 0 | total;
}
function verify(result) {
  assert(result === 12, "one-iteration operand variations");
  return result;
}
return { run, verify };
