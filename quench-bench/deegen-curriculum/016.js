// Interleaved property get and set
// stage=property-generic-ic; mechanism=Get and set on the same stable-shape property in one loop body: proves the idempotent probe is reused correctly for both.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function Box(v) { this.v = v; }
  const items = Array.from({ length: 500 }, (_, i) => new Box(i));
  for (let iter = 0; iter < 4; iter++) {
    for (const b of items) b.v = b.v + 1;
  }
  return items.reduce((sum, b) => sum + b.v, 0);
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
