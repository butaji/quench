// VM micro-case 048
// family=functions; operation=bound-method-cache; variant=8; work_units=1640; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const receiver = { bias: 7, add(value) { return this.bias + value; } }; const bound = receiver.add.bind(receiver); let total = 0; for (let i = 0; i < 1640; i++) total += bound(i); return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "1355460", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
