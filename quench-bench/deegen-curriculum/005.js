// Exception throw/catch/finally
// stage=interpreter-dispatch; mechanism=A hot loop that throws and catches on a subset of iterations: exercises the unwind/exception path deliberately.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let caught = 0, finalized = 0;
  for (let i = 0; i < 500; i++) {
    try {
      if (i % 3 === 0) throw new Error("boom");
    } catch (e) {
      caught++;
    } finally {
      finalized++;
    }
  }
  return caught * 1000 + finalized;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
