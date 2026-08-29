// VM micro-case 455
// family=meta-builtins; level=10; depth=5
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let message = ""; try { throw new TypeError("micro-5"); } catch (error) { assert(error instanceof TypeError && error.message === "micro-5", "error object"); message = error.name + ":" + error.message; }
const evaluated = Function("x", "return x + 1")(5);
assert(evaluated === 6, "function constructor");
return message;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
