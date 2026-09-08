// VM micro-case 003
// family=numeric; operation=bitwise-mixer; variant=3; work_units=5680; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let bits = -1640531525; for (let i = 0; i < 5680; i++) bits = Math.imul(bits ^ (bits >>> 13), 0x85ebca6b) | 0; return bits >>> 0;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "3388529760", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
