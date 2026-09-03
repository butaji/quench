// VM micro-case 007
// family=numeric; operation=float-rounding; variant=7; work_units=4680; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let value = 6.125; for (let i = 0; i < 4680; i++) value = Math.fround(value * 1.00031 - 0.00017); return Math.fround(value);
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
