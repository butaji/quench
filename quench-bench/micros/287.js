// VM micro-case 287
// family=strings-regexp; level=6; depth=37
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const nodes = Array.from({ length: 6 }, (_, id) => ({ id, value: id }));
for (let pass = 0; pass < 5; pass++) for (let i = 1; i < nodes.length; i++) nodes[i].value = nodes[i - 1].value + 1;
assert(nodes.at(-1).value === nodes.length - 1, "constraint propagation");
return nodes.at(-1).value;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
