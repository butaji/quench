// VM micro-case 165
// family=objects; level=4; depth=15
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const key = Symbol("micro"); const object = { [key]: 15, plain: true };
assert(object[key] === 15 && Reflect.ownKeys(object).includes(key), "symbol key");
return [typeof key, object.plain, object[key]];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
