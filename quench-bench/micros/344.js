// VM micro-case 344
// family=iterables; level=7; depth=44
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const left = { x: 44, y: 2 }; const right = { y: 3, z: 4 }; const merged = { ...left, ...right };
assert(merged.x === 44 && merged.y === 3 && merged.z === 4, "object spread");
return Object.keys(merged);
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
