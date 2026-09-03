// Mixed arithmetic operators loop
// stage=fast-path-kernel-admission; mechanism=Subtract/multiply/divide in one hot loop: exercises fast-path admission for operators beyond add.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let acc = 1000;
  for (let i = 1; i < 3000; i++) {
    acc = acc - 1;
    acc = (acc * 3) % 100003;
    acc = Math.floor(acc / 2) + i;
  }
  return acc;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
