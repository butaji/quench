// VM micro-case 087
// family=typed-memory; operation=uint32-bitfields; variant=7; work_units=1020; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const values = new Uint32Array(1020); let total = 0; for (let i = 0; i < values.length; i++) { values[i] = (i << 16) | (i ^ 6); total ^= values[i] >>> 5; } return total >>> 0;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
