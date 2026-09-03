// Accessor swapped mid-loop via defineProperty
// stage=fallback-safety-adversarial; mechanism=A plain data property is redefined as an accessor mid-loop: subsequent reads must observe the new accessor, not a stale cached slot.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const obj = { v: 1 };
  let total = 0;
  for (let i = 0; i < 300; i++) {
    if (i === 150) {
      Object.defineProperty(obj, "v", { get() { return 999; } });
    }
    total += obj.v;
  }
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
