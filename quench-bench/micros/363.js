// VM micro-case 363
// family=collections; level=8; depth=13
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const key = {}; const weak = new WeakMap([[key, 13]]);
assert(weak.has(key) && weak.get(key) === 13, "weak map key");
return weak.get(key);
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
