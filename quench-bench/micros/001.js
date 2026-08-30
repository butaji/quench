// VM micro-case 001
// family=numeric; operation=integer-recurrence; variant=1; work_units=96; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let value = 3; for (let i = 0; i < 96; i++) value = (value * 1664525 + i + 11) | 0; return value >>> 0;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
