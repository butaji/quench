// VM micro-case 161
// family=objects; level=4; depth=11
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const object = {}; object.z = 1; object["a"] = 2; object[11] = 3;
const keys = Reflect.ownKeys(object);
assert(keys[0] === String(11) && keys[1] === "z" && keys[2] === "a", "property order");
return keys;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
