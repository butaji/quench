// VM micro-case 083
// family=control-flow; level=2; depth=33
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
let label = "";
for (let i = 0; i < 33; i++) { switch (i % 3) { case 0: label += "a"; break; case 1: label += "b"; break; default: label += "c"; } }
assert(label.length === 33, "switch");
return label;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
