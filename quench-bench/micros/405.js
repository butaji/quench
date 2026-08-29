// VM micro-case 405
// family=typed-memory; level=9; depth=5
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const original = new Uint16Array([5, 6, 7]); const copy = original.buffer.slice(2); const sliced = new Uint16Array(copy);
assert(sliced[0] === 6 && sliced.length === 2, "buffer slice");
return [...sliced];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
