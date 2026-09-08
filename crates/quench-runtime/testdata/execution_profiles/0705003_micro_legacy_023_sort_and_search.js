// VM micro-case 023
// family=arrays; operation=sort-and-search; variant=3; work_units=2765; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = Array.from({ length: 320 }, (_, i) => (i * 37 + 22) % 997); values.sort((a, b) => a - b); let found = 0; for (let i = 0; i < values.length; i++) if (values[i] >= 26) found += values[i]; return found;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "158684", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
