// VM micro-case 089
// family=typed-memory; operation=shared-view-alias; variant=9; work_units=1200; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const buffer = new ArrayBuffer(4800); const words = new Uint32Array(buffer); const bytes = new Uint8Array(buffer); let total = 0; for (let i = 0; i < words.length; i++) { words[i] = i + 8; total += bytes[(i * 3) % bytes.length]; } return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "36227", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
