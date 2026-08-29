// VM micro-case 332
// family=iterables; level=7; depth=32
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
function* sequence() { for (let i = 0; i < 6; i++) yield i * i; }
const values = [...sequence()];
assert(values.at(-1) === (5) ** 2, "generator");
return values;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
