// VM micro-case 085
// family=typed-memory; operation=buffer-copy-slice; variant=5; work_units=840; memory=external
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const source = new Uint16Array(840); for (let i = 0; i < source.length; i++) source[i] = i + 4; const copy = source.buffer.slice(2); const view = new Uint16Array(copy); let total = 0; for (const value of view) total += value; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "355736", "exact encoded result");
return __profileResult;
