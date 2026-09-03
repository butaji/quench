// Stable property-chain hot loop
// stage=fast-path-kernel-admission; mechanism=A guarded property-access chain on one stable shape, hot enough to be a fast-path kernel candidate.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function Vec(x, y, z) { this.x = x; this.y = y; this.z = z; }
  const vecs = Array.from({ length: 400 }, (_, i) => new Vec(i, i + 1, i + 2));
  let total = 0;
  for (let iter = 0; iter < 5; iter++) for (const v of vecs) total += v.x + v.y + v.z;
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
