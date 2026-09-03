// Repeated plain function call
// stage=interpreter-dispatch; mechanism=A hot call site to one stable function: exercises call dispatch and site-level quickening at baseline.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function addOne(x) { return x + 1; }
  let total = 0;
  for (let i = 0; i < 2000; i++) total = addOne(total);
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
