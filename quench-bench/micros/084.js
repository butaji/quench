// VM micro-case 084
// family=typed-memory; operation=bigint64-typed; variant=4; work_units=180; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = new BigInt64Array(180); let total = 0n; for (let i = 0; i < values.length; i++) { values[i] = BigInt(i + 3); total += values[i] * 3n; } return Number(total);
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
