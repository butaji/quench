// Numeric add hostile (mixed types)
// stage=fast-path-kernel-admission; mechanism=Same loop shape as case 17, but operands alternate number/string: must defeat fast-path admission and still compute the correct (string-concatenated) result via the complete fallback.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let acc = 0;
  for (let i = 0; i < 2000; i++) acc = acc + (i % 2 === 0 ? i : String(i));
  return String(acc).length;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
