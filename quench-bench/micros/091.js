// VM micro-case 091
// family=meta; operation=proxy-get-set; variant=1; work_units=108; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const target = { value: 0 }; let gets = 0; const proxy = new Proxy(target, { get(object, key, receiver) { gets++; return Reflect.get(object, key, receiver); }, set(object, key, value, receiver) { return Reflect.set(object, key, value, receiver); } }); for (let i = 0; i < 108; i++) { proxy.value = proxy.value + 1; } return proxy.value + gets;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
