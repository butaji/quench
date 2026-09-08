// VM micro-case 022
// family=arrays; operation=sparse-probe; variant=2; work_units=420; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const sparse = []; for (let i = 0; i < 420; i += 3) sparse[i * 2] = i + 1; let total = 0; for (let i = 0; i < sparse.length; i++) if (i in sparse) total += sparse[i]; return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "29330", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
