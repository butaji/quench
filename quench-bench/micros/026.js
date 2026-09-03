// VM micro-case 026
// family=arrays; operation=prefix-sum; variant=6; work_units=660; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = Array.from({ length: 660 }, (_, i) => (i + 5) & 31); for (let i = 1; i < values.length; i++) values[i] += values[i - 1]; return values.at(-1);
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
