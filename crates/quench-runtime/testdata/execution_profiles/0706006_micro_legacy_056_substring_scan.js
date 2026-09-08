// VM micro-case 056
// family=strings; operation=substring-scan; variant=6; work_units=960; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const text = ("segment-5-").repeat(288); let total = 0; for (let i = 0; i < text.length; i += 3) if (text.slice(i, i + 4).includes("e")) total++; return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "672", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
