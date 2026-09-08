// VM micro-case 001
// family=numeric; operation=integer-recurrence; variant=1; work_units=1920; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let value = 3; for (let i = 0; i < 1920; i++) value = (value * 1664525 + i + 11) | 0; return value >>> 0;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));

const __profileResult = JSON.stringify(result);
if (__profileResult !== "1563355587") throw new Error("legacy exact result mismatch");
return __profileResult;
