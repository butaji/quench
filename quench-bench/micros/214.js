// VM micro-case 214
// family=functions; level=5; depth=14
"use strict";
const assert = (condition, message) => {
  if (!condition) throw new Error("micro assertion failed: " + message);
};
const same = (a, b) => Object.is(a, b);
const result = (() => {
function Box(value) { this.value = value; }
Box.prototype.bump = function () { return ++this.value; }; const box = new Box(14);
assert(box instanceof Box && box.bump() === 15, "constructor prototype");
return box.value;
})();
const emit = typeof console !== "undefined" && typeof console.log === "function" ? console.log.bind(console) : (typeof print === "function" ? print : () => {});
emit("ok:" + JSON.stringify(result));
