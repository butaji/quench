// VM micro-case 024
// family=arrays; operation=splice-rotate; variant=4; work_units=150; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = Array.from({ length: 96 }, (_, i) => i); for (let i = 0; i < 150; i++) { const moved = values.splice((i * 3) % values.length, 1)[0]; values.splice((i * 5) % (values.length + 1), 0, moved); } return values[(3 * 7) % values.length];
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "54", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
