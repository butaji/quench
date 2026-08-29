// VM micro-case 444
// family=typed-memory; level=9; depth=44
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const values = new BigInt64Array([BigInt(44), -BigInt(44)]);
assert(values[0] === BigInt(44) && values[1] === -BigInt(44), "bigint typed array");
return Array.from(values, String);
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
