// VM micro-case 091
// family=meta; operation=proxy-get-set; variant=1; work_units=108; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const target = { value: 0 }; let gets = 0; const proxy = new Proxy(target, { get(object, key, receiver) { gets++; return Reflect.get(object, key, receiver); }, set(object, key, value, receiver) { return Reflect.set(object, key, value, receiver); } }); for (let i = 0; i < 108; i++) { proxy.value = proxy.value + 1; } return proxy.value + gets;
}
function run() {
  return microRun();
}
function verify(result) {
assert(Number.isFinite(result), "result");
assert(typeof microRun === "function", "scenario entry is callable");
const __profileResult = JSON.stringify(result);
assert(__profileResult === "217", "exact encoded result");
  return __profileResult;
}
return { run: run, verify: verify };
