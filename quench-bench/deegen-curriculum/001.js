// Straight-line arithmetic recurrence
// stage=interpreter-dispatch; mechanism=Plain arithmetic dispatch: every opcode should execute via the fast/compact lane.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let value = 3;
  for (let i = 0; i < 4000; i++) value = (value * 1664525 + i + 11) | 0;
  return value >>> 0;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
