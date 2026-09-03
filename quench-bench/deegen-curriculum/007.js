// Bounded polymorphic call site
// stage=call-inline-cache; mechanism=A call site rotating across 3 stable callees: should become bounded polymorphic, not degrade.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function a(x) { return x + 1; }
  function b(x) { return x + 2; }
  function c(x) { return x + 3; }
  const fns = [a, b, c];
  let total = 0;
  for (let i = 0; i < 900; i++) total += fns[i % 3](i);
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
