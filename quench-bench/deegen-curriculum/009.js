// Stable call site that switches once
// stage=call-inline-cache; mechanism=A call site monomorphic for the first half, then permanently switches callee: expect one re-install, not thrashing.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function first(x) { return x - 1; }
  function second(x) { return x + 1; }
  let total = 0;
  for (let i = 0; i < 600; i++) {
    const fn = i < 300 ? first : second;
    total += fn(i);
  }
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
