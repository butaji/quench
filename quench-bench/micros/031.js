// VM micro-case 031
// family=objects; operation=monomorphic-load-store; variant=1; work_units=1920; memory=ephemeral
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const point = { x: 0, y: 1, z: 2 }; let total = 0; for (let i = 0; i < 1920; i++) { point.x += 1; point.y = point.x + point.z; total += point.y; } return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
