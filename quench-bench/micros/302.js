// VM micro-case 302
// family=iterables; level=7; depth=2
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
function* sequence() { for (let i = 0; i < 3; i++) yield i * i; }
const values = [...sequence()];
assert(values.at(-1) === (2) ** 2, "generator");
return values;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
