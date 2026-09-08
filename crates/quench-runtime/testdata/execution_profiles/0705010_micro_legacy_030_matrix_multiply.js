// VM micro-case 030
// family=arrays; operation=matrix-multiply; variant=10; work_units=10648; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const size = 22; const left = Array.from({ length: size }, (_, row) => Array.from({ length: size }, (_, col) => row + col + 9)); const right = Array.from({ length: size }, (_, row) => Array.from({ length: size }, (_, col) => row === col ? 1 : 0)); let total = 0; for (let row = 0; row < size; row++) for (let col = 0; col < size; col++) for (let inner = 0; inner < size; inner++) total += left[row][inner] * right[inner][col]; return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "14520", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
