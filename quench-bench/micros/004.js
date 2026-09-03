// VM micro-case 004
// family=numeric; operation=transcendental-scan; variant=4; work_units=1650; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 1; i <= 1650; i++) total += Math.sin(i * 0.017 + 3) * Math.cos(i * 0.013); return Math.round(total * 1e6);
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
