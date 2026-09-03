// Loop control: break and continue
// stage=interpreter-dispatch; mechanism=Nested loops with break/continue: control-flow opcodes stay on the fast dispatch path.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let count = 0;
  outer: for (let i = 0; i < 200; i++) {
    for (let j = 0; j < 200; j++) {
      if (j === 150) continue outer;
      if (i === 190) break outer;
      count++;
    }
  }
  return count;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
