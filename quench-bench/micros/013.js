// VM micro-case 013
// family=control; operation=switch-dispatch; variant=3; work_units=6840; memory=none
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0; for (let i = 0; i < 6840; i++) { switch ((i + 2) % 7) { case 0: total += 3; break; case 1: total ^= 5; break; case 2: total -= 2; break; case 3: total += i & 1; break; case 4: total = (total << 1) | 1; break; case 5: total >>>= 1; break; default: total += 7; } } return total | 0;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
