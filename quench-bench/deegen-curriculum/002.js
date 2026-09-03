// Branch ladder dispatch
// stage=interpreter-dispatch; mechanism=if/else and switch-style branching: dispatch stays on the fast lane across all branch outcomes.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let total = 0;
  for (let i = 0; i < 3000; i++) {
    if (i % 7 === 0) total += 3;
    else if (i % 5 === 0) total += 2;
    else if (i % 3 === 0) total -= 1;
    else total += 1;
  }
  return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
