// VM micro-case 100
// family=meta; operation=proxy-invariant; variant=10; work_units=270; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const target = Object.freeze({ value: 9 }); const proxy = new Proxy(target, { get(object, key) { return Reflect.get(object, key); }, has(object, key) { return Reflect.has(object, key); } }); let total = 0; for (let i = 0; i < 270; i++) if ("value" in proxy) total += proxy.value; return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "2430", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
