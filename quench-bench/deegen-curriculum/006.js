// Monomorphic call site
// stage=call-inline-cache; mechanism=One callee called repeatedly at the same call site: should install and hold a monomorphic call IC.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function square(x) { return x * x; }
  let total = 0;
  for (let i = 0; i < 800; i++) total += square(i % 37);
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
