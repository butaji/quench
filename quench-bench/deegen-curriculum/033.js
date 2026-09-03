// Polymorphic dispatch inside a hot loop
// stage=full-system-closure; mechanism=A hot loop dispatching to one of several handler shapes each iteration: combines call-IC polymorphism with a hot loop.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function Add(n) { this.apply = (x) => x + n; }
  function Mul(n) { this.apply = (x) => x * n; }
  function Neg() { this.apply = (x) => -x; }
  const handlers = [new Add(3), new Mul(2), new Neg(), new Add(-1)];
  let value = 1;
  for (let i = 0; i < 4000; i++) value = handlers[i % handlers.length].apply(value % 1000);
  return value;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
