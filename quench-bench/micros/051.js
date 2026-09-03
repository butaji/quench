// VM micro-case 051
// family=strings; operation=concat-rope; variant=1; work_units=336; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let text = ""; for (let i = 0; i < 336; i++) text += String.fromCharCode(65 + ((i + 0) % 26)) + i; return text.length;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
