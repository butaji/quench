"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let total = 11;
  for (let i = 3; i < 143; i++) {
    const periodic = i % 7 === 3;
    const masked = (i & 2) === 2;
    if (periodic && masked) total++;
  }
  return total;
}
function verify(result) {
  assert(result === 21, "swapped and reduction result");
  return result;
}
return { run, verify };
