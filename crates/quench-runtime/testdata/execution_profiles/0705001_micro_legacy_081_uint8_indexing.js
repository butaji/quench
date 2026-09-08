// VM micro-case 081
// family=typed-memory; operation=uint8-indexing; variant=1; work_units=480; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const bytes = new Uint8Array(480); let total = 0; for (let i = 0; i < bytes.length; i++) { bytes[i] = (i * 17) & 255; total += bytes[i]; } return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "61200", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
