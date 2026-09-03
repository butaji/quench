// Recursive self-call
// stage=call-inline-cache; mechanism=Recursive calls to one function object: the recursive call site should be monomorphic by construction.
"use strict";
const assert = (condition, message) => { if (!condition) throw new Error("micro assertion failed: " + message); };
function microRun() {
  function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
  return fib(22);
}
globalThis.microRun = microRun;
const result = microRun();
assert(result !== undefined, "result");
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
