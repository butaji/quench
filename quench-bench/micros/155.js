// VM micro-case 155
// family=objects; level=4; depth=5
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const key = Symbol("micro"); const object = { [key]: 5, plain: true };
assert(object[key] === 5 && Reflect.ownKeys(object).includes(key), "symbol key");
return [typeof key, object.plain, object[key]];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
