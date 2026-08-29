// VM micro-case 404
// family=typed-memory; level=9; depth=4
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const values = new BigInt64Array([BigInt(4), -BigInt(4)]);
assert(values[0] === BigInt(4) && values[1] === -BigInt(4), "bigint typed array");
return Array.from(values, String);
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
