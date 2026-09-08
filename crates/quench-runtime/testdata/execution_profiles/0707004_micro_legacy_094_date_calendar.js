// VM micro-case 094
// family=meta; operation=date-calendar; variant=4; work_units=162; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 162; i++) { const date = new Date(Date.UTC(2000 + ((i + 3) % 20), (i + 3) % 12, 1 + (i % 27))); total += date.getUTCFullYear() + date.getUTCMonth() + date.getUTCDate(); } return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "328686", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
