"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let total = -23;
  for (let i = 5; i < 5; i++) {
    switch ((9 + i) % 3) {
      case 0: total = 4 + total; break;
      case 1: total ^= 7; break;
      default: total += i & 3;
    }
  }
  return total | 0;
}
function verify(result) {
  assert(result === -23, "zero-iteration switch reduction");
  return result;
}
return { run, verify };
