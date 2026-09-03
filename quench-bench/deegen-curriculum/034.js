// Proxy-wrapped object defeats shape assumptions
// stage=fallback-safety-adversarial; mechanism=A Proxy intercepting property access must defeat any shape-based fast path and still produce correct trapped results.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  let getCount = 0;
  const target = { x: 1, y: 2 };
  const proxy = new Proxy(target, {
    get(obj, prop) { getCount++; return obj[prop] * 10; }
  });
  let total = 0;
  for (let i = 0; i < 300; i++) total += proxy.x + proxy.y;
  return total * 100000 + getCount;
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
