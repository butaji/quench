// VM micro-case 341
// family=iterables; level=7; depth=41
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const iterable = { [Symbol.iterator]() { let i = 0; return { next() { return i < 6 ? { value: i++, done: false } : { value: undefined, done: true }; } }; } };
const values = [...iterable];
assert(values.length === 6 && values[0] === 0, "iterator protocol");
return values;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
