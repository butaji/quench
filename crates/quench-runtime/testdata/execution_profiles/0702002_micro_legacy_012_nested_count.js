// VM micro-case 012
// family=control; operation=nested-count; variant=2; work_units=9360; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let row = 0; row < 40; row++) for (let col = 0; col < 26; col++) for (let lane = 0; lane < 9; lane++) total += (row ^ col ^ lane) & 3; return total;
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "14040", "exact encoded result");
  return __profileResult;
}
return { run: microRun, verify: verify };
