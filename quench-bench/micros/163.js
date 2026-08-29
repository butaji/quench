// VM micro-case 163
// family=objects; level=4; depth=13
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const parent = { base: 13 }; const child = Object.create(parent); child.own = 2;
assert(child.base === 13 && Object.hasOwn(child, "own") && !Object.hasOwn(child, "base"), "prototype");
return [child.base, Object.getPrototypeOf(child) === parent];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
