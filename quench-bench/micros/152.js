// VM micro-case 152
// family=objects; level=4; depth=2
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const object = {}; Object.defineProperty(object, "answer", { value: 2, enumerable: false, writable: false, configurable: true });
const descriptor = Object.getOwnPropertyDescriptor(object, "answer");
assert(descriptor.value === 2 && descriptor.enumerable === false && descriptor.writable === false, "descriptor");
return [Object.keys(object), descriptor.value];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
