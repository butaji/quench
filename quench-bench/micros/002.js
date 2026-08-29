// VM micro-case 002
// family=primitives; level=1; depth=2
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const values = ["2", 2, true, null];
const coerced = values.map(Number);
assert(coerced[0] === 2 && coerced[1] === 2 && coerced[2] === 1 && coerced[3] === 0, "coercion");
return coerced;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
