// VM micro-case 096
// family=meta; operation=proxy-ownkeys; variant=6; work_units=2970; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const target = {}; for (let i = 0; i < 99; i++) target["p" + i] = i; const proxy = new Proxy(target, { ownKeys(object) { return Reflect.ownKeys(object).reverse(); } }); let total = 0; for (let i = 0; i < 30; i++) total += Reflect.ownKeys(proxy).length; return total;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "2970", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
