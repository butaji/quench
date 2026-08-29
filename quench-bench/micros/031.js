// VM micro-case 031
// family=primitives; level=1; depth=31
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let total = 0;
for (let i = 0; i < 31; i++) total = (total + i * 3 + 1) % 997;
assert(total === (31 * (31 - 1) * 3 / 2 + 31) % 997, "arithmetic");
return { total, inf: 1 / 0, nan: Number.isNaN(0 / 0) };
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
