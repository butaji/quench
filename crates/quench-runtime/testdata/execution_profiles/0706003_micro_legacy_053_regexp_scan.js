// VM micro-case 053
// family=strings; operation=regexp-scan; variant=3; work_units=216; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const text = Array.from({ length: 216 }, (_, i) => (i & 1 ? "word" : "item") + i).join(" "); const pattern = /[a-z]+|\d+/gi; let total = 0; let match; while ((match = pattern.exec(text)) !== null) total += match[0].length; return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "1402", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
