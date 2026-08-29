// VM micro-case 351
// family=collections; level=8; depth=1
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const map = new Map(); map.set("first", 1); map.set("second", 2); map.set("first", 3);
assert(map.size === 2 && map.get("first") === 3, "map identity");
return [...map.keys()];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
