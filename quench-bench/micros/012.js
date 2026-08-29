// VM micro-case 012
// family=primitives; level=1; depth=12
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const values = ["12", 12, true, null];
const coerced = values.map(Number);
assert(coerced[0] === 12 && coerced[1] === 12 && coerced[2] === 1 && coerced[3] === 0, "coercion");
return coerced;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
