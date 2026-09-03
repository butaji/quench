// Polymorphic property get (3 shapes)
// stage=property-generic-ic; mechanism=Property reads rotate across 3 distinct, stable shapes: expect bounded polymorphic behavior, not degrade.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function A(v) { this.tag = "a"; this.v = v; }
  function B(v) { this.tag = "b"; this.v = v; this.extra = 0; }
  function C(v) { this.tag = "c"; this.v = v; this.extra = 0; this.more = 0; }
  const ctors = [A, B, C];
  const items = Array.from({ length: 600 }, (_, i) => new ctors[i % 3](i));
  let total = 0;
  for (const it of items) total += it.v;
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
