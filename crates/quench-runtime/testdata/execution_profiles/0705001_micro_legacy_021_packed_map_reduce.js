// VM micro-case 021
// family=arrays; operation=packed-map-reduce; variant=1; work_units=360; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const input = Array.from({ length: 360 }, (_, i) => (i * 3) & 255); const output = input.map((x) => (x * 7 + 3) & 255).filter((x) => (x & 3) !== 0); return output.reduce((sum, x) => (sum + x) | 0, 0);
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "34688", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
