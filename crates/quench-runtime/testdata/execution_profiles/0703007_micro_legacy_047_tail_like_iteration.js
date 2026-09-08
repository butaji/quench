// VM micro-case 047
// family=functions; operation=tail-like-iteration; variant=7; work_units=760; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function iterate(value, steps) { while (steps-- > 0) value = (value * 3 + steps + 6) | 0; return value; } let total = 0; for (let i = 0; i < 760; i++) total ^= iterate(i, 16 + (i & 7)); return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "155958264", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
