// Monomorphic property get
// stage=property-generic-ic; mechanism=Repeated get of the same property on objects sharing one stable shape.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function Point(x, y) { this.x = x; this.y = y; }
  const points = Array.from({ length: 500 }, (_, i) => new Point(i, i * 2));
  let total = 0;
  for (const p of points) total += p.x;
  for (let iter = 0; iter < 4; iter++) for (const p of points) total += p.x;
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
