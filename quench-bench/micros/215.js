// VM micro-case 215
// family=functions; level=5; depth=15
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
class Box { constructor(value) { this.value = value; } bump(amount = 1) { return this.value + amount; } static label() { return "Box"; } }
const box = new Box(15);
assert(box instanceof Box && box.bump(0) === 15 && Box.label() === "Box", "class");
return [box.value, Box.label()];
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
