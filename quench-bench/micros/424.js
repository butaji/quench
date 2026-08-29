// VM micro-case 424
// family=typed-memory; level=9; depth=24
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const values = new BigInt64Array([BigInt(24), -BigInt(24)]);
assert(values[0] === BigInt(24) && values[1] === -BigInt(24), "bigint typed array");
return Array.from(values, String);
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
