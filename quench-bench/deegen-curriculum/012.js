// Monomorphic property set
// stage=property-generic-ic; mechanism=Repeated set of the same property on objects sharing one stable shape.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function Counter() { this.value = 0; }
  const items = Array.from({ length: 500 }, () => new Counter());
  for (let iter = 0; iter < 5; iter++) for (const c of items) c.value = c.value + 1;
  return items.reduce((sum, c) => sum + c.value, 0);
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
