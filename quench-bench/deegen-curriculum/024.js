// Many distinct cold functions, one call each
// stage=tier-up-threshold; mechanism=50 distinct functions each called exactly once: none should individually cross the tier-up threshold.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0;
  for (let i = 0; i < 50; i++) {
    const fn = new Function("x", "return x + " + i + ";");
    total += fn(i);
  }
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
