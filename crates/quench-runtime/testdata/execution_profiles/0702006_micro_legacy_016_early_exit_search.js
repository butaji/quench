// VM micro-case 016
// family=control; operation=early-exit-search; variant=6; work_units=9900; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let found = -1; for (let i = 0; i < 9900; i++) { if (((i * 17 + 5) % 971) === 8) { found = i; break; } } return found;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "400", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
