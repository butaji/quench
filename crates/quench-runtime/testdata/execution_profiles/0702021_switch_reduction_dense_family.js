"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let total = 5;
  for (let i = 3; i < 303; i++) {
    switch ((i + 5) % 4) {
      case 0: total += 11; break;
      case 1: total ^= 9; break;
      case 2: total -= 4; break;
      default: total += i & 3;
    }
  }
  return total | 0;
}
function verify(result) {
  assert(result === 779, "dense switch reduction");
  return result;
}
return { run, verify };
