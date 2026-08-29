// VM micro-case 471
// family=meta-builtins; level=10; depth=21
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
const events = []; const target = { value: 21 }; const proxy = new Proxy(target, { get(object, key, receiver) { events.push(String(key)); return Reflect.get(object, key, receiver); } });
assert(proxy.value === 21 && events[0] === "value", "proxy get");
return events;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
