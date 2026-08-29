// VM micro-case 213
// family=functions; level=5; depth=13
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
function add(a, b) { return this.bias + a + b; }
const receiver = { bias: 13 }; const value = add.call(receiver, 2, 3) + add.apply(receiver, [4, 5]) + add.bind(receiver, 6)(7);
assert(value === 66, "call apply bind");
return value;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
