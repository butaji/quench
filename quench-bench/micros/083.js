// VM micro-case 083
// family=typed-memory; operation=float64-transform; variant=3; work_units=660; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = new Float64Array(660); let total = 0; for (let i = 0; i < values.length; i++) { values[i] = Math.sin(i + 2) * 0.5; total += values[i]; } return Math.round(total * 1e6);
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
