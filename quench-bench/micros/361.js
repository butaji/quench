// VM micro-case 361
// family=collections; level=8; depth=11
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const map = new Map(); map.set("first", 11); map.set("second", 12); map.set("first", 13);
assert(map.size === 2 && map.get("first") === 13, "map identity");
return [...map.keys()];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
