// Function invoked well over the promotion threshold
// stage=tier-up-threshold; mechanism=Invoke a small function thousands of times: should cross the tier-up threshold and promote to the baseline tier.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function step(x) { return (x * 3 + 1) >>> 0; }
  let v = 7;
  for (let i = 0; i < 20000; i++) v = step(v);
  return v;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
