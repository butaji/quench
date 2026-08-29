// VM micro-case 223
// family=functions; level=5; depth=23
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
function add(a, b) { return this.bias + a + b; }
const receiver = { bias: 23 }; const value = add.call(receiver, 2, 3) + add.apply(receiver, [4, 5]) + add.bind(receiver, 6)(7);
assert(value === 96, "call apply bind");
return value;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
