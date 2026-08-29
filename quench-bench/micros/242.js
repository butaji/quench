// VM micro-case 242
// family=functions; level=5; depth=42
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
function factorial(n) { return n <= 1 ? 1 : n * factorial(n - 1); }
const value = factorial(6);
assert(Number.isInteger(value) && value > 0, "recursion");
return value;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
