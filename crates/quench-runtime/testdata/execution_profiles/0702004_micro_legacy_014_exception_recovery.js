// VM micro-case 014
// family=control; operation=exception-recovery; variant=4; work_units=1310; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let recovered = 0; for (let i = 0; i < 1310; i++) { try { if ((i + 3) % 5 === 0) throw new RangeError(String(i)); recovered += i & 3; } catch (error) { recovered += Number(error.message) & 7; } } return recovered;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "2487", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
