// Megamorphic call site (distinct closure per call)
// stage=call-inline-cache; mechanism=Every call sees a freshly-created closure: call IC should degrade past its bound and stay correct.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function makeAdder(n) { return function adder(x) { return x + n; }; }
  let total = 0;
  for (let i = 0; i < 300; i++) total += makeAdder(i)(i);
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
