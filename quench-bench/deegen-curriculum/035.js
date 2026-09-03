// Getter with an observable side effect
// stage=fallback-safety-adversarial; mechanism=A property getter with a side effect must run exactly once per access, even under repeated access at a hot call site.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let calls = 0;
  const obj = {
    get value() { calls++; return calls; }
  };
  let total = 0;
  for (let i = 0; i < 200; i++) total += obj.value;
  return total * 100000 + calls;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
