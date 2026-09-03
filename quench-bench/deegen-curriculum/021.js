// Hostile polymorphic property chain
// stage=fast-path-kernel-admission; mechanism=Same access chain as case 20, but shapes rotate every iteration: must defeat guarded-region admission and stay correct via fallback.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function VecA(x, y) { this.x = x; this.y = y; }
  function VecB(x, y) { this.x = x; this.y = y; this.z = 0; }
  const vecs = Array.from({ length: 400 }, (_, i) => (i % 2 === 0 ? new VecA(i, i + 1) : new VecB(i, i + 1)));
  let total = 0;
  for (const v of vecs) total += v.x + v.y;
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
