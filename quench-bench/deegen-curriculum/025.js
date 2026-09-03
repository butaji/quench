// Single-invocation hot loop (paper's OSR motivating case)
// stage=osr-entry; mechanism=A function called exactly once, containing a very hot loop: tier-up-on-entry can never fire (only one invocation), so only OSR-entry mid-loop could promote this.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function runOnce() {
    let acc = 0;
    for (let i = 0; i < 200000; i++) acc = (acc + i * 3 - 1) | 0;
    return acc >>> 0;
  }
  return runOnce();
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
