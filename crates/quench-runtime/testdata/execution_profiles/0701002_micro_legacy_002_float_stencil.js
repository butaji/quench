// VM micro-case 002
// family=numeric; operation=float-stencil; variant=2; work_units=24800; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const width = 33; const values = new Float64Array(width); for (let i = 0; i < width; i++) values[i] = (i + 2) / (width + 1); for (let pass = 0; pass < 800; pass++) for (let i = 1; i < width - 1; i++) values[i] = (values[i - 1] + values[i] + values[i + 1]) * 0.3333333333333333; return Math.round(values[width >> 1] * 1e9);
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "529411765", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
