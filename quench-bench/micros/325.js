// VM micro-case 325
// family=iterables; level=7; depth=25
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const values = []; for (const value of new Set([25, 25, 26])) values.push(value);
assert(values.length === 2 && values[0] === 25, "for of");
return values;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
