// VM micro-case 086
// family=typed-memory; operation=int16-saturation; variant=6; work_units=930; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = new Int16Array(930); for (let i = 0; i < values.length; i++) values[i] = (i * 22) - 20000; let total = 0; for (const value of values) total += Math.max(-1000, Math.min(1000, value)); return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "-882282", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
