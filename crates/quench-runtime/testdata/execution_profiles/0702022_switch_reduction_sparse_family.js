"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let total = -17;
  for (let i = 5; i < 405; i++) {
    switch ((i + 1) % 9) {
      case 0: total = (total << 2) | 3; break;
      case 4: total >>>= 2; break;
      case 7: total += i & 7; break;
      default: total -= 6;
    }
  }
  return total | 0;
}
function verify(result) {
  assert(result === -3361, "sparse switch reduction");
  return result;
}
return { run, verify };
