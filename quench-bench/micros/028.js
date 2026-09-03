// VM micro-case 028
// family=arrays; operation=ring-buffer; variant=8; work_units=3120; memory=retained-ring
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
const ring = new Array(32).fill(0);
function microRun() {
  ring.fill(0); let total = 0; for (let i = 0; i < 3120; i++) { ring[i & 31] = (i + 7) & 255; total += ring[(i + 7) & 31]; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
