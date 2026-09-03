// Guard invalidation mid-loop
// stage=property-generic-ic; mechanism=A stable shape is mutated by adding a new property mid-loop: the IC's guard must re-probe, not trust a stale state.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function Node(v) { this.v = v; }
  const items = Array.from({ length: 400 }, (_, i) => new Node(i));
  let total = 0;
  for (let i = 0; i < items.length; i++) {
    if (i === 200) for (const it of items) it.extra = 0;
    total += items[i].v;
  }
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
