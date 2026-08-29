// VM micro-case 094
// family=control-flow; level=2; depth=44
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let state = 0;
try { for (let i = 0; i < 44; i++) { state += i; if (i === 42) throw new RangeError("stop"); } } catch (error) { assert(error instanceof RangeError, "catch type"); state += 1; } finally { state *= 2; }
return state;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
