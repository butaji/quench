// VM micro-case 465
// family=meta-builtins; level=10; depth=15
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let message = ""; try { throw new TypeError("micro-15"); } catch (error) { assert(error instanceof TypeError && error.message === "micro-15", "error object"); message = error.name + ":" + error.message; }
const evaluated = Function("x", "return x + 1")(15);
assert(evaluated === 16, "function constructor");
return message;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
