// Call target reassigned mid-loop with side-effecting arguments
// stage=fallback-safety-adversarial; mechanism=A previously-monomorphic call site's target is reassigned partway through, with an argument expression that has a side effect: both the dispatch target and the side effect must stay correct after reassignment.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let sideEffects = 0;
  function first(x) { return x + 1; }
  function second(x) { return x * 2; }
  let fn = first;
  let total = 0;
  for (let i = 0; i < 400; i++) {
    if (i === 200) fn = second;
    total += fn((sideEffects++, i));
  }
  return total * 100000 + sideEffects;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
