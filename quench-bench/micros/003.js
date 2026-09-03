// VM micro-case 003
// family=numeric; operation=bitwise-mixer; variant=3; work_units=5680; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let bits = -1640531525; for (let i = 0; i < 5680; i++) bits = Math.imul(bits ^ (bits >>> 13), 0x85ebca6b) | 0; return bits >>> 0;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
