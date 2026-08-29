// VM micro-case 035
// family=primitives; level=1; depth=35
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const a = 35;
const b = Number(String(a));
assert(a == b && a === b && !same(a, -a - 1), "equality");
return { loose: a == b, strict: a === b, type: typeof a };
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
