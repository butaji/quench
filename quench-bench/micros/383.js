// VM micro-case 383
// family=collections; level=8; depth=33
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const key = {}; const weak = new WeakMap([[key, 33]]);
assert(weak.has(key) && weak.get(key) === 33, "weak map key");
return weak.get(key);
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
