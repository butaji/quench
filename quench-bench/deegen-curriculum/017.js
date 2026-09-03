// Numeric add-heavy hot loop
// stage=fast-path-kernel-admission; mechanism=A tight numeric-add loop shaped to admit a fused fast-path kernel over the arithmetic sequence.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let acc = 0;
  for (let i = 0; i < 5000; i++) acc = acc + i;
  return acc;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
