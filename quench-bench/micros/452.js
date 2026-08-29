// VM micro-case 452
// family=meta-builtins; level=10; depth=2
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const object = {}; Reflect.defineProperty(object, "value", { value: 2, enumerable: true });
assert(Reflect.has(object, "value") && Reflect.get(object, "value") === 2, "reflect");
return Reflect.ownKeys(object);
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
