// VM micro-case 005
// family=numeric; operation=bigint-accumulator; variant=5; work_units=1880; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0n; for (let i = 0n; i < 1880n; i++) total = (total * 6364136223846793005n + i + 5n) & 0xffffffffffffffffn; return Number(total & 0xffffffffn);
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "29482204", "exact encoded result");
return __profileResult;
