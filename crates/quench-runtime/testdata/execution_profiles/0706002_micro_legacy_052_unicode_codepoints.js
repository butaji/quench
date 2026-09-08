// VM micro-case 052
// family=strings; operation=unicode-codepoints; variant=2; work_units=288; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const text = ("micro-1-😀-🙂").repeat(24); let total = 0; for (const point of text) total += point.codePointAt(0); return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "6187488", "exact encoded result");
return __profileResult;
