// VM micro-case 202
// family=functions; level=5; depth=2
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
function factorial(n) { return n <= 1 ? 1 : n * factorial(n - 1); }
const value = factorial(2);
assert(Number.isInteger(value) && value > 0, "recursion");
return value;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
