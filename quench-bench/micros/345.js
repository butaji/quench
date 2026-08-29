// VM micro-case 345
// family=iterables; level=7; depth=45
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const values = []; for (const value of new Set([45, 45, 46])) values.push(value);
assert(values.length === 2 && values[0] === 45, "for of");
return values;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
