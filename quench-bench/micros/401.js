// VM micro-case 401
// family=typed-memory; level=9; depth=1
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const bytes = new Uint8Array(3); for (let i = 0; i < bytes.length; i++) bytes[i] = i * 17;
assert(bytes[0] === 0 && bytes[1] === 17, "uint8");
return [...bytes.slice(0, 4)];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
