// VM micro-case 063
// family=iterables; operation=destructure-rest; variant=3; work_units=260; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let pass = 0; pass < 260; pass++) { const source = Array.from({ length: 19 }, (_, i) => i + 2 + pass); const [first, second, ...rest] = source; const [, fourth, ...tail] = rest; total += first + second + fourth + tail.length; } return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "107510", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
