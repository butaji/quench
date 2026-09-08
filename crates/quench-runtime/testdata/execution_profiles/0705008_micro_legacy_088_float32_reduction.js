// VM micro-case 088
// family=typed-memory; operation=float32-reduction; variant=8; work_units=1110; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = new Float32Array(1110); for (let i = 0; i < values.length; i++) values[i] = Math.sin(i * 0.03 + 7); let total = 0; for (let i = 0; i < values.length; i++) total += values[i] * values[i]; return Math.round(total * 1e6);
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "570688890", "exact encoded result");
return __profileResult;
