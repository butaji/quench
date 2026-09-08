// VM micro-case 084
// family=typed-memory; operation=bigint64-typed; variant=4; work_units=180; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = new BigInt64Array(180); let total = 0n; for (let i = 0; i < values.length; i++) { values[i] = BigInt(i + 3); total += values[i] * 3n; } return Number(total);
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "49950", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
