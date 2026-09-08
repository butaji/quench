// VM micro-case 093
// family=meta; operation=json-roundtrip; variant=3; work_units=1728; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const source = { id: 2, values: Array.from({ length: 96 }, (_, i) => ({ index: i, value: i * i })) }; let total = 0; for (let i = 0; i < 18; i++) { const copy = JSON.parse(JSON.stringify(source)); total += copy.values.length + copy.id; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "1764", "exact encoded result");
return __profileResult;
