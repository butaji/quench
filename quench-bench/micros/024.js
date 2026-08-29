// VM micro-case 024
// family=primitives; level=1; depth=24
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const negativeZero = -0;
const values = [negativeZero, Number.MIN_VALUE, Number.MAX_SAFE_INTEGER, BigInt(24)];
assert(same(values[0], -0) && values[1] > 0 && Number.isSafeInteger(values[2]), "numeric edges");
return [Object.is(values[0], -0), String(values[3])];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
