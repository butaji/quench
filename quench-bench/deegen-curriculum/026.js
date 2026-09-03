// Structured for-in loop, single invocation
// stage=osr-entry; mechanism=A structured (for-in-shaped) hot loop called once: per the documented ForI exclusion, this must not attempt OSR-entry and must still be correct via the ordinary loop gateway.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function runOnce() {
    const obj = {};
    for (let i = 0; i < 3000; i++) obj["k" + i] = i;
    let total = 0;
    for (const key in obj) total += obj[key];
    return total;
  }
  return runOnce();
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
