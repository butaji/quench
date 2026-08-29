// VM micro-case 321
// family=iterables; level=7; depth=21
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const iterable = { [Symbol.iterator]() { let i = 0; return { next() { return i < 4 ? { value: i++, done: false } : { value: undefined, done: true }; } }; } };
const values = [...iterable];
assert(values.length === 4 && values[0] === 0, "iterator protocol");
return values;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
