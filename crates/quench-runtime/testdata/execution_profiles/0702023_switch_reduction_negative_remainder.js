"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let total = 19;
  for (let i = 0; i < 200; i++) {
    switch ((3 - i) % 5) {
      case -4: total += 2; break;
      case -1: total ^= 7; break;
      case 0: total -= i & 3; break;
      default: total += 1;
    }
  }
  return total | 0;
}
function verify(result) {
  assert(result === 142, "negative-remainder switch reduction");
  return result;
}
return { run, verify };
