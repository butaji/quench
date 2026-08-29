// VM micro-case 497
// family=meta-builtins; level=10; depth=47
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const nodes = Array.from({ length: 16 }, (_, id) => ({ id, value: id }));
for (let pass = 0; pass < 7; pass++) for (let i = 1; i < nodes.length; i++) nodes[i].value = nodes[i - 1].value + 1;
assert(nodes.at(-1).value === nodes.length - 1, "constraint propagation");
return nodes.at(-1).value;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
