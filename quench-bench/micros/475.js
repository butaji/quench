// VM micro-case 475
// family=meta-builtins; level=10; depth=25
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let message = ""; try { throw new TypeError("micro-25"); } catch (error) { assert(error instanceof TypeError && error.message === "micro-25", "error object"); message = error.name + ":" + error.message; }
const evaluated = Function("x", "return x + 1")(25);
assert(evaluated === 26, "function constructor");
return message;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
