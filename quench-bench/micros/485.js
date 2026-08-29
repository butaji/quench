// VM micro-case 485
// family=meta-builtins; level=10; depth=35
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let message = ""; try { throw new TypeError("micro-35"); } catch (error) { assert(error instanceof TypeError && error.message === "micro-35", "error object"); message = error.name + ":" + error.message; }
const evaluated = Function("x", "return x + 1")(35);
assert(evaluated === 36, "function constructor");
return message;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
