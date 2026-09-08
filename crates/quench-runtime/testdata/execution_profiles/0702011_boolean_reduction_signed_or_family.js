"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let total = -3;
  for (let i = -20; i < 80; i++) {
    const masked = (i & 7) === 0;
    const periodic = i % 6 === -1;
    if (masked || periodic) total++;
  }
  return total;
}
function verify(result) {
  assert(result === 13, "signed or reduction result");
  return result;
}
return { run, verify };
