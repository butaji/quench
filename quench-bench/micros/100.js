// VM micro-case 100
// family=meta; operation=proxy-invariant; variant=10; work_units=270; memory=allocation-heavy
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  const target = Object.freeze({ value: 9 }); const proxy = new Proxy(target, { get(object, key) { return Reflect.get(object, key); }, has(object, key) { return Reflect.has(object, key); } }); let total = 0; for (let i = 0; i < 270; i++) if ("value" in proxy) total += proxy.value; return total;
}
globalThis.microRun = microRun;
const result = microRun();
assert(Number.isFinite(result), "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
