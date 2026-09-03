// VM micro-case 082
// family=typed-memory; operation=dataview-endian; variant=2; work_units=570; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const buffer = new ArrayBuffer(2280); const view = new DataView(buffer); let total = 0; for (let i = 0; i < 570; i++) { view.setInt32((i % 570) * 4, i + 1, (i & 1) === 0); total += view.getInt32((i % 570) * 4, (i & 1) === 0); } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
