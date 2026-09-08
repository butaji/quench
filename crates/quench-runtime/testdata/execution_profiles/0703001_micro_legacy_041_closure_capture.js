// VM micro-case 041
// family=functions; operation=closure-capture; variant=1; work_units=1600; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function make(seed) { let value = seed; return () => value = (value * 31 + 7) | 0; } const next = make(1); let total = 0; for (let i = 0; i < 1600; i++) total ^= next(); return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "246567936", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
