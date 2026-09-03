// Nested hot loops, single invocation
// stage=osr-entry; mechanism=A function called once with a hot outer loop and a hot inner loop: exercises OSR-entry admission at more than one back-edge in the same invocation.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function runOnce() {
    let acc = 0;
    for (let i = 0; i < 400; i++) {
      for (let j = 0; j < 400; j++) {
        acc = (acc + i * 31 + j) | 0;
      }
    }
    return acc >>> 0;
  }
  return runOnce();
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
