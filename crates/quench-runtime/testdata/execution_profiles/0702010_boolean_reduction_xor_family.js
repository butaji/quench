"use strict";
const assert = (condition, message) => { if (!condition) throw new Error(message); };
function run() {
  let total = 7;
  for (let i = 5; i < 105; i++) {
    const masked = (i & 3) === 1;
    const periodic = i % 5 === 4;
    if ((masked && !periodic) || (!masked && periodic)) total++;
  }
  return total;
}
function verify(result) {
  assert(result === 42, "xor reduction result");
  return result;
}
return { run, verify };
