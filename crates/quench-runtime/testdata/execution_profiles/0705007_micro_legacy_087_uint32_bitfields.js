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
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "0", "exact encoded result");
return __profileResult;
